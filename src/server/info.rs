use prettytable::{Table, Row, Cell};

use crate::server::detect::{self, VersionQuery};
use crate::server::distribution::MajorVersion;
use crate::server::init::find_distribution;
use crate::server::linux;
use crate::server::macos;
use crate::server::options::Info;
use crate::server::package::Package;
use crate::server::version::Version;
use crate::table;


#[derive(serde::Serialize)]
struct JsonInfo<'a> {
    installation_method: &'a str,
    major_version: &'a MajorVersion,
    version: &'a Version<String>,
    binary_path: Option<&'a str>,
}


pub fn info(options: &Info) -> anyhow::Result<()> {
    let version_query = VersionQuery::new(
        options.nightly, options.version.as_ref());
    let current_os = detect::current_os()?;
    let avail_methods = current_os.get_available_methods()?;
    let (distr, method, _) = find_distribution(
        &*current_os, &avail_methods,
        &version_query, &options.method)?;
    let cmd = distr.downcast_ref::<Package>().map(|pkg| {
        if cfg!(target_os="macos") {
            macos::get_server_path(&pkg.slot)
        } else {
            linux::get_server_path(Some(&pkg.slot))
        }
    });
    if options.bin_path {
        if let Some(cmd) = cmd {
            if options.json {
                if let Some(cmd) = cmd.to_str() {
                    println!("{}", serde_json::to_string(cmd)?);
                } else {
                    anyhow::bail!("Path {:?} can't be represented as JSON",
                        cmd);
                }
            } else {
                println!("{}", cmd.display());
            }
        } else {
            anyhow::bail!("cannot print binary path for {} installation",
                method.option());
        }
    } else if options.json {
        println!("{}", serde_json::to_string_pretty(&JsonInfo {
            installation_method: method.short_name(),
            major_version: distr.major_version(),
            version: distr.version(),
            binary_path: cmd.as_ref().and_then(|cmd| cmd.to_str()),
        })?)
    } else {
        let mut table = Table::new();
        table.add_row(Row::new(vec![
            Cell::new("Installation method"),
            Cell::new(method.title()),
        ]));
        table.add_row(Row::new(vec![
            Cell::new("Major version"),
            Cell::new(distr.major_version().title()),
        ]));
        table.add_row(Row::new(vec![
            Cell::new("Exact version"),
            Cell::new(distr.version().as_ref()),
        ]));
        if let Some(cmd) = cmd {
            table.add_row(Row::new(vec![
                Cell::new("Binary path"),
                Cell::new(&cmd.display().to_string()),
            ]));
        }
        table.set_format(*table::FORMAT);
        table.printstd();
    }
    Ok(())
}
