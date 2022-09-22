use edgedb_client::client::Connection;

use async_std::fs;
use async_std::path::Path;

use crate::print::{echo, Highlight};
use crate::commands::Options;
use crate::commands::parser::MigrationEdit;
use crate::migrations::context::Context;
use crate::migrations::migration::{read_names, file_num};
use crate::migrations::grammar::parse_migration;
use crate::platform::{tmp_file_path, spawn_editor};


pub async fn edit_no_check(_common: &Options, options: &MigrationEdit)
    -> Result<(), anyhow::Error>
{
    let ctx = Context::from_project_or_config(&options.cfg)?;
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
        if Path::new(&tmp_file).exists().await {
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

pub async fn edit(_cli: &mut Connection,
                  common: &Options, options: &MigrationEdit)
    -> Result<(), anyhow::Error>
{
    // TODO(tailhook)
    edit_no_check(common, options).await
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
