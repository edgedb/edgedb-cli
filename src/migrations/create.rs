use std::borrow::Cow;
use std::collections::BTreeMap;

use async_std::fs;
use async_std::io::prelude::WriteExt;
use async_std::io;
use async_std::path::{Path, PathBuf};
use async_std::stream::StreamExt;
use colorful::Colorful;
use edgedb_client::client::Connection;
use edgedb_client::errors::{Error, QueryError};
use edgedb_derive::Queryable;
use edgedb_protocol::queryable::Queryable;
use edgeql_parser::hash::Hasher;
use edgeql_parser::expr;
use edgeql_parser::tokenizer::{TokenStream, Kind as TokenKind};
use edgeql_parser::schema_file::validate;
use fn_error_context::context;
use immutable_chunkmap::set::Set;
use rustyline::error::ReadlineError;
use serde::Deserialize;

use crate::bug;
use crate::commands::parser::CreateMigration;
use crate::commands::{Options, ExitCode};
use crate::error_display::print_query_error;
use crate::highlight;
use crate::migrations::context::Context;
use crate::migrations::migration;
use crate::migrations::print_error::print_migration_error;
use crate::migrations::prompt;
use crate::migrations::source_map::{Builder, SourceMap};
use crate::platform::tmp_file_name;
use crate::print;
use crate::print::style::Styler;
use crate::question;

const SAFE_CONFIDENCE: f64 = 0.99999;

pub enum SourceName {
    Prefix,
    Semicolon(PathBuf),
    Suffix,
    File(PathBuf),
}

#[derive(Clone, Debug)]
enum Choice {
    Yes,
    No,
    List,
    Confirmed,
    Back,
    Split,
    Quit,
}

#[derive(Deserialize, Debug)]
pub struct RequiredUserInput {
    placeholder: String,
    prompt: String,
}

#[derive(Deserialize, Debug)]
pub struct StatementProposal {
    pub text: String,
}

#[derive(Deserialize, Debug)]
pub struct Proposal {
    pub prompt_id: Option<String>,
    pub statements: Vec<StatementProposal>,
    pub confidence: f64,
    #[serde(default)]
    pub prompt: Option<String>,
    #[serde(default)]
    pub required_user_input: Vec<RequiredUserInput>,
}

#[derive(Deserialize, Queryable, Debug)]
#[edgedb(json)]
pub struct CurrentMigration {
    pub complete: bool,
    pub parent: String,
    pub confirmed: Vec<String>,
    pub proposed: Option<Proposal>,
}

struct InteractiveMigration<'a> {
    cli: &'a mut Connection,
    save_point: usize,
    operations: Vec<Set<String>>,
    confirmed: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
#[error("refused to input data required for placeholder")]
struct Refused;

#[derive(Debug, thiserror::Error)]
#[error("split migration")]
struct SplitMigration;

async fn execute(cli: &mut Connection, text: impl AsRef<str>)
    -> Result<(), Error>
{
    let text = text.as_ref();
    log::debug!(target: "edgedb::migrations::query", "Executing `{}`", text);
    cli.execute(text).await?;
    Ok(())
}

async fn query_row<R>(cli: &mut Connection, text: &str)
    -> Result<R, Error>
    where R: Queryable
{
    let text = text.as_ref();
    log::debug!(target: "edgedb::migrations::query", "Executing `{}`", text);
    cli.query_row(text, &()).await
}

#[context("could not read schema file {}", path.display())]
async fn read_schema_file(path: &Path) -> anyhow::Result<String> {
    let data = fs::read_to_string(path).await?;
    validate(&data)?;
    Ok(data)
}

fn print_statements(statements: impl IntoIterator<Item=impl AsRef<str>>) {
    let mut buf: String = String::with_capacity(1024);
    let styler = Styler::dark_256();
    for statement in statements {
        buf.truncate(0);
        highlight::edgeql(&mut buf, statement.as_ref(), &styler);
        for line in buf.lines() {
            println!("    {}", line);
        }
    }
}

fn choice(prompt: &str) -> anyhow::Result<Choice> {
    use Choice::*;

    let mut q = question::Choice::new(prompt.to_string());
    q.option(Yes, &["y", "yes"],
        "confirm the prompt, use the DDL statements");
    q.option(No, &["n", "no"],
        "reject the prompt");
    q.option(List, &["l", "list"],
        "list the DDL statements associated with prompt");
    q.option(Confirmed, &["c", "confirmed"],
        "list already confirmed EdgeQL statements");
    q.option(Back, &["b", "back"],
        "revert back to previous save point");
    q.option(Split, &["s", "stop"],
        "stop and save changes (splits migration into multiple)");
    q.option(Quit, &["q", "quit"],
        "quit without saving changes");
    q.ask()
}

#[context("could not read schema in {}", ctx.schema_dir.display())]
async fn gen_start_migration(ctx: &Context)
    -> anyhow::Result<(String, SourceMap<SourceName>)>
{
    let mut bld = Builder::new();
    bld.add_lines(SourceName::Prefix, "START MIGRATION TO {");
    let mut dir = fs::read_dir(&ctx.schema_dir).await?;
    while let Some(item) = dir.next().await.transpose()? {
        let fname = item.file_name();
        let lossy_name = fname.to_string_lossy();
        if lossy_name.starts_with(".") || !lossy_name.ends_with(".esdl")
            || !item.file_type().await?.is_file()
        {
            continue;
        }
        let path = item.path();
        let chunk = read_schema_file(&path).await?;
        bld.add_lines(SourceName::File(path.clone()), &chunk);
        bld.add_lines(SourceName::Semicolon(path), ";");
    }
    bld.add_lines(SourceName::Suffix, "};");
    Ok(bld.done())
}

pub async fn execute_start_migration(ctx: &Context, cli: &mut Connection)
    -> anyhow::Result<()>
{
    let (text, source_map) = gen_start_migration(&ctx).await?;
    match execute(cli, text).await {
        Ok(_) => Ok(()),
        Err(e) if e.is::<QueryError>() => {
            print_migration_error(&e, &source_map)?;
            anyhow::bail!("cannot proceed until .esdl files are fixed");
        }
        Err(e) => Err(e)?,
    }
}

async fn run_non_interactive(ctx: &Context, cli: &mut Connection, index: u64,
    options: &CreateMigration)
    -> anyhow::Result<()>
{
    let descr = loop {
        let data = query_row::<CurrentMigration>(cli,
            "DESCRIBE CURRENT MIGRATION AS JSON"
        ).await?;
        if data.complete {
            break data;
        }
        if let Some(proposal) = data.proposed {
            if proposal.confidence >= SAFE_CONFIDENCE || options.allow_unsafe {
                if !proposal.required_user_input.is_empty() {
                    for input in proposal.required_user_input {
                        eprintln!("Input required: {}", input.prompt);
                    }
                    anyhow::bail!("Cannot apply migration without user input");
                }
                for statement in proposal.statements {
                    execute(cli, &statement.text).await?;
                }
            } else {
                eprintln!("EdgeDB is about to apply the following migration:");
                for statement in proposal.statements {
                    for line in statement.text.lines() {
                        eprintln!("    {}", line);
                    }
                }
                eprintln!("But confidence is {} (minimum is {})",
                    proposal.confidence, SAFE_CONFIDENCE);
                anyhow::bail!("EdgeDB cannot make decision. Please run in \
                    interactive mode to confirm changes, \
                    or use `--allow-unsafe`");
            }
        } else {
            anyhow::bail!("EdgeDB could not resolve \
                migration automatically. Please run in \
                interactive mode to confirm changes, \
                or use `--allow-unsafe`");
        }
    };
    if descr.confirmed.is_empty() && !options.allow_empty {
        print::warn("No schema changes detected.");
        return Err(ExitCode::new(4))?;
    }
    write_migration(ctx, &descr, index, false).await?;
    Ok(())
}

impl InteractiveMigration<'_> {
    fn new(cli: &mut Connection) -> InteractiveMigration {
        InteractiveMigration {
            cli,
            save_point: 0,
            operations: vec![Set::new()],
            confirmed: Vec::new(),
        }
    }
    async fn save_point(&mut self) -> Result<(), Error> {
        execute(self.cli,
            format!("DECLARE SAVEPOINT migration_{}", self.save_point)
        ).await
    }
    async fn rollback(&mut self) -> Result<(), Error> {
        execute(self.cli, format!(
            "ROLLBACK TO SAVEPOINT migration_{}", self.save_point)
        ).await
    }
    async fn run(mut self) -> anyhow::Result<CurrentMigration> {
        self.save_point().await?;
        loop {
            let descr = query_row::<CurrentMigration>(self.cli,
                "DESCRIBE CURRENT MIGRATION AS JSON",
            ).await?;
            self.confirmed = descr.confirmed.clone();
            if descr.complete {
                return Ok(descr);
            }
            if let Some(proposal) = &descr.proposed {
                match self.process_proposal(proposal).await {
                    Err(e) if e.is::<SplitMigration>() => return Ok(descr),
                    Err(e) => return Err(e),
                    Ok(()) => {}
                }
            } else {
                self.could_not_resolve().await?;
            }
        }
    }
    async fn process_proposal(&mut self, proposal: &Proposal)
        -> anyhow::Result<()>
    {
        use Choice::*;

        let cur_oper = self.operations.last().unwrap();
        let already_approved = proposal.prompt_id.as_ref()
            .map(|op| cur_oper.contains(op))
            .unwrap_or(false);
        let input;
        if already_approved {
            input = loop {
                println!("The following extra DDL statements will be applied:");
                for statement in &proposal.statements {
                    for line in statement.text.lines() {
                        println!("    {}", line);
                    }
                }
                println!("(approved as part of an earlier prompt)");
                match get_user_input(&proposal.required_user_input) {
                    Ok(data) => break data,
                    Err(e) if e.is::<Refused>() => {
                        // TODO(tailhook) ask if we want to rollback or quit
                        continue;
                    }
                    Err(e) => return Err(e.into()),
                };
            };
        } else {
            let prompt = if let Some(prompt) = &proposal.prompt {
                prompt
            } else {
                println!("The following DDL statements will be applied:");
                print_statements(proposal.statements.iter().map(|s| &s.text));
                "Apply the DDL statements?"
            };
            loop {
                match choice(prompt)? {
                    Yes => {
                        match get_user_input(&proposal.required_user_input) {
                            Ok(data) => input = data,
                            Err(e) if e.is::<Refused>() => continue,
                            Err(e) => return Err(e.into()),
                        };
                        break;
                    }
                    No => {
                        execute(self.cli,
                            "ALTER CURRENT MIGRATION REJECT PROPOSED"
                        ).await?;
                        self.save_point += 1;
                        self.save_point().await?;
                        return Ok(());
                    }
                    List => {
                        println!("The following DDL statements will be applied:");
                        print_statements(
                            proposal.statements.iter().map(|s| &s.text)
                        );
                        continue;
                    }
                    Confirmed => {
                        if self.confirmed.is_empty() {
                            println!(
                                "No EdgeQL statements were confirmed yet");
                        } else {
                            println!(
                                "The following EdgeQL statements were confirmed:");
                            print_statements(&self.confirmed);
                        }
                        continue;
                    }
                    Back => {
                        if self.save_point == 0 {
                            eprintln!("Already at latest savepoint");
                            continue;
                        }
                        self.save_point -= 1;
                        self.rollback().await?;
                        self.operations.truncate(self.save_point + 1);
                        return Ok(());
                    }
                    Split => {
                        return Err(SplitMigration.into());
                    }
                    Quit => {
                        print::error(
                            "Migration aborted; no results are saved."
                        );
                        return Err(ExitCode::new(0))?;
                    }
                }
            }
        }
        for statement in &proposal.statements {
            let text = substitute_placeholders(&statement.text, &input)?;
            match execute(self.cli, &text).await {
                Ok(()) => {}
                Err(e) => {
                    if e.is::<QueryError>() {
                        print_query_error(&e, &text, false)?;
                    } else {
                        if print::use_color() {
                            eprintln!(
                                "{}: {:#}",
                                "Error applying statement"
                                    .bold().light_red(),
                                e.to_string().bold().white(),
                            );
                        } else {
                            eprintln!("Error applying statement: {:#}", e);
                        }
                    }
                    eprintln!("Rolling back last operation...");
                    self.rollback().await?;
                    return Ok(());
                }
            }
        }
        if let Some(prompt_id) = &proposal.prompt_id {
            self.operations.push(
                self.operations.last().unwrap().insert(prompt_id.clone()).0
            );
        } else {
            self.operations.push(self.operations.last().unwrap().clone());
        }
        self.save_point += 1;
        self.save_point().await?;
        Ok(())
    }
    async fn could_not_resolve(&mut self) -> anyhow::Result<()> {
        // TODO(tailhook) allow rollback
        anyhow::bail!("EdgeDB could not resolve \
            migration with the provided answers. \
            Please retry with different answers.");
    }
}


async fn run_interactive(ctx: &Context, cli: &mut Connection, index: u64,
    options: &CreateMigration)
    -> anyhow::Result<()>
{
    let descr = InteractiveMigration::new(cli).run().await?;

    if descr.confirmed.is_empty() && !options.allow_empty {
        print::warn("No schema changes detected.");
        return Err(ExitCode::new(4))?;
    }
    write_migration(ctx, &descr, index, true).await?;
    Ok(())
}

pub async fn write_migration(ctx: &Context, descr: &CurrentMigration,
    index: u64, verbose: bool)
    -> anyhow::Result<()>
{
    let dir = ctx.schema_dir.join("migrations");
    let filename = dir.join(format!("{:05}.edgeql", index));
    _write_migration(descr, filename.as_ref(), verbose).await
}

#[context("could not write migration file {}", filepath.display())]
async fn _write_migration(descr: &CurrentMigration, filepath: &Path,
    verbose: bool)
    -> anyhow::Result<()>
{
    let statements = descr.confirmed.iter()
        .map(|s| s.clone())
        .collect::<Vec<_>>();
    let mut hasher = Hasher::start_migration(&descr.parent);
    for statement in &statements {
        hasher.add_source(&statement)
            .map_err(|e| migration::hashing_error(statement, e))?;
    }
    let id = hasher.make_migration_id();
    let dir = filepath.parent().unwrap();
    let tmp_file = filepath.with_file_name(tmp_file_name(&filepath.as_ref()));
    if !filepath.exists().await {
        fs::create_dir_all(&dir).await?;
    }
    fs::remove_file(&tmp_file).await.ok();
    let mut file = io::BufWriter::new(fs::File::create(&tmp_file).await?);
    file.write(format!("CREATE MIGRATION {}\n", id).as_bytes()).await?;
    file.write(format!("    ONTO {}\n", descr.parent).as_bytes()).await?;
    file.write(b"{\n").await?;
    for statement in &statements {
        for line in statement.lines() {
            file.write(&format!("  {}\n", line).as_bytes()).await?;
        }
    }
    file.write(b"};\n").await?;
    file.flush().await?;
    drop(file);
    fs::rename(&tmp_file, &filepath).await?;
    if verbose {
        if print::use_color() {
            eprintln!(
                "{} {}, id: {}",
                "Created".bold().light_green(),
                filepath.display().to_string().bold().white(),
                id,
            );
        } else {
            eprintln!("Created {}, id: {}", filepath.display(), id);
        }
    }
    Ok(())
}

pub async fn create(cli: &mut Connection, _options: &Options,
    create: &CreateMigration)
    -> Result<(), anyhow::Error>
{
    let ctx = Context::from_config(&create.cfg);
    let migrations = migration::read_all(&ctx, true).await?;
    execute_start_migration(&ctx, cli).await?;
    let descr = query_row::<CurrentMigration>(cli,
        "DESCRIBE CURRENT MIGRATION AS JSON",
    ).await?;
    let db_migration = if descr.parent == "initial" {
        None
    } else {
        Some(&descr.parent)
    };
    if db_migration != migrations.keys().last() {
        anyhow::bail!("Database must be updated to the last migration \
            on the filesystem for `migration create`. Run:\n  \
            edgedb migrate");
    }

    let exec = if create.non_interactive {
        run_non_interactive(&ctx, cli, migrations.len() as u64 +1,
            &create).await
    } else {
        if create.allow_unsafe {
            log::warn!(
                "The `--allow-unsafe` flag is unused in interactive mode");
        }
        run_interactive(&ctx, cli, migrations.len() as u64 + 1, &create).await
    };
    let abort = cli.execute("ABORT MIGRATION").await.map_err(|e| e.into());
    exec.and(abort)?;
    Ok(())
}

fn add_newline_after_comment(value: &mut String) -> Result<(), anyhow::Error> {
    let last_token = TokenStream::new(value).last()
        .ok_or_else(|| bug::error("input should not be empty"))?
        .map_err(|e| bug::error(
            format!("tokenizer failed on reparsing: {}", e)))?;
    let token_end = last_token.end.offset as usize;
    if token_end < value.len()
        && !value[token_end..].trim().is_empty()
    {
        // Non-empty data after last token means comment.
        // Let's add a newline after input to make sure that
        // adding data after the input is safe
        value.push('\n');
    }
    Ok(())
}

fn get_input(req: &RequiredUserInput) -> Result<String, anyhow::Error> {
    let prompt = format!("{}> ", req.placeholder);
    loop {
        println!("{}:", req.prompt);
        let mut value = match prompt::expression(&prompt, &req.placeholder) {
            Ok(val) => val,
            Err(e) => match e.downcast::<ReadlineError>() {
                Ok(ReadlineError::Eof) => return Err(Refused.into()),
                Ok(e) => return Err(e.into()),
                Err(e) => return Err(e),
            },
        };
        match expr::check(&value) {
            Ok(()) => {}
            Err(e) => {
                println!("Invalid expression: {}", e);
                continue;
            }
        }
        add_newline_after_comment(&mut value)?;
        return Ok(value);
    }
}

fn get_user_input(req: &[RequiredUserInput])
    -> Result<BTreeMap<String, String>, anyhow::Error>
{
    let mut result = BTreeMap::new();
    for item in req {
        result.insert(item.placeholder.clone(), get_input(item)?);
    }
    Ok(result)
}

fn substitute_placeholders<'x>(input: &'x str,
    placeholders: &BTreeMap<String, String>)
    -> Result<Cow<'x, str>, anyhow::Error>
{
    let mut output = String::with_capacity(input.len());
    let mut parser = TokenStream::new(input);
    let mut start = 0;
    for item in &mut parser {
        let item = match item {
            Ok(item) => item,
            Err(e) => Err(bug::error(format!(
                "the server sent an invalid query: {}", e)))?,
        };
        if item.token.kind == TokenKind::Substitution {
            output.push_str(&input[start..item.start.offset as usize]);
            let name = item.token.value.strip_prefix(r"\(")
                .and_then(|item| item.strip_suffix(")"))
                .ok_or_else(|| bug::error(format!("bad substitution token")))?;
            let expr = placeholders.get(name)
                .ok_or_else(|| bug::error(format!(
                    "missing input for {:?} placeholder", name)))?;
            output.push_str(expr);
            start = item.end.offset as usize;
        }
    }
    if start == 0 {
        return Ok(input.into());
    }
    output.push_str(&input[start..]);
    Ok(output.into())
}

#[test]
fn placeholders() {
    let mut inputs = BTreeMap::new();
    inputs.insert("one".into(), " 1 ".into());
    inputs.insert("two".into(), "'two'".into());
    assert_eq!(substitute_placeholders(r"SELECT \(one);", &inputs).unwrap(),
        "SELECT  1 ;");
    assert_eq!(
        substitute_placeholders(r"SELECT {\(one), \(two)};", &inputs).unwrap(),
        "SELECT { 1 , 'two'};");
}

#[test]
fn add_newline() {
    fn wrapper(s: &str) -> String {
        let mut data = s.to_string();
        add_newline_after_comment(&mut data).unwrap();
        return data;
    }
    assert_eq!(wrapper("1+1"), "1+1");
    assert_eq!(wrapper("1    "), "1    ");
    assert_eq!(wrapper("1  #xx  "), "1  #xx  \n");
    assert_eq!(wrapper("(1 + 7) #xx"), "(1 + 7) #xx\n");
    assert_eq!(wrapper("(1 #one\n + 3 #three\n)"), "(1 #one\n + 3 #three\n)");
}
