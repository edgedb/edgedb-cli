use prettytable::{Table, Row, Cell};

use crate::server::distribution::DistributionRef;
use crate::server::methods::InstallMethod;
use crate::table;


#[derive(Debug)]
pub struct Settings {
    pub method: InstallMethod,
    pub distribution: DistributionRef,
}

impl Settings {
    pub fn print(&self) {
        let mut table = Table::new();
        table.add_row(Row::new(vec![
            Cell::new("Installation method"),
            Cell::new(self.method.title()),
            Cell::new(self.method.option()),
        ]));
        table.add_row(Row::new(vec![
            Cell::new("Major version"),
            Cell::new(&self.distribution.version_slot().title().to_string()),
            Cell::new(&self.distribution.version_slot()
                      .to_query().install_option()),
        ]));
        table.add_row(Row::new(vec![
            Cell::new("Exact version"),
            Cell::new(self.distribution.version().as_ref()),
        ]));
        table.set_format(*table::FORMAT);
        table.printstd();
    }
}



