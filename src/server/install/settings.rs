use linked_hash_map::LinkedHashMap;
use once_cell::sync::Lazy;
use prettytable::format::TableFormat;
use prettytable::format::{FormatBuilder, LinePosition, LineSeparator};
use prettytable::{Table, Row, Cell};

use crate::server::options::Install;
use crate::server::detect::{Detect, InstallMethod, VersionQuery};
use crate::server::version::Version;


pub static FORMAT: Lazy<TableFormat> = Lazy::new(|| {
    FormatBuilder::new()
    .column_separator('│')
    .borders('│')
    .separators(&[LinePosition::Top],
                LineSeparator::new('─',
                                   '┬',
                                   '┌',
                                   '┐'))
    .separators(&[LinePosition::Title],
                LineSeparator::new('─',
                                   '┼',
                                   '├',
                                   '┤'))
    .separators(&[LinePosition::Bottom],
                LineSeparator::new('─',
                                   '┴',
                                   '└',
                                   '┘'))
    .padding(1, 1)
    .build()
});


#[derive(Debug)]
pub struct SettingsBuilder<'a> {
    pub detect: &'a Detect,
    pub nightly: bool,
    pub package_name: String,
    pub major_version: Version<String>,
    pub version: Version<String>,
    pub extra: LinkedHashMap<String, String>,
}

#[derive(Debug)]
pub struct Settings {
    pub method: InstallMethod,
    pub nightly: bool,
    pub package_name: String,
    pub major_version: Version<String>,
    pub version: Version<String>,
    pub extra: LinkedHashMap<String, String>,
}

pub enum BuildError {
    Fatal(anyhow::Error),
    Configurable(Vec<String>),
}

impl SettingsBuilder<'_> {
    pub fn new<'x>(detect: &'x Detect, options: &Install)
        -> Result<SettingsBuilder<'x>, anyhow::Error>
    {
        let version = detect.get_version(
            &if options.nightly {
                VersionQuery::Nightly
            } else {
                VersionQuery::Stable(options.version.clone())
            }
        )?;
        Ok(SettingsBuilder {
            detect,
            package_name: version.package_name,
            major_version: version.major_version,
            version: version.version,
            nightly: options.nightly,
            extra: LinkedHashMap::new(),
        })
    }
    pub fn build(self) -> Result<Settings, BuildError> {
        let mut errors = vec![];
        let method = match self.detect.get_available_methods() {
            [] => {
                return Err(BuildError::Fatal(anyhow::anyhow!(
                    "No installation method \
                    available. Please consider opening an issue ticket \
                    at https://github.com/edgedb/edgedb-cli/issues/new\
                    ?template=install-unsupported.md")));
            }
            [meth] => Some(meth),
            [all@..] => {
                errors.push(
                    format!("Installation methods available: \n{}\n",
                        all.iter()
                        .map(|m| format!("  * `{}` -- {}",
                                         m.option(), m.title()))
                        .collect::<Vec<_>>()
                        .join(","))
                );
                None
            }
        };
        if errors.is_empty() {
            Ok(Settings {
                method: method.unwrap().clone(),
                package_name: self.package_name,
                major_version: self.major_version,
                version: self.version,
                nightly: self.nightly,
                extra: self.extra,
            })
        } else {
            Err(BuildError::Configurable(errors))
        }
    }
}

impl Settings {
    pub fn print(&self) {
        let mut table = Table::new();
        table.add_row(Row::new(vec![
            Cell::new("Installation method"),
            Cell::new(self.method.title()),
        ]));
        table.add_row(Row::new(vec![
            Cell::new("Major version"),
            Cell::new(self.major_version.num()),
        ]));
        table.add_row(Row::new(vec![
            Cell::new("Exact version"),
            Cell::new(self.version.num()),
        ]));
        for (k, v) in &self.extra {
            table.add_row(Row::new(vec![
                Cell::new(k),
                Cell::new(v),
            ]));
        }
        table.set_format(*FORMAT);
        table.printstd();
    }
}


