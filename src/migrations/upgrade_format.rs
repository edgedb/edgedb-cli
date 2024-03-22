use std::fs;
use std::path::PathBuf;
use regex::Regex;
use crate::commands::Options;
use crate::connect::Connection;
use crate::migrations::Context;
use crate::migrations::migration::{file_num, read_file, read_names};
use crate::migrations::options::MigrationUpgradeFormat;
use crate::print;

pub async fn upgrade_format(
    _cli: &mut Connection,
    _opts: &Options,
    params: &MigrationUpgradeFormat,
) -> anyhow::Result<()> {
    let ctx = Context::from_project_or_config(&params.cfg, false).await?;

    _upgrade_format(&ctx).await
}

async fn _upgrade_format(context: &Context) -> anyhow::Result<()> {
    let names = read_names(context).await?;

    let old_filename = Regex::new(r"^\d{5}$")?;
    let new_filename = Regex::new(r"^\d{5}-[a-z0-9]{7}$")?;

    for name in names {
        let fname = name.file_stem().unwrap().to_str().unwrap();

        if let Some(_) = old_filename.captures(fname) {
            // migrate to new filename
            eprintln!("Upgrading migration file layout for {}.edgeql...", fname);
            upgrade_format_of_file(&name, file_num(&name).unwrap()).await?;
        } else if let Some(_) = new_filename.captures(fname) {
            println!("Migration {} OK", fname)
        } else {
            print::warn(format!("Unknown migration file naming schema: {fname}", ))
        }
    }

    println!("All files formatted");

    Ok(())
}

async fn upgrade_format_of_file(file: &PathBuf, num: u64) -> anyhow::Result<()> {
    let migration = read_file(file, true).await?;
    let new_name = file.parent().unwrap().join(format!("{:05}-{}.edgeql", num, &migration.id[..7]));

    fs::rename(file, new_name)?;

    Ok(())
}

#[cfg(test)]
mod test {
    use std::fs;
    use regex::Regex;
    use crate::migrations::Context;
    use crate::migrations::migration::{read_file, read_names};
    use crate::migrations::upgrade_format::_upgrade_format;

    #[tokio::test]
    async fn test_upgrade() {
        use std::env;

        let mut original_schema_dir = env::current_dir().unwrap();
        original_schema_dir.push("tests/migrations/db3");

        let tmp_dir = tempfile::tempdir().expect("tmpdir");
        fs_extra::dir::copy(original_schema_dir, &tmp_dir, &Default::default()).unwrap();
        let schema_dir = tmp_dir.path().to_path_buf();

        let ctx = Context { schema_dir, edgedb_version: None, quiet: false };

        _upgrade_format(&ctx).await.unwrap();

        let files = read_names(&ctx).await.unwrap();
        let re = Regex::new(r"^(\d{5})-([a-z0-9]{7})$").unwrap();

        for file in files {
            let migration = read_file(&file, true).await.unwrap();
            let name = file.file_stem().unwrap().to_str().unwrap();

            // verify that the filename matches the new format
            let re_match = re.captures(name).unwrap();

            // verify the migration ID in the filename starts with the ID in the migration
            let migration_part = re_match.get(2).unwrap().as_str();
            assert!(migration.id.starts_with(migration_part));

            // rename the file back to the old one
            let target = file
                .parent()
                .unwrap()
                .join(format!("{:05}.edgeql", re_match.get(1).unwrap().as_str()));
            fs::rename(file, target).unwrap();
        }
    }
}
