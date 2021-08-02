use std::path::{PathBuf};

use prettytable::{Table, Row, Cell};

use crate::options::Options;
use crate::platform;
use crate::table;

fn dir_to_str(path: &anyhow::Result<PathBuf>) -> String {
    let path_str = match path {
        Ok(dir) => match dir.clone().into_os_string().into_string() {
            Ok(s) =>
                Ok(s.to_string()),
            Err(_) =>
                Err("Error: failed to convert OsString to String".to_string())
        },
        Err(e) => Err(format!("Error: {} ", e.to_string()))
    };

    match path_str {
        Ok(dir) => dir,
        Err(e) => e.to_string()
    }
}

pub async fn info(_options: &Options)-> Result<(), anyhow::Error>
{
    let mut table = Table::new();

    table.add_row(Row::new(vec![
        Cell::new("Cache"),
        Cell::new(dir_to_str(&platform::cache_dir()).as_ref()),
    ]));
    table.add_row(Row::new(vec![
        Cell::new("Config"),
        Cell::new(dir_to_str(&platform::config_dir()).as_ref()),
    ]));

    table.set_format(*table::FORMAT);

    println!("EdgeDB uses the following local directories:");
    table.printstd();

    Ok(())
}
