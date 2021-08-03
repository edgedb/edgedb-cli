use std::env;
use std::path::{PathBuf, MAIN_SEPARATOR};

use prettytable::{Table, Row, Cell};

use crate::options::Options;
use crate::platform;
use crate::table;


fn dir_to_str(path: PathBuf) -> String {
    let mut rv = path.display().to_string();
    rv.push(MAIN_SEPARATOR);
    rv
}

pub async fn info(_options: &Options)-> Result<(), anyhow::Error>
{
    let mut table = Table::new();

    table.add_row(Row::new(vec![
        Cell::new("Cache"),
        Cell::new(&dir_to_str(platform::cache_dir()?)),
    ]));
    table.add_row(Row::new(vec![
        Cell::new("Config"),
        Cell::new(&dir_to_str(platform::config_dir()?)),
    ]));
    if let Ok(current_exe) = env::current_exe() {
        table.add_row(Row::new(vec![
            if current_exe == platform::binary_path()? {
                Cell::new("CLI Binary")
            } else {
                Cell::new("Custom Binary")
            },
            Cell::new(&current_exe.display().to_string()),
        ]));
    }
    let data_dir = platform::data_dir()?;
    if data_dir.exists() {
        table.add_row(Row::new(vec![
            Cell::new("Data"),
            Cell::new(&dir_to_str(data_dir)),
        ]));
    }
    if cfg!(target_os="linux") {
        use crate::server::linux::unit_dir;

        table.add_row(Row::new(vec![
            Cell::new("Service"),
            Cell::new(&dir_to_str(unit_dir(false)?)),
        ]));
    } else if cfg!(target_os="macos") {
        use crate::server::macos::plist_dir;

        table.add_row(Row::new(vec![
            Cell::new("Service"),
            Cell::new(&dir_to_str(plist_dir(false)?)),
        ]));
    }

    table.set_format(*table::FORMAT);

    println!("EdgeDB uses the following local paths:");
    table.printstd();

    Ok(())
}
