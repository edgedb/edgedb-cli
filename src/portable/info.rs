use anyhow::Context;

use crate::portable::local;
use crate::portable::options::Info;
use crate::portable::repository::Query;
use crate::portable::ver;
use crate::table;


#[derive(serde::Serialize)]
#[serde(rename_all="kebab-case")]
struct JsonInfo<'a> {
    version: &'a ver::Build,
    binary_path: Option<&'a str>,
}


pub fn info(options: &Info) -> anyhow::Result<()> {
    if !options.nightly && !options.latest && !options.version.is_some() {
        anyhow::bail!("One of `--latest`, `--nightly`, `--version=` required");
    }
    // note this assumes that latest is set if no nightly and version
    let query = Query::from_options(options.nightly, &options.version)?;
    let all = local::get_installed()?;
    let inst = all.into_iter().filter(|item| query.matches(&item.version))
        .max_by_key(|item| item.version.specific())
        .context("cannot find installed packages maching your criteria")?;

    let item = options.get.as_deref()
        .or(options.bin_path.then(|| "bin-path"));
    if let Some(item) = item {
        match item {
            "bin-path" => {
                let path = inst.server_path()?;
                if options.json {
                    let path = path.to_str()
                        .context("cannot convert path to a string")?;
                    println!("{}", serde_json::to_string(path)?);
                } else {
                    println!("{}", path.display());
                }

            }
            _ => unreachable!(),
        }
    } else if options.json {
        println!("{}", serde_json::to_string_pretty(&JsonInfo {
            version: &inst.version,
            binary_path: inst.server_path()?.to_str(),
        })?)
    } else {
        table::settings(&[
            ("Version", &inst.version.to_string()),
            ("Binary path", &inst.server_path()?.display().to_string()),
        ]);
    }
    Ok(())
}
