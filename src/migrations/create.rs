use async_std::fs;
use async_std::io::prelude::WriteExt;
use async_std::io;
use async_std::path::{Path, PathBuf};
use async_std::stream::StreamExt;
use edgedb_client::client::Connection;
use edgedb_derive::Queryable;
use edgedb_protocol::queryable::Queryable;
use edgedb_protocol::server_message::ErrorResponse;
use edgedb_protocol::value::Value;
use edgeql_parser::preparser::{full_statement, is_empty};
use fn_error_context::context;
use serde::Deserialize;

use crate::commands::parser::CreateMigration;
use crate::commands::{Options, ExitCode};
use crate::platform::tmp_file_name;
use crate::migrations::context::Context;
use crate::migrations::migration;
use crate::migrations::print_error::print_migration_error;
use crate::migrations::source_map::{Builder, SourceMap};

const SAFE_CONFIDENCE: f64 = 0.99999;

pub enum SourceName {
    Prefix,
    Suffix,
    File(PathBuf),
}

pub enum Choice {
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
    name: String,
    prompt: String,
}

#[derive(Deserialize, Debug)]
pub struct StatementProposal {
    pub text: String,
    #[serde(default)]
    pub required_user_input: Vec<RequiredUserInput>,
}

#[derive(Deserialize, Debug)]
pub struct Proposal {
    pub statements: Vec<StatementProposal>,
    pub confidence: f64,
    #[serde(default)]
    pub prompt: Option<String>,
}

#[derive(Deserialize, Queryable, Debug)]
#[edgedb(json)]
pub struct CurrentMigration {
    pub complete: bool,
    pub parent: String,
    pub confirmed: Vec<String>,
    pub proposed: Option<Proposal>,
}

async fn execute(cli: &mut Connection, text: impl AsRef<str>)
    -> anyhow::Result<()>
{
    let text = text.as_ref();
    log::debug!(target: "edgedb::migrations::query", "Executing `{}`", text);
    cli.execute(text).await?;
    Ok(())
}

async fn query_row<R>(cli: &mut Connection, text: &str)
    -> anyhow::Result<R>
    where R: Queryable
{
    let text = text.as_ref();
    log::debug!(target: "edgedb::migrations::query", "Executing `{}`", text);
    cli.query_row(text, &Value::empty_tuple()).await
}

#[context("could not read schema file {}", path.display())]
async fn read_schema_file(path: &Path) -> anyhow::Result<String> {
    let data = fs::read_to_string(path).await?;
    let mut offset = 0;
    loop {
        match full_statement(data[offset..].as_bytes(), None) {
            Ok(shift) => offset += shift,
            Err(_) => {
                if !is_empty(&data[offset..]) {
                    anyhow::bail!("final statement does not \
                                   end with a semicolon");
                }
                return Ok(data);
            }
        }
    }
}

async fn choice(prompt: &str) -> anyhow::Result<Choice> {
    use Choice::*;

    const HELP: &str = r###"
y - confirm the prompt, use the DDL statements
n - reject the prompt
l - list the DDL statements associated with prompt
c - list already confirmed EdgeQL statements
b - revert back to previous save point, perhaps previous question
s - stop and save changes (splits migration into multiple)
q - quit without saving changes
h or ? - print help
"###;

    let mut input = String::with_capacity(10);
    loop {
        println!("{} [y,n,l,c,b,s,q,?]", prompt);
        input.truncate(0);
        if io::stdin().read_line(&mut input).await? == 0 {
            return Ok(Quit);
        }
        let val = match &input.trim().to_lowercase()[..] {
            "y"|"yes" => Yes,
            "n"|"no" => No,
            "l"|"list" => List,
            "c"|"confirmed" => Confirmed,
            "b"|"back" => Back,
            "s"|"stop"|"split" => Split,
            "h"|"?"|"help" => {
                print!("{}", HELP);
                continue;
            }
            "q"|"quit" => Quit,
            val => {
                eprintln!("Error: unknown command {}", val);
                continue;
            }
        };
        return Ok(val);
    }
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
        bld.add_lines(SourceName::File(path), &chunk);
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
        Err(e) => match e.downcast::<ErrorResponse>() {
            Ok(e) => {
                print_migration_error(&e, &source_map)?;
                anyhow::bail!("cannot proceed until .esdl files are fixed");
            }
            Err(e) => Err(e)?,
        }
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
                for statement in proposal.statements {
                    if !statement.required_user_input.is_empty() {
                        for input in statement.required_user_input {
                            eprintln!("Input required: {}", input.prompt);
                        }
                        anyhow::bail!(
                            "cannot apply `{}` without user input",
                            statement.text);
                    }
                    execute(cli, &statement.text).await?;
                }
            } else {
                eprintln!("Server is about to apply the following migration:");
                for statement in proposal.statements {
                    for line in statement.text.lines() {
                        eprintln!("    {}", line);
                    }
                }
                eprintln!("But confidence is {} (minimum is {})",
                    proposal.confidence, SAFE_CONFIDENCE);
                anyhow::bail!("Server cannot make decision. Please run in \
                    interactive mode to confirm changes, \
                    or use `--allow-unsafe`");
            }
        } else {
            anyhow::bail!("Server could not figure out \
                migration automatically. Please run in \
                interactive mode to confirm changes, \
                or use `--allow-unsafe`");
        }
    };
    if descr.confirmed.is_empty() && !options.allow_empty {
        eprintln!("No schema changes detected.");
        return Err(ExitCode::new(4))?;
    }
    write_migration(ctx, &descr, index, false).await?;
    Ok(())
}

async fn run_interactive(ctx: &Context, cli: &mut Connection, index: u64,
    options: &CreateMigration)
    -> anyhow::Result<()>
{
    use Choice::*;

    let mut save_point = 0;
    execute(cli, format!("DECLARE SAVEPOINT migration_{}", save_point)).await?;
    let descr = 'migration: loop {
        let descr = query_row::<CurrentMigration>(cli,
            "DESCRIBE CURRENT MIGRATION AS JSON",
        ).await?;
        if descr.complete {
            break descr;
        }
        if let Some(proposal) = &descr.proposed {
            let prompt = if let Some(prompt) = &proposal.prompt {
                prompt
            } else {
                println!("Following DDL statements will be applied:");
                for statement in &proposal.statements {
                    for line in statement.text.lines() {
                        println!("    {}", line);
                    }
                }
                "Apply the DDL statements?"
            };
            loop {
                match choice(prompt).await? {
                    Yes => break,
                    No => {
                        execute(cli,
                            "ALTER CURRENT MIGRATION REJECT PROPOSED"
                        ).await?;
                        save_point += 1;
                        execute(cli,
                            format!("DECLARE SAVEPOINT migration_{}",
                                     save_point)
                        ).await?;
                        continue 'migration;
                    }
                    List => {
                        println!("Following DDL statements will be applied:");
                        for statement in &proposal.statements {
                            for line in statement.text.lines() {
                                println!("    {}", line);
                            }
                        }
                        continue;
                    }
                    Confirmed => {
                        if descr.confirmed.is_empty() {
                            println!(
                                "No EdgeQL statements were confirmed yet");
                        } else {
                            println!(
                                "Following EdgeQL statements were confirmed:");
                            for statement in &descr.confirmed {
                                for line in statement.lines() {
                                    println!("    {}", line);
                                }
                            }
                        }
                        continue;
                    }
                    Back => {
                        if save_point == 0 {
                            eprintln!("Already at latest savepoint");
                            continue;
                        }
                        save_point -= 1;
                        execute(cli, format!(
                            "ROLLBACK TO SAVEPOINT migration_{}", save_point)
                        ).await?;
                        continue 'migration;
                    }
                    Split => {
                        break 'migration descr;
                    }
                    Quit => {
                        eprintln!("Migration aborted no results are saved.");
                        return Err(ExitCode::new(0))?;
                    }
                }
            }
            for statement in &proposal.statements {
                if !statement.required_user_input.is_empty() {
                    for input in &statement.required_user_input {
                        eprintln!("Input required: {}", input.prompt);
                    }
                    anyhow::bail!(
                        "cannot apply `{}` without user input. \
                         User input is not implemented yet",
                        statement.text);
                }
                execute(cli, &statement.text).await?;
            }
            save_point += 1;
            execute(cli,
                format!("DECLARE SAVEPOINT migration_{}", save_point)
            ).await?;
        } else {
            anyhow::bail!("Server could not figure out \
                migration with your answers. \
                Please retry with different answers");
        }
    };
    if descr.confirmed.is_empty() && !options.allow_empty {
        eprintln!("No schema changes detected.");
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
    let mut hasher = migration::Hasher::new(&descr.parent);
    for statement in &statements {
        hasher.source(&statement)?;
    }
    let id = hasher.make_id();
    let dir = filepath.parent().unwrap();
    let tmp_file = filepath.with_file_name(tmp_file_name(&filepath.as_ref()));
    if !filepath.exists().await {
        fs::create_dir_all(&dir).await?;
    }
    if verbose {
        eprintln!("Created {}, id: {}", filepath.display(), id);
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
        anyhow::bail!("Database must be updated to the last miration \
            on the filesystem for `create-migration`. Run:\n  \
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
    let abort = cli.execute("ABORT MIGRATION").await;
    exec.and(abort)?;
    Ok(())
}
