use prettytable::{Table, Row, Cell};

use crate::server::detect::{self, VersionQuery};
use crate::server::options::Info;
use crate::server::package::Package;
use crate::server::linux;
use crate::server::macos;
use crate::server::init::find_distribution;
use crate::table;


pub fn info(options: &Info) -> anyhow::Result<()> {
    let version_query = VersionQuery::new(
        options.nightly, options.version.as_ref());
    let current_os = detect::current_os()?;
    let avail_methods = current_os.get_available_methods()?;
    let (distr, method, _) = find_distribution(
        &*current_os, &avail_methods,
        &version_query, &options.method)?;
    if options.bin_path {
        if let Some(pkg) = distr.downcast_ref::<Package>() {
            let cmd = if cfg!(target_os="macos") {
                macos::get_server_path(&pkg.slot)
            } else {
                linux::get_server_path(Some(&pkg.slot))
            };
            println!("{}", cmd.display());
        } else {
            anyhow::bail!("cannot print binary path for {} installation",
                method.option());
        }
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
        if let Some(pkg) = distr.downcast_ref::<Package>() {
            let cmd = if cfg!(target_os="macos") {
                macos::get_server_path(&pkg.slot)
            } else {
                linux::get_server_path(Some(&pkg.slot))
            };
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
