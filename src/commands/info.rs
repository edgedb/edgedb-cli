use std::env;
use std::path::{PathBuf, MAIN_SEPARATOR};

use prettytable::{Table, Row, Cell};

use crate::options::{Options, Info};
use crate::platform;
use crate::table;


fn dir_to_str(path: PathBuf) -> String {
    let mut rv = path.display().to_string();
    rv.push(MAIN_SEPARATOR);
    rv
}

pub fn specific_info(item: &str) -> Result<(), anyhow::Error> {
    match item {
        "config-dir" => {
            println!("{}", dir_to_str(platform::config_dir()?));
        }
        "cache-dir" => {
            println!("{}", dir_to_str(platform::cache_dir()?));
        }
        "data-dir" => {
            if cfg!(windows) {
                anyhow::bail!("Windows instances don't have the data dir,
                               as they are created in the WSL");
            } else {
                println!("{}", dir_to_str(platform::data_dir()?));
            }
        }
        "service-dir" => {
            if cfg!(target_os="linux") {
                use crate::portable::linux::unit_dir;
                println!("{}", &dir_to_str(unit_dir()?));
            } else if cfg!(target_os="macos") {
                use crate::portable::macos::plist_dir;
                println!("{}", &dir_to_str(plist_dir()?));
            } else if cfg!(windows) {
                use crate::portable::windows::startup_dir;
                println!("{}", &dir_to_str(startup_dir()?));
            }
        }
        _ => unreachable!(),
    }
    Ok(())
}

#[tokio::main]
pub async fn info(_options: &Options, info: &Info)-> Result<(), anyhow::Error>
{
    if let Some(ref item) = info.get {
        return specific_info(&item);
    }
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
    if cfg!(not(windows)) {
        table.add_row(Row::new(vec![
            Cell::new("Data"),
            Cell::new(&dir_to_str(data_dir)),
        ]));
    }
    if cfg!(target_os="linux") {
        use crate::portable::linux::unit_dir;

        table.add_row(Row::new(vec![
            Cell::new("Service"),
            Cell::new(&dir_to_str(unit_dir()?)),
        ]));
    } else if cfg!(target_os="macos") {
        use crate::portable::macos::plist_dir;

        table.add_row(Row::new(vec![
            Cell::new("Service"),
            Cell::new(&dir_to_str(plist_dir()?)),
        ]));
    }

    table.set_format(*table::FORMAT);

    println!("EdgeDB uses the following local paths:");
    table.printstd();

    Ok(())
}
