use std::borrow::Cow;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::Context as _;
use colorful::Colorful;
use edgedb_derive::Queryable;
use edgedb_errors::{Error, QueryError, InvalidSyntaxError};
use edgeql_parser::expr;
use edgeql_parser::hash::Hasher;
use edgeql_parser::schema_file::validate;
use edgeql_parser::tokenizer::{Tokenizer, Kind as TokenKind};
use fn_error_context::context;
use immutable_chunkmap::set::SetM as Set;
use once_cell::sync::OnceCell;
use rustyline::error::ReadlineError;
use serde::Deserialize;
use tokio::fs;
use tokio::io::{self, AsyncWriteExt};
use tokio::task::spawn_blocking as unblock;

use crate::async_try;
use crate::bug;
use crate::commands::{Options, ExitCode};
use crate::connect::Connection;
use crate::migrations::edb::{execute, execute_if_connected, query_row};
use crate::error_display::print_query_error;
use crate::highlight;
use crate::migrations::context::Context;
use crate::migrations::dev_mode;
use crate::migrations::migration;
use crate::migrations::options::CreateMigration;
use crate::migrations::print_error::print_migration_error;
use crate::migrations::prompt;
use crate::migrations::source_map::{Builder, SourceMap};
use crate::migrations::squash;
use crate::migrations::timeout;
use crate::platform::tmp_file_name;
use crate::print::style::Styler;
use crate::print;
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

#[derive(Deserialize, Debug, Clone)]
pub struct RequiredUserInput {
    placeholder: String,
    prompt: String,
    #[allow(dead_code)]
    old_type: Option<String>,
    old_type_is_object: Option<bool>,
    new_type: Option<String>,
    new_type_is_object: Option<bool>,
    #[serde(rename="type")]
    type_name: Option<String>,
    pointer_name: Option<String>,
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

#[derive(Debug)]
pub enum MigrationKey {
    Index(u64),
    Fixup { target_revision: String },
}

pub trait MigrationToText {
    type StatementsIter<'a>: Iterator<Item = &'a String> where Self: 'a;
    fn key(&self) -> &MigrationKey;
    fn parent(&self) -> anyhow::Result<&str>;
    fn id(&self) -> anyhow::Result<&str>;
    fn statements<'a>(&'a self) -> Self::StatementsIter<'a>;
}

#[derive(Debug)]
pub struct FutureMigration {
    key: MigrationKey,
    parent: String,
    statements: Vec<String>,
    id: OnceCell<String>,
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

#[derive(Debug, thiserror::Error)]
#[error("EdgeDB could not resolve migration automatically. \
         Please run `edgedb migration create` in interactive mode.")]
struct CantResolve;

#[derive(Debug, thiserror::Error)]
#[error("cannot proceed until .esdl files are fixed")]
pub struct EsdlError;

impl FutureMigration {
    fn new(key: MigrationKey, descr: CurrentMigration) -> Self {
        FutureMigration {
            key,
            parent: descr.parent,
            statements: descr.confirmed,
            id: OnceCell::new(),
        }
    }
    pub fn empty(key: MigrationKey, parent: &str) -> Self {
        FutureMigration {
            key,
            parent: parent.to_owned(),
            statements: Vec::new(),
            id: OnceCell::new(),
        }
    }
}

impl MigrationToText for FutureMigration {
    type StatementsIter<'a> = std::slice::Iter<'a, String>;

    fn key(&self) -> &MigrationKey {
        &self.key
    }

    fn parent(&self) -> anyhow::Result<&str> {
        Ok(&self.parent)
    }

    fn id(&self) -> anyhow::Result<&str> {
        let FutureMigration { ref parent, ref statements, ref id, .. } = self;
        id.get_or_try_init(|| {
            let mut hasher = Hasher::start_migration(parent);
            for statement in statements {
                hasher.add_source(&statement)
                    .map_err(|e| migration::hashing_error(statement, e))?;
            }
            Ok(hasher.make_migration_id())
        }).map(|s| &s[..])
    }

    fn statements<'a>(&'a self) -> Self::StatementsIter<'a> {
        self.statements.iter()
    }
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

async fn choice(prompt: &str) -> anyhow::Result<Choice> {
    use Choice::*;

    let mut q = question::Choice::new(prompt.to_string());
    q.option(Yes, &["y", "yes"],
        "Confirm the prompt, use the DDL statements");
    q.option(No, &["n", "no"],
        "Reject the prompt");
    q.option(List, &["l", "list"],
        "List DDL statements associated with the prompt");
    q.option(Confirmed, &["c", "confirmed"],
        "List already confirmed EdgeQL statements");
    q.option(Back, &["b", "back"],
        "Revert to previous save point");
    q.option(Split, &["s", "stop"],
        "Stop and save changes (splits migration into multiple)");
    q.option(Quit, &["q", "quit"],
        "Quit without saving changes");
    q.async_ask().await
}

#[context("could not read schema in {}", ctx.schema_dir.display())]
async fn gen_start_migration(ctx: &Context)
    -> anyhow::Result<(String, SourceMap<SourceName>)>
{
    let mut bld = Builder::new();
    bld.add_lines(SourceName::Prefix, "START MIGRATION TO {");
    let mut dir = match fs::read_dir(&ctx.schema_dir).await {
        Ok(dir) => dir,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            bld.add_lines(SourceName::Suffix, "};");
            return Ok(bld.done());
        }
        Err(e) => Err(e).context(format!("cannot read {:?}", ctx.schema_dir))?,
    };
    while let Some(item) = dir.next_entry().await? {
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
            return Err(EsdlError)?;
        }
        Err(e) => Err(e)?,
    }
}

pub async fn first_migration(cli: &mut Connection, ctx: &Context,
                         options: &CreateMigration)
    -> anyhow::Result<FutureMigration>
{
    execute_start_migration(&ctx, cli).await?;
    async_try! {
        async {
            execute(cli, "POPULATE MIGRATION").await?;
            let descr = query_row::<CurrentMigration>(cli,
                "DESCRIBE CURRENT MIGRATION AS JSON"
            ).await?;
            if descr.parent != "initial" {
                // We know there are zero revisions in the filesystem
                anyhow::bail!("No database revision {} in \
                    the filesystem. Consider updating sources.",
                    descr.parent);
            }
            if !descr.complete {
                return Err(bug::error("First migration population is not complete"));
            }
            if descr.confirmed.is_empty() && !options.allow_empty {
                print::warn("No schema changes detected.");
                return Err(ExitCode::new(4))?;
            }
            Ok(FutureMigration::new(MigrationKey::Index(1), descr))
        },
        finally async {
            execute_if_connected(cli, "ABORT MIGRATION").await
        }
    }
}

pub fn make_default_expression(input: &RequiredUserInput)
    -> Option<String>
{
    let name = &input.placeholder[..];
    let kind_end = name.find("_expr").unwrap_or(name.len());
    let expr = match &name[..kind_end] {
        "fill" if input.type_name.is_some() => {
            format!("<{}>{{}}",
                    input.type_name.as_ref().unwrap())
        }
        "cast"
            if input.pointer_name.is_some() &&
               input.new_type.is_some()
        => {
            let pointer_name = input.pointer_name.as_deref().unwrap();
            let new_type = input.new_type.as_deref().unwrap();
            match (input.old_type_is_object, input.new_type_is_object) {
                (Some(true), Some(true)) => {
                    format!(".{pointer_name}[IS {new_type}]")
                }
                (Some(false), Some(false)) | (None, None) => {
                    format!("<{new_type}>.{pointer_name}")
                }
                // TODO(tailhook) maybe create something for mixed case?
                _ => return None,
            }
        }
        "conv" if input.pointer_name.is_some() => {
            format!("(SELECT .{} LIMIT 1)",
                    input.pointer_name.as_ref().unwrap())
        }
        _ => {
            return None;
        }
    };
    Some(expr)
}

pub async fn unsafe_populate(_ctx: &Context, cli: &mut Connection)
    -> anyhow::Result<CurrentMigration>
{
    loop {
        let data = query_row::<CurrentMigration>(cli,
            "DESCRIBE CURRENT MIGRATION AS JSON"
        ).await?;
        if data.complete {
            return Ok(data);
        }
        if let Some(proposal) = &data.proposed {
            let mut placeholders = BTreeMap::new();
            if !proposal.required_user_input.is_empty() {
                for input in &proposal.required_user_input {
                    let Some(expr) = make_default_expression(input) else {
                        log::debug!("Cannot fill placeholder {} \
                                    into {:?}, input info: {:?}",
                                    input.placeholder,
                                    proposal.statements,
                                    input);
                        return Err(CantResolve)?;
                    };
                    placeholders.insert(input.placeholder.clone(), expr);
                }
            }
            if !apply_proposal(cli, &proposal, &placeholders).await? {
                execute(cli, "ALTER CURRENT MIGRATION REJECT PROPOSED").await?;
            }
        } else {
            log::debug!("No proposal generated");
            return Err(CantResolve)?;
        }
    }
}

async fn apply_proposal(cli: &mut Connection, proposal: &Proposal,
                        placeholders: &BTreeMap<String, String>)
    -> anyhow::Result<bool>
{
    execute(cli, "DECLARE SAVEPOINT proposal").await?;
    let mut rollback = false;
    async_try!{
        async {
            for statement in &proposal.statements {
                let statement = substitute_placeholders(
                    &statement.text, &placeholders)?;
                if let Err(e) = execute(cli, &statement).await {
                    if e.is::<InvalidSyntaxError>() {
                        log::error!("Error executing: {}", statement);
                        return Err(e)?;
                    } else if e.is::<QueryError>() {
                        rollback = true;
                        log::info!("Statement {:?} failed: {:#}",
                                   statement, e);
                        return Ok(false);
                    } else {
                        return Err(e)?;
                    }
                }
            }
            Ok(true)
        },
        finally async {
            if rollback {
                execute_if_connected(cli,
                    "ROLLBACK TO SAVEPOINT proposal",
                ).await?;
            }
            execute_if_connected(cli, "RELEASE SAVEPOINT proposal").await
        }
    }
}

async fn non_interactive_populate(_ctx: &Context, cli: &mut Connection)
    -> anyhow::Result<CurrentMigration>
{
    loop {
        let data = query_row::<CurrentMigration>(cli,
            "DESCRIBE CURRENT MIGRATION AS JSON"
        ).await?;
        if data.complete {
            return Ok(data);
        }
        if let Some(proposal) = data.proposed {
            if proposal.confidence >= SAFE_CONFIDENCE {
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
                eprintln!("EdgeDB intended to apply the following migration:");
                for statement in proposal.statements {
                    for line in statement.text.lines() {
                        eprintln!("    {}", line);
                    }
                }
                eprintln!("But confidence is {}, below minimum threshold of {}",
                    proposal.confidence, SAFE_CONFIDENCE);
                anyhow::bail!("EdgeDB is unable to make a decision. Please run in \
                    interactive mode to confirm changes, \
                    or use `--allow-unsafe`");
            }
        } else {
            anyhow::bail!("EdgeDB could not resolve \
                migration automatically. Please run in \
                interactive mode to confirm changes, \
                or use `--allow-unsafe`");
        }
    }
}

async fn run_non_interactive(ctx: &Context, cli: &mut Connection,
                             key: MigrationKey, options: &CreateMigration)
    -> anyhow::Result<FutureMigration>
{
    let descr = if options.allow_unsafe {
        unsafe_populate(ctx, cli).await?
    } else {
        non_interactive_populate(ctx, cli).await?
    };
    if descr.confirmed.is_empty() && !options.allow_empty {
        print::warn("No schema changes detected.");
        //print::echo!("Hint: --allow-empty can be used to create a data-only migration with no schema changes.");
        return Err(ExitCode::new(4))?;
    }
    Ok(FutureMigration::new(key, descr))
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
                let input = self.cli.ping_while(
                    get_user_input(&proposal.required_user_input)
                ).await;
                match input {
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
                match self.cli.ping_while(choice(prompt)).await? {
                    Yes => {
                        let input_res = self.cli.ping_while(
                            get_user_input(&proposal.required_user_input)
                        ).await;
                        match input_res {
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
                                "No EdgeQL statements have been confirmed.");
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
                            "Migration aborted; no results were saved."
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
                        print_query_error(&e, &text, false, "<statement>")?;
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
                    if self.cli.is_consistent() {
                        eprintln!("Rolling back last operation...");
                        self.rollback().await?;
                        return Ok(());
                    } else {
                        return Err(ExitCode::new(1).into());
                    }
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


async fn run_interactive(_ctx: &Context, cli: &mut Connection,
                         key: MigrationKey, options: &CreateMigration)
    -> anyhow::Result<FutureMigration>
{
    let descr = InteractiveMigration::new(cli).run().await?;

    if descr.confirmed.is_empty() && !options.allow_empty {
        print::warn("No schema changes detected.");
        //print::echo!("Hint: --allow-empty can be used to create a data-only migration with no schema changes.");
        return Err(ExitCode::new(4))?;
    }
    Ok(FutureMigration::new(key, descr))
}

pub async fn write_migration(ctx: &Context, descr: &impl MigrationToText,
    verbose: bool)
    -> anyhow::Result<()>
{
    let filename = match &descr.key() {
        MigrationKey::Index(idx) => {
            let dir = ctx.schema_dir.join("migrations");
            dir.join(format!("{:05}.edgeql", idx))
        }
        MigrationKey::Fixup { target_revision } => {
            let dir = ctx.schema_dir.join("fixups");
            dir.join(format!("{}-{}.edgeql", descr.parent()?, target_revision))
        }
    };
    _write_migration(descr, filename.as_ref(), verbose).await
}

#[context("could not write migration file {}", filepath.display())]
async fn _write_migration(descr: &impl MigrationToText, filepath: &Path,
    verbose: bool)
    -> anyhow::Result<()>
{
    let id = descr.id()?;
    let dir = filepath.parent().unwrap();
    let tmp_file = filepath.with_file_name(tmp_file_name(&filepath.as_ref()));
    if !fs::metadata(filepath).await.is_ok() {
        fs::create_dir_all(&dir).await?;
    }
    fs::remove_file(&tmp_file).await.ok();
    let mut file = io::BufWriter::new(fs::File::create(&tmp_file).await?);
    file.write(format!("CREATE MIGRATION {}\n", id).as_bytes()).await?;
    file.write(format!("    ONTO {}\n", descr.parent()?).as_bytes()).await?;
    file.write(b"{\n").await?;
    for statement in descr.statements() {
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

pub async fn create(cli: &mut Connection, options: &Options,
    create: &CreateMigration)
    -> anyhow::Result<()>
{
    if create.squash {
        squash::main(cli, options, create).await
    } else {
        let old_state = cli.set_ignore_error_state();
        let res = _create(cli, options, create).await;
        cli.restore_state(old_state);
        return res;
    }
}

async fn _create(cli: &mut Connection, options: &Options,
    create: &CreateMigration)
    -> anyhow::Result<()>
{
    let ctx = Context::from_project_or_config(&create.cfg, false).await?;

    if dev_mode::check_client(cli).await? {
        let dev_num = query_row::<i64>(cli, "SELECT count((
            SELECT schema::Migration
            FILTER .generated_by = schema::MigrationGeneratedBy.DevMode
        ))").await?;
        if dev_num > 0 {
            log::info!("Detected dev-mode migrations");
            return dev_mode::create(cli, &ctx, options, create).await;
        }
    }

    let migrations = migration::read_all(&ctx, true).await?;
    let old_timeout = timeout::inhibit_for_transaction(cli).await?;
    let migration = async_try! {
        async {
            // This decision must be done early on because
            // of the bug in EdgeDB:
            //   https://github.com/edgedb/edgedb/issues/3958
            if migrations.len() == 0 {
                first_migration(cli, &ctx, create).await
            } else {
                let key = MigrationKey::Index((migrations.len() + 1) as u64);
                let parent = migrations.keys().last().map(|x| &x[..]);
                normal_migration(cli, &ctx, key, parent, create).await
            }
        },
        finally async {
            timeout::restore_for_transaction(cli, old_timeout).await
        }
    }?;
    write_migration(&ctx, &migration, !create.non_interactive).await?;
    Ok(())
}

pub async fn normal_migration(cli: &mut Connection, ctx: &Context,
                              key: MigrationKey,
                              ensure_parent: Option<&str>,
                              create: &CreateMigration)
    -> anyhow::Result<FutureMigration>
{
    execute_start_migration(&ctx, cli).await?;
    async_try! {
        async {
            let descr = query_row::<CurrentMigration>(cli,
                "DESCRIBE CURRENT MIGRATION AS JSON",
            ).await?;
            let db_migration = if descr.parent == "initial" {
                None
            } else {
                Some(&descr.parent[..])
            };
            if db_migration != ensure_parent {
                anyhow::bail!("Database must be updated to the last migration \
                    on the filesystem for `migration create`. Run:\n  \
                    edgedb migrate");
            }

            if create.non_interactive {
                run_non_interactive(&ctx, cli, key, &create).await
            } else {
                if create.allow_unsafe {
                    log::warn!("The `--allow-unsafe` flag is unused \
                                in interactive mode");
                }
                run_interactive(&ctx, cli, key, &create).await
            }
        },
        finally async {
            execute_if_connected(cli, "ABORT MIGRATION").await
        }
    }
}

fn add_newline_after_comment(value: &mut String) -> Result<(), anyhow::Error> {
    let last_token = Tokenizer::new(value).last()
        .ok_or_else(|| bug::error("input should not be empty"))?
        .map_err(|e| bug::error(
            format!("tokenizer failed on reparsing: {e:#}")))?;
    let token_end = last_token.span.end as usize;
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
    let mut prev = make_default_expression(req).unwrap_or(String::new());
    loop {
        println!("{}:", req.prompt);
        let mut value = match prompt::expression(&prompt, &req.placeholder, &prev) {
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
                prev = value;
                continue;
            }
        }
        add_newline_after_comment(&mut value)?;
        return Ok(value);
    }
}

async fn get_user_input(req: &[RequiredUserInput])
    -> Result<BTreeMap<String, String>, anyhow::Error>
{
    let mut result = BTreeMap::new();
    for item in req {
        let copy = item.clone();
        let input = unblock(move || get_input(&copy)).await??;
        result.insert(item.placeholder.clone(), input);
    }
    Ok(result)
}

fn substitute_placeholders<'x>(input: &'x str,
    placeholders: &BTreeMap<String, String>)
    -> Result<Cow<'x, str>, anyhow::Error>
{
    let mut output = String::with_capacity(input.len());
    let mut parser = Tokenizer::new(input);
    let mut start = 0;
    for item in &mut parser {
        let token = match item {
            Ok(item) => item,
            Err(e) => Err(bug::error(format!(
                "the server sent an invalid query: {e:#}")))?,
        };
        if token.kind == TokenKind::Substitution {
            output.push_str(&input[start..token.span.start as usize]);
            let name = token.text.strip_prefix(r"\(")
                .and_then(|item| item.strip_suffix(")"))
                .ok_or_else(|| bug::error(format!("bad substitution token")))?;
            let expr = placeholders.get(name)
                .ok_or_else(|| bug::error(format!(
                    "missing input for {:?} placeholder", name)))?;
            output.push_str(expr);
            start = token.span.end as usize;
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
