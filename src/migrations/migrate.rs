use std::collections::{HashMap, VecDeque};
use std::path::Path;

use anyhow::Context as _;
use colorful::Colorful;
use gel_protocol::common::{
    Capabilities, Cardinality, CompilationOptions, InputLanguage, IoFormat,
};
use indexmap::IndexMap;
use indicatif::ProgressBar;
use tokio::fs;

use crate::async_try;
use crate::branding::BRANDING_CLI_CMD;
use crate::bug;
use crate::commands::ExitCode;
use crate::commands::Options;
use crate::connect::{Connection, ResponseStream};
use crate::error_display::print_query_error;
use crate::hint::HintExt;
use crate::hooks;
use crate::migrations::context::Context;
use crate::migrations::db_migration;
use crate::migrations::db_migration::{DBMigration, MigrationGeneratedBy};
use crate::migrations::dev_mode;
use crate::migrations::edb::{execute, execute_if_connected};
use crate::migrations::migration::{self, MigrationFile};
use crate::migrations::options::Migrate;
use crate::migrations::timeout;
use crate::print;

#[derive(Debug, Clone, Copy)]
pub enum Operation<'a> {
    Apply(&'a MigrationFile),
    Rewrite(&'a indexmap::map::Slice<String, MigrationFile>),
}

#[cfg_attr(test, derive(Debug))]
enum PathElem<'a> {
    Fixup(&'a MigrationFile),
    Normal(&'a MigrationFile),
}

type OperationIter<'a> = Box<dyn Iterator<Item = Operation<'a>> + Send + 'a>;

pub trait AsOperations {
    fn as_operations(&self) -> OperationIter<'_>;
}

#[derive(Debug, thiserror::Error)]
#[error("error in one of the migrations")]
pub struct ApplyMigrationError;

fn slice<'x, M>(
    migrations: &'x IndexMap<String, M>,
    // start is exclusive and end is inclusive
    start: Option<&String>,
    end: Option<&String>,
) -> anyhow::Result<&'x indexmap::map::Slice<String, M>> {
    let start_index = start
        .and_then(|m| migrations.get_index_of(m))
        .map(|idx| idx + 1)
        .unwrap_or(0); // this zero is for start=None, get_index_of returning
                       // None should never happen as we switch for `fixup`
    let end_index = end
        .and_then(|m| migrations.get_index_of(m))
        .map(|idx| idx + 1)
        .unwrap_or(migrations.len());
    migrations
        .get_range(start_index..end_index)
        .ok_or_else(|| bug::error("slicing error"))
}

impl AsOperations for indexmap::map::Slice<String, MigrationFile> {
    fn as_operations(&self) -> OperationIter<'_> {
        Box::new(self.values().map(Operation::Apply))
    }
}

impl AsOperations for IndexMap<String, MigrationFile> {
    fn as_operations(&self) -> OperationIter<'_> {
        Box::new(self.values().map(Operation::Apply))
    }
}

impl<'a> AsOperations for Vec<Operation<'a>> {
    fn as_operations(&self) -> OperationIter<'_> {
        Box::new(self.iter().cloned())
    }
}

pub async fn migrate(
    cli: &mut Connection,
    _options: &Options,
    migrate: &Migrate,
) -> Result<(), anyhow::Error> {
    let old_state = cli.set_ignore_error_state();
    let res = do_migrate(cli, migrate).await;
    cli.restore_state(old_state);
    res
}

async fn do_migrate(cli: &mut Connection, migrate: &Migrate) -> Result<(), anyhow::Error> {
    let ctx = Context::for_migration_config(&migrate.cfg, migrate.quiet).await?;
    if migrate.dev_mode {
        // TODO(tailhook) figure out progressbar in non-quiet mode
        return dev_mode::migrate(cli, &ctx, &ProgressBar::hidden()).await;
    }
    let migrations = migration::read_all(&ctx, true).await?;
    let db_migrations = db_migration::read_all(cli, false, true).await?;
    let last_db_rev = db_migrations.last().map(|kv| kv.0);

    let target_rev = if let Some(prefix) = &migrate.to_revision {
        let db_rev = db_migration::find_by_prefix(cli, prefix).await?;
        let file_revs = migrations
            .iter()
            .filter(|r| r.0.starts_with(prefix))
            .map(|r| &r.1.data)
            .collect::<Vec<_>>();
        if file_revs.len() > 1 {
            anyhow::bail!("More than one revision matches prefix {:?}", prefix);
        }
        let target_rev = match (&db_rev, file_revs.last()) {
            (None, None) => {
                anyhow::bail!("No revision with prefix {:?} found", prefix);
            }
            (None, Some(targ)) => &targ.id,
            (Some(a), Some(b)) if a.name != b.id => {
                anyhow::bail!("More than one revision matches prefix {:?}", prefix);
            }
            (Some(_), Some(targ)) => &targ.id,
            (Some(targ), None) => &targ.name,
        };
        if let Some(db_rev) = db_rev {
            if !migrate.quiet {
                let mut msg = "Database is up to date.".to_string();
                if print::use_color() {
                    msg = format!("{}", msg.bold().light_green());
                }
                if Some(&db_rev.name) == last_db_rev {
                    eprintln!("{} Revision {}", msg, db_rev.name);
                } else {
                    eprintln!(
                        "{} Revision {} is the ancestor of the latest {}",
                        msg,
                        db_rev.name,
                        last_db_rev.unwrap_or(&String::from("initial")),
                    );
                }
            }
            return Err(ExitCode::new(0))?;
        }
        Some(target_rev.clone())
    } else {
        None
    };

    if let Some(last_db_rev) = last_db_rev {
        if let Some(last) = migrations.last() {
            if !migrations.contains_key(last_db_rev) {
                let target_rev = target_rev.as_ref().unwrap_or(last.0);
                return fixup(cli, &ctx, &migrations, &db_migrations, target_rev, migrate).await;
            }
        } else {
            return Err(anyhow::anyhow!(
                "there is applied migration history in the database \
                 but {:?} is empty",
                ctx.schema_dir.join("migrations"),
            )
            .with_hint(|| {
                format!(
                    "You might have an incorrect or outdated source checkout. \
                     If you don't, consider running `{BRANDING_CLI_CMD} migration extract` \
                     to bring the history in {:?} in sync with the database.",
                    ctx.schema_dir.join("migrations")
                )
            }))?;
        }
    };
    let migrations = slice(&migrations, last_db_rev, target_rev.as_ref())?;
    if migrations.is_empty() {
        if !migrate.quiet {
            if print::use_color() {
                eprintln!(
                    "{} Revision {}",
                    "Everything is up to date.".bold().light_green(),
                    last_db_rev
                        .map(|m| &m[..])
                        .unwrap_or("initial")
                        .bold()
                        .white(),
                );
            } else {
                eprintln!(
                    "Everything is up to date. Revision {}",
                    last_db_rev.map(|m| &m[..]).unwrap_or("initial"),
                );
            }
        }
        return Ok(());
    }
    apply_migrations(cli, migrations, &ctx, migrate.single_transaction).await?;
    if db_migrations.is_empty() {
        disable_ddl(cli).await?;
    }
    Ok(())
}

async fn fixup(
    cli: &mut Connection,
    ctx: &Context,
    migrations: &IndexMap<String, MigrationFile>,
    db_migrations: &IndexMap<String, DBMigration>,
    target: &String,
    _options: &Migrate,
) -> anyhow::Result<()> {
    let fixups = migration::read_fixups(ctx, true).await?;
    let last_db_migration = db_migrations
        .last()
        .map(|kv| kv.1)
        .context("database migration history is empty")?;
    let last_db_mname = &last_db_migration.name;
    let hint = migration_error_hint(ctx, last_db_migration);
    let Some(path) = find_path(migrations, &fixups, last_db_mname, target)? else {
        match last_db_migration.generated_by {
            Some(MigrationGeneratedBy::DevMode) => {
                return Err(anyhow::anyhow!(
                    "database contains Dev mode / `{BRANDING_CLI_CMD} watch` migrations."
                )
                .with_hint(hint))?;
            }
            Some(MigrationGeneratedBy::DDLStatement) | None => {
                let last_fs_rev = migrations
                    .last()
                    .map(|kv| kv.0)
                    .context("filesystem migration history is empty")?;

                let migrations_dir = ctx.schema_dir.join("migrations");

                // Check if database history is strictly ahead of filesystem's.
                if db_migrations.contains_key(last_fs_rev) {
                    let diff = slice(db_migrations, Some(last_fs_rev), None)?;

                    return Err(anyhow::anyhow!(
                        "database applied migration history is ahead of \
                        migration history in {:?} by {} revision{}",
                        migrations_dir,
                        diff.len(),
                        if diff.len() != 1 { "s" } else { "" },
                    )
                    .with_hint(hint))?;
                }

                let mut last_common_rev: Option<&str> = None;

                // Check if there is common history.
                for (fs_rev, db_rev) in migrations.keys().zip(db_migrations.keys()) {
                    if fs_rev != db_rev {
                        break;
                    }
                    last_common_rev = Some(fs_rev);
                }

                if let Some(last_common_rev) = last_common_rev {
                    return Err(anyhow::anyhow!(
                        "database applied migration history diverges from \
                        migration history in {:?} at revision {:?}",
                        migrations_dir,
                        last_common_rev,
                    )
                    .with_hint(hint))?;
                }

                // No revisions in common
                return Err(anyhow::anyhow!(
                    "database applied migration history is completely \
                    unrelated to migration history in {:?}",
                    migrations_dir,
                )
                .with_hint(hint))?;
            }
        }
    };

    let mut operations = Vec::with_capacity(path.len() * 2);
    for path_elem in path {
        match path_elem {
            PathElem::Normal(f) => {
                operations.push(Operation::Apply(f));
            }
            PathElem::Fixup(f) => {
                operations.push(Operation::Apply(f));
                let target = f
                    .fixup_target
                    .as_ref()
                    .ok_or_else(|| bug::error("not a fixup rev"))?;
                let last = migrations.get_index_of(target).ok_or_else(|| {
                    anyhow::anyhow!(
                        "\
                        target of fixup revision {target:?} \
                        is not in a target history. Implementation of \
                        subsequent fixups is limited.\
                    "
                    )
                })?;
                let slice = migrations
                    .get_range(..last + 1)
                    .ok_or_else(|| bug::error("range slicing error"))?;
                operations.push(Operation::Rewrite(slice));
            }
        }
    }

    apply_migrations(cli, &operations, ctx, _options.single_transaction).await?;
    Ok(())
}

fn migration_error_hint<'a: 'b, 'b: 'a>(
    ctx: &'a Context,
    last_db_migration: &'b DBMigration,
) -> impl Fn() -> String + 'a + 'b {
    move || {
        let migrations_dir = ctx.schema_dir.join("migrations");
        match last_db_migration.generated_by {
            Some(MigrationGeneratedBy::DevMode) => {
                format!(
                    "Use `{BRANDING_CLI_CMD} migration create` followed by \
                    `{BRANDING_CLI_CMD} migrate --dev-mode`, or resume `{BRANDING_CLI_CMD} watch`"
                )
            }
            Some(MigrationGeneratedBy::DDLStatement) => {
                format!(
                    "Last recorded database migration is the result of \
                    a direct DDL statement. \
                    Consider running `{BRANDING_CLI_CMD} migration extract` \
                    to bring the history in {migrations_dir:?} in sync with the database."
                )
            }
            _ => {
                format!(
                    "You might have an incorrect or outdated source checkout. \
                    If you don't, consider running `{BRANDING_CLI_CMD} migration extract` \
                    to bring the history in {migrations_dir:?} in sync with the database."
                )
            }
        }
    }
}

fn find_path<'a>(
    migrations: &'a IndexMap<String, MigrationFile>,
    fixups: &'a [MigrationFile],
    db_migration: &String,
    target: &String,
) -> anyhow::Result<Option<Vec<PathElem<'a>>>> {
    let mut by_target = HashMap::new();
    let mut by_source = HashMap::new();
    for mig in fixups {
        if let Some(fixup_target) = &mig.fixup_target {
            if fixup_target != &mig.data.id {
                // do not push twice
                by_target
                    .entry(fixup_target)
                    .or_insert_with(Vec::new)
                    .push(&mig.data.parent_id);
            }
        }
        by_target
            .entry(&mig.data.id)
            .or_insert_with(Vec::new)
            .push(&mig.data.parent_id);
        by_source
            .entry(&mig.data.parent_id)
            .or_insert_with(Vec::new)
            .push(mig);
    }
    for mig in migrations.values() {
        by_target
            .entry(&mig.data.id)
            .or_insert_with(Vec::new)
            .push(&mig.data.parent_id);
        by_source
            .entry(&mig.data.parent_id)
            .or_insert_with(Vec::new)
            .push(mig);
    }

    // Use breadth first search
    let mut queue = VecDeque::new();
    let mut markup = HashMap::new();
    queue.push_back((target, 0));
    markup.insert(target, 0);
    while let Some((migration, num)) = queue.pop_front() {
        if migration == db_migration {
            return backtrack(&markup, &by_source, num, migration).map(Some);
        }
        if let Some(items) = by_target.get(migration) {
            for item in items {
                markup.insert(item, num + 1);
                queue.push_back((item, num + 1));
            }
        }
    }
    Ok(None)
}

fn backtrack<'a>(
    markup: &HashMap<&String, u32>,
    by_source: &HashMap<&String, Vec<&'a MigrationFile>>,
    num: u32,
    db_revision: &String,
) -> anyhow::Result<Vec<PathElem<'a>>> {
    let mut result = Vec::with_capacity(num as usize);
    let mut cur_target = db_revision;
    for idx in (0..num).rev() {
        let sources = by_source
            .get(cur_target)
            .ok_or_else(|| bug::error("failed to backtrack BFS"))?;
        for item in sources {
            if &item.data.parent_id != cur_target {
                continue;
            }
            match item.fixup_target {
                // Normal, non-fixup path takes priority, so that
                // we can skip rebase if revision equals to fixup revision
                _ if markup.get(&item.data.id) == Some(&idx) => {
                    result.push(PathElem::Normal(item));
                    cur_target = &item.data.id;
                    break;
                }
                Some(ref fixup) if markup.get(fixup) == Some(&idx) => {
                    result.push(PathElem::Fixup(item));
                    cur_target = fixup;
                    break;
                }
                _ => {}
            }
        }
    }
    if result.is_empty() || result.len() != num as usize {
        return Err(bug::error("failed to backtrack BFS"));
    }
    Ok(result)
}

pub async fn apply_migrations(
    cli: &mut Connection,
    migrations: &(impl AsOperations + ?Sized),
    ctx: &Context,
    single_transaction: bool,
) -> anyhow::Result<()> {
    if let Some(project) = &ctx.project {
        hooks::on_action("migration.apply.before", project).await?;
    }

    let old_timeout = timeout::inhibit_for_transaction(cli).await?;
    {
        async_try! {
            async {
                if single_transaction {
                    execute(cli, "START TRANSACTION", None).await?;
                    async_try! {
                        async {
                            apply_migrations_inner(cli, migrations, !ctx.quiet).await
                        },
                        except async {
                            execute_if_connected(cli, "ROLLBACK").await
                        },
                        else async {
                            execute(cli, "COMMIT", None).await
                        }
                    }
                } else {
                    apply_migrations_inner(cli, migrations, !ctx.quiet).await
                }
            },
            finally async {
                timeout::restore_for_transaction(cli, old_timeout).await
            }
        }
    }?;
    if let Some(project) = &ctx.project {
        hooks::on_action("migration.apply.after", project).await?;
    }
    Ok(())
}

pub async fn apply_migration(
    cli: &mut Connection,
    migration: &MigrationFile,
    verbose: bool,
) -> anyhow::Result<()> {
    if verbose {
        let file_name = migration.path.file_name().unwrap();
        if print::use_color() {
            eprintln!(
                "Applying {} ({})",
                migration.data.id[..].bold().white(),
                Path::new(file_name).display(),
            );
        } else {
            eprintln!(
                "Applying {} ({})",
                migration.data.id,
                Path::new(file_name).display(),
            );
        }
    }

    let data = fs::read_to_string(&migration.path)
        .await
        .context("error re-reading migration file")?;

    let res = execute_with_parse_callback(cli, &data, || {
        if verbose {
            eprintln!("... parsed");
        }
    })
    .await;

    res.map_err(|err| {
        let fname = migration.path.display().to_string();
        match print_query_error(&err, &data, false, &fname) {
            Ok(()) => ApplyMigrationError.into(),
            Err(err) => err,
        }
    })?;

    if verbose {
        if print::use_color() {
            eprintln!("... {}", "applied".bold().green());
        } else {
            eprintln!("... applied");
        }
    }
    Ok(())
}

async fn execute_with_parse_callback(
    cli: &mut Connection,
    query: &str,
    after_parse: impl FnOnce(),
) -> Result<(), gel_errors::Error> {
    let opts = CompilationOptions {
        implicit_limit: None,
        implicit_typenames: false,
        implicit_typeids: false,
        explicit_objectids: true,
        allow_capabilities: Capabilities::ALL,
        input_language: InputLanguage::EdgeQL,
        io_format: IoFormat::Binary,
        expected_cardinality: Cardinality::Many,
    };

    let command = cli.parse(&opts, query).await?;
    after_parse();
    let stream: ResponseStream<bool> = cli.execute_stream(&opts, query, &command, &()).await?;
    stream.complete().await?;
    Ok(())
}

pub async fn apply_migrations_inner(
    cli: &mut Connection,
    migrations: &(impl AsOperations + ?Sized),
    verbose: bool,
) -> anyhow::Result<()> {
    for operation in migrations.as_operations() {
        match operation {
            Operation::Apply(migration) => {
                apply_migration(cli, migration, verbose).await?;
            }
            Operation::Rewrite(migrations) => {
                execute(cli, "START MIGRATION REWRITE", None).await?;
                async_try! {
                    async {
                        for migration in migrations.values() {
                            apply_migration(cli, migration, false).await?;
                        }
                        anyhow::Ok(())
                    },
                    except async {
                        execute_if_connected(cli, "ABORT MIGRATION REWRITE")
                            .await
                    },
                    else async {
                        execute(cli, "COMMIT MIGRATION REWRITE", None).await
                            .context("commit migration rewrite")
                    }
                }?;
            }
        }
    }
    Ok(())
}

pub async fn disable_ddl(cli: &mut Connection) -> Result<(), anyhow::Error> {
    let ddl_setting = cli
        .query_required_single(
            r#"
        SELECT exists(
            SELECT prop := (
                    SELECT schema::ObjectType
                    FILTER .name = 'cfg::DatabaseConfig'
                ).properties.name
            FILTER prop = "allow_bare_ddl"
        )
    "#,
            &(),
        )
        .await?;
    if ddl_setting {
        cli.execute(
            r#"
            CONFIGURE CURRENT DATABASE SET allow_bare_ddl :=
                cfg::AllowBareDDL.NeverAllow;
        "#,
            &(),
        )
        .await?;
    }
    Ok(())
}

#[cfg(test)]
mod test {
    use super::PathElem;
    use crate::migrations::migration::{Migration, MigrationFile};
    use indexmap::{indexmap, IndexMap};
    use PathMock::*;

    #[derive(Debug)]
    pub enum PathMock {
        Fixup(&'static str),
        Normal(&'static str),
    }

    impl PartialEq<PathMock> for PathElem<'_> {
        fn eq(&self, other: &PathMock) -> bool {
            use PathElem as E;

            match (other, self) {
                (Fixup(a), E::Fixup(b)) if Some(*a) == b.fixup_target.as_deref() => true,
                (Normal(a), E::Normal(b)) if a == &b.data.id => true,
                _ => false,
            }
        }
    }
    impl PartialEq<PathElem<'_>> for PathMock {
        fn eq(&self, other: &PathElem<'_>) -> bool {
            use PathElem as E;

            match (self, other) {
                (Fixup(a), E::Fixup(b)) if Some(*a) == b.fixup_target.as_deref() => true,
                (Normal(a), E::Normal(b)) if a == &b.data.id => true,
                _ => false,
            }
        }
    }

    fn find_path<'a>(
        migrations: &'a IndexMap<String, MigrationFile>,
        fixups: &'a [MigrationFile],
        db: &str,
        target: &str,
    ) -> Vec<PathElem<'a>> {
        super::find_path(migrations, fixups, &db.into(), &target.into())
            .expect("no error")
            .expect("path found")
    }

    fn normal(name: &str, parent: &str) -> MigrationFile {
        MigrationFile {
            path: format!("/__non_existent__/{name}.edgeql").into(),
            fixup_target: None,
            data: Migration {
                id: name.into(),
                parent_id: parent.into(),
                id_range: (0, 0),
                parent_id_range: (0, 0),
                text_range: (0, 0),
                message: None,
            },
        }
    }

    fn fixup(target: &str, parent: &str) -> MigrationFile {
        MigrationFile {
            path: format!("/__non_existent__/{parent}-{target}.edgeql").into(),
            fixup_target: Some(target.into()),
            data: Migration {
                id: format!("{target}{parent}"),
                parent_id: parent.into(),
                id_range: (0, 0),
                parent_id_range: (0, 0),
                text_range: (0, 0),
                message: None,
            },
        }
    }

    #[test]
    fn typical_after_squash() {
        let migrations = indexmap! {
            "m121".into() => normal("m121", "initial"),
        };
        let fixups = vec![fixup("m121", "m107")];
        assert_eq!(
            find_path(&migrations, &fixups, "m107", "m121"),
            vec![Fixup("m121")],
        );
    }

    #[test]
    fn extra_revs() {
        let migrations = indexmap! {
            "m121".into() => normal("m121", "initial"),
            "m122".into() => normal("m122", "m121"),
            "m123".into() => normal("m123", "m122"),
            "m124".into() => normal("m124", "m123"),
        };
        let fixups = vec![fixup("m121", "m105")];
        assert_eq!(
            find_path(&migrations, &fixups, "m105", "m124"),
            vec![
                Fixup("m121"),
                Normal("m122"),
                Normal("m123"),
                Normal("m124")
            ],
        );
        assert_eq!(
            find_path(&migrations, &fixups, "m105", "m123"),
            vec![Fixup("m121"), Normal("m122"), Normal("m123")],
        );
    }

    #[test]
    fn two_fixups() {
        // Currently it only works if fixup target == migration id.
        // This is because we have no way to rebase history after first fixup.
        // We have to send second fixup without revision id (i.e. rewrite
        // migration a bit) to make this fully work.
        //
        // But we have a test for path search anyways as there are limited cases
        // where it actually works
        let migrations = indexmap! {
            "m121".into() => normal("m121", "initial"),
            "m122".into() => normal("m122", "m121"),
            "m123".into() => normal("m123", "m122"),
            "m124".into() => normal("m124", "m123"),
        };
        let fixups = vec![fixup("m105", "m103"), fixup("m121", "m105")];
        assert_eq!(
            find_path(&migrations, &fixups, "m103", "m123"),
            vec![Fixup("m105"), Fixup("m121"), Normal("m122"), Normal("m123")],
        );
    }

    #[test]
    fn shortcut() {
        // I'm not sure it's useful, but the test here is to ensure that this
        // behavior is not broken
        let migrations = indexmap! {
            "m101".into() => normal("m101", "initial"),
            "m102".into() => normal("m102", "m101"),
            "m103".into() => normal("m103", "m102"),
            "m104".into() => normal("m104", "m103"),
            "m105".into() => normal("m105", "m104"),
            "m106".into() => normal("m106", "m105"),
        };
        let fixups = vec![fixup("m105", "m102")];
        assert_eq!(
            find_path(&migrations, &fixups, "m101", "m106"),
            vec![Normal("m102"), Fixup("m105"), Normal("m106")],
        );
    }
}
