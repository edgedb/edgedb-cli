use std::fs;
use std::path::Path;

use prettytable::{Table, Row, Cell};

use crate::project::options::Info;
use crate::project::{project_dir, stash_path};
use crate::table;


#[derive(serde::Serialize)]
#[serde(rename_all="kebab-case")]
struct JsonInfo<'a> {
    instance_name: &'a str,
    root: &'a Path,
}

pub fn info(options: &Info) -> anyhow::Result<()> {
    let root = project_dir(options.project_dir.as_ref().map(|x| x.as_path()))?;
    let stash_dir = stash_path(&root)?;
    let instance_name = fs::read_to_string(stash_dir.join("instance-name"))?;

    if options.instance_name {
        if options.json {
            println!("{}", serde_json::to_string(&instance_name)?);
        } else {
            println!("{}", instance_name);
        }
    } else if options.json {
        println!("{}", serde_json::to_string_pretty(&JsonInfo {
            instance_name: &instance_name,
            root: &root,
        })?);
    } else {
        let mut table = Table::new();
        table.add_row(Row::new(vec![
            Cell::new("Instance name"),
            Cell::new(&instance_name),
        ]));
        table.add_row(Row::new(vec![
            Cell::new("Project root"),
            Cell::new(&root.display().to_string()),
        ]));
        table.set_format(*table::FORMAT);
        table.printstd();
    }
    Ok(())
}
