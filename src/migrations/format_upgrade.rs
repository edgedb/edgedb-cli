use std::fs;
use std::path::PathBuf;
use regex::Regex;
use crate::commands::Options;
use crate::connect::Connection;
use crate::migrations::Context;
use crate::migrations::migration::{file_num, read_file, read_names};
use crate::migrations::options::{MigrationFormatUpgrade};

pub async fn format_upgrade(
    _cli: &mut Connection,
    _opts: &Options,
    params: &MigrationFormatUpgrade,
) -> anyhow::Result<()> {
    let ctx = Context::from_project_or_config(&params.cfg, false).await?;

    let names = read_names(&ctx).await?;

    let old_filename = Regex::new(r"^\d{5}$")?;
    let new_filename = Regex::new(r"^\d{5}-[a-z0-9]{7}$").unwrap();

    for name in names {
        let fname = name.file_stem().unwrap().to_str().unwrap();

        if let Some(_) = old_filename.captures(fname) {
            // migrate to new filename
            println!("Formatting {}.edgeql...", fname);
            format_file(&name, file_num(&name).unwrap()).await?;
        } else if let Some(_) = new_filename.captures(fname) {
            println!("Migration {} OK", fname)
        } else {
            anyhow::bail!("Unknown file \"{}\"", fname)
        }
    }

    println!("All files formatted");

    Ok(())
}

async fn format_file(file: &PathBuf, num: u64) -> anyhow::Result<()> {
    let migration = read_file(file, true).await?;
    let new_name = file.parent().unwrap().join(format!("{:05}-{}.edgeql", num, &migration.id[..7]));

    fs::rename(file, new_name)?;

    Ok(())
}