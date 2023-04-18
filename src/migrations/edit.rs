use std::path::Path;

use dissimilar::{diff, Chunk};
use tokio::fs;
use tokio::task::spawn_blocking as unblock;

use crate::commands::Options;
use crate::connect::Connection;
use crate::error_display::print_query_error;
use crate::migrations::context::Context;
use crate::migrations::grammar::parse_migration;
use crate::migrations::migration::{read_names, file_num};
use crate::migrations::options::MigrationEdit;
use crate::platform::{tmp_file_path, spawn_editor};
use crate::print::{echo, err_marker, Highlight};
use crate::question::Choice;


#[derive(Copy, Clone)]
enum OldAction {
    Restore,
    Replace,
    Diff
}

#[derive(Copy, Clone)]
enum InvalidAction {
    Edit,
    Diff,
    Abort,
    Restore,
}

#[derive(Copy, Clone)]
enum FailAction {
    Edit,
    Force,
    Diff,
    Abort,
    Restore,
}

fn print_diff(path1: &Path, data1: &str, path2: &Path, data2: &str) {
    println!("--- {}", path1.display());
    println!("+++ {}", path2.display());
    let changeset = diff(data1, data2);
    let n1 = data1.split("\n").count();
    let n2 = data2.split("\n").count();
    println!("@@ -1,{} +1,{}", n1, n2);
    for item in &changeset {
        match item {
            Chunk::Equal(block) => {
                for line in block.split("\n") {
                    println!(" {}", line);
                }
            }
            Chunk::Insert(block) => {
                for line in block.split("\n") {
                    println!("+{}", line.added());
                }
            }
            Chunk::Delete(block) => {
                for line in block.split("\n") {
                    println!("-{}", line.deleted());
                }
            }
        }
    }
}

#[tokio::main]
pub async fn edit_no_check(_common: &Options, options: &MigrationEdit)
    -> Result<(), anyhow::Error>
{
    let ctx = Context::from_project_or_config(&options.cfg, false).await?;
    // TODO(tailhook) do we have to make the full check of whether there are no
    // gaps and parent revisions are okay?
    let (_n, path) = read_names(&ctx).await?
        .into_iter()
        .filter_map(|p| file_num(&p).map(|n| (n, p)))
        .max_by(|(an, _), (bn, _)| an.cmp(bn))
        .ok_or_else(|| anyhow::anyhow!("no migration exists. \
                                       Run `edgedb migration create`"))?;

    if !options.non_interactive {
        spawn_editor(path.as_ref()).await?;
    }

    let text = fs::read_to_string(&path).await?;
    let migration = parse_migration(&text)?;
    let new_id = migration.expected_id(&text)?;

    if migration.id != new_id {
        let tmp_file = tmp_file_path(path.as_ref());
        if fs::metadata(&tmp_file).await.is_ok() {
            fs::remove_file(&tmp_file).await?;
        }
        fs::write(&tmp_file, migration.replace_id(&text, &new_id)).await?;
        fs::rename(&tmp_file, &path).await?;
        echo!("Updated migration id to", new_id.emphasize());
    } else {
        echo!("Id", migration.id.emphasize(), "is already correct.");
    }
    Ok(())
}

async fn check_migration(cli: &mut Connection, text: &str)
    -> anyhow::Result<()>
{
    cli.execute("START TRANSACTION", &()).await?;
    let res = cli.execute(&text, &()).await.map_err(|err| {
        match print_query_error(&err, &text, false) {
            Ok(()) => err.into(),
            Err(err) => err,
        }
    });
    cli.execute("ROLLBACK", &()).await
        .map_err(|e| log::warn!("Error rolling back the transaction: {:#}", e))
        .ok();
    return res.map(|_| ());
}

pub async fn edit(cli: &mut Connection,
                  common: &Options, options: &MigrationEdit)
    -> anyhow::Result<()>
{
    let old_state = cli.set_ignore_error_state();
    let res = _edit(cli, common, options).await;
    cli.restore_state(old_state);
    return res;
}

async fn _edit(cli: &mut Connection,
                  _common: &Options, options: &MigrationEdit)
    -> anyhow::Result<()>
{
    let ctx = Context::from_project_or_config(&options.cfg, false).await?;
    // TODO(tailhook) do we have to make the full check of whether there are no
    // gaps and parent revisions are okay?
    let (n, path) = cli.ping_while(read_names(&ctx)).await?
        .into_iter()
        .filter_map(|p| file_num(&p).map(|n| (n, p)))
        .max_by(|(an, _), (bn, _)| an.cmp(bn))
        .ok_or_else(|| anyhow::anyhow!("no migration exists. \
                                       Run `edgedb migration create`"))?;

    if options.non_interactive {
        let text = cli.ping_while(fs::read_to_string(&path)).await?;
        let migration = parse_migration(&text)?;
        let new_id = migration.expected_id(&text)?;
        let new_data = migration.replace_id(&text, &new_id);
        check_migration(cli, &new_data).await?;

        if migration.id != new_id {
            cli.ping_while(async {
                let tmp_file = tmp_file_path(path.as_ref());
                if fs::metadata(&tmp_file).await.is_ok() {
                    fs::remove_file(&tmp_file).await?;
                }
                fs::write(&tmp_file, &new_data).await?;
                fs::rename(&tmp_file, &path).await?;
                anyhow::Ok(())
            }).await?;
            echo!("Updated migration id to", new_id.emphasize());
        } else {
            echo!("Id", migration.id.emphasize(), "is already correct.");
        }
    } else {
        let temp_path = path.parent().unwrap()
            .join(format!(".editing.{}.edgeql", n));
        if cli.ping_while(fs::metadata(&temp_path)).await.is_ok() {
            loop {
                let mut q = Choice::new(
                    "Previously edited file exists. Restore?");
                q.option(OldAction::Restore, &["y", "yes"],
                         format!("use previously edited {:?}", temp_path));
                q.option(OldAction::Replace, &["n", "no"],
                         format!("use original {:?} instead", path));
                q.option(OldAction::Diff, &["d", "diff"], "show diff");
                match cli.ping_while(q.async_ask()).await? {
                    OldAction::Restore => break,
                    OldAction::Replace => {
                        cli.ping_while(fs::copy(&path, &temp_path)).await?;
                        break;
                    }
                    OldAction::Diff => {
                        cli.ping_while(async {
                            let path = path.clone();
                            let temp_path = temp_path.clone();
                            let normal = fs::read_to_string(&path).await?;
                            let modif = fs::read_to_string(&temp_path).await?;
                            unblock(move || {
                                print_diff(&path, &normal, &temp_path, &modif);
                            }).await?;
                            anyhow::Ok(())
                        }).await?;
                    }
                }
            }
        } else {
            cli.ping_while(fs::copy(&path, &temp_path)).await?;
        }
        'edit: loop {
            cli.ping_while(spawn_editor(temp_path.as_ref())).await?;
            let mut new_data =
                cli.ping_while(fs::read_to_string(&temp_path)).await?;
            let migration = match parse_migration(&new_data) {
                Ok(migr) => migr,
                Err(e) => {
                    echo!(err_marker(), "error parsing file:", e);
                    loop {
                        let mut q = Choice::new("Edit again?");
                        q.option(InvalidAction::Edit, &["y", "yes"][..],
                                 "edit the file again");
                        q.option(InvalidAction::Diff, &["d", "diff"][..],
                                 "show diff");
                        q.option(InvalidAction::Restore, &["r", "restore"][..],
                                 "restore original and abort");
                        q.option(InvalidAction::Abort, &["q", "quit"][..],
                                 "abort and keep temporary file");
                        match cli.ping_while(q.async_ask()).await? {
                            InvalidAction::Edit => continue 'edit,
                            InvalidAction::Diff => {
                                cli.ping_while(async {
                                    let path = path.clone();
                                    let temp_path = temp_path.clone();
                                    let new_data = new_data.clone();
                                    let data = fs::read_to_string(&path).await?;
                                    unblock(move || {
                                        print_diff(&path, &data,
                                                   &temp_path, &new_data);
                                    }).await?;
                                    anyhow::Ok(())
                                }).await?;
                            }
                            InvalidAction::Restore => {
                                fs::copy(&path, &temp_path).await?;
                                anyhow::bail!("Restored");
                            }
                            InvalidAction::Abort => {
                                anyhow::bail!("Aborted!");
                            }
                        }
                    }
                }
            };
            let new_id = migration.expected_id(&new_data)?;
            if migration.id != new_id {
                new_data = migration.replace_id(&new_data, &new_id);
                fs::write(&temp_path, &new_data).await?;
                echo!("Updated migration id to", new_id.emphasize());
            } else {
                echo!("Id", migration.id.emphasize(), "is already correct.");
            }
            match check_migration(cli, &new_data).await {
                Ok(()) => {}
                Err(e) => {
                    echo!(err_marker(), "error checking migration:", e);
                    loop {
                        let mut q = Choice::new("Edit again?");
                        q.option(FailAction::Edit, &["y", "yes"][..],
                                 "edit the file again");
                        q.option(FailAction::Force, &["f", "force"][..],
                                 "force overwrite and quit");
                        q.option(FailAction::Diff, &["d", "diff"][..],
                                 "show diff");
                        q.option(FailAction::Restore, &["r", "restore"][..],
                                 "restore original and abort");
                        q.option(FailAction::Abort, &["q", "quit"][..],
                                 "abort and keep temporary file for later");
                        match q.async_ask().await? {
                            FailAction::Edit => continue 'edit,
                            FailAction::Force => {
                                fs::rename(&temp_path, &path).await?;
                                anyhow::bail!("Done. Replaced {:?} with \
                                               possibly invalid migration.",
                                               std::path::Path::new(&path));
                            }
                            FailAction::Diff => {
                                let data = fs::read_to_string(&path).await?;
                                print_diff(&path, &data,
                                           &temp_path, &new_data);
                            }
                            FailAction::Restore => {
                                fs::copy(&path, &temp_path).await?;
                                anyhow::bail!("Restored");
                            }
                            FailAction::Abort => {
                                anyhow::bail!("Aborted!");
                            }
                        }
                    }
                }
            }
            fs::rename(&temp_path, &path).await?;
            break;
        }
    }
    Ok(())
}

#[test]
fn default() {
    let original = "
        CREATE MIGRATION m1wrvvw3lycyovtlx4szqm75554g75h5nnbjq3a5qsdncn3oef6nia
        ONTO m1e5vq3h4oizlsp4a3zge5bqhu7yeoorc27k3yo2aaenfqgfars6uq
        {
            CREATE TYPE X;
        };
    ";
    let migration = parse_migration(&original).unwrap();
    let new_id = migration.expected_id(&original).unwrap();
    assert_eq!(migration.replace_id(&original, &new_id), "
        CREATE MIGRATION m1uaw5ik4wg4w33jj35sjgdgg3pai23ysqy5pi7xmxqnd3gtneb57q
        ONTO m1e5vq3h4oizlsp4a3zge5bqhu7yeoorc27k3yo2aaenfqgfars6uq
        {
            CREATE TYPE X;
        };
    ");
}

#[test]
fn space() {
    let original = "
        CREATE MIGRATION xx \
            ONTO m1e5vq3h4oizlsp4a3zge5bqhu7yeoorc27k3yo2aaenfqgfars6uq
        {
            CREATE TYPE X;
        };
    ";
    let migration = parse_migration(&original).unwrap();
    let new_id = migration.expected_id(&original).unwrap();
    assert_eq!(migration.replace_id(&original, &new_id), "
        CREATE MIGRATION \
            m1uaw5ik4wg4w33jj35sjgdgg3pai23ysqy5pi7xmxqnd3gtneb57q \
            ONTO m1e5vq3h4oizlsp4a3zge5bqhu7yeoorc27k3yo2aaenfqgfars6uq
        {
            CREATE TYPE X;
        };
    ");
}
