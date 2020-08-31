use linked_hash_map::LinkedHashMap;
use prettytable::{Table, Row, Cell};

use crate::server::options::Install;
use crate::server::os_trait::{CurrentOs, Method};
use crate::server::detect::VersionQuery;
use crate::server::methods::InstallMethod;
use crate::server::distribution::DistributionRef;
use crate::table;


#[derive(Debug)]
pub struct SettingsBuilder<'a> {
    pub method: InstallMethod,
    pub version_query: VersionQuery,
    pub distribution: Option<DistributionRef>,
    pub extra: LinkedHashMap<String, String>,
    pub os: &'a dyn CurrentOs,
    pub methods: LinkedHashMap<InstallMethod, Box<dyn Method + 'a>>,
}

#[derive(Debug)]
pub struct Settings {
    pub method: InstallMethod,
    pub distribution: DistributionRef,
    pub extra: LinkedHashMap<String, String>,
}

impl<'os> SettingsBuilder<'os> {
    pub fn new(os: &'os dyn CurrentOs, options: &Install,
        methods: LinkedHashMap<InstallMethod, Box<dyn Method + 'os>>)
        -> Result<SettingsBuilder<'os>, anyhow::Error>
    {
        let version_query = VersionQuery::new(
            options.nightly, options.version.as_ref());
        Ok(SettingsBuilder {
            os,
            method: options.method.clone()
                .or_else(|| methods.keys().next().cloned())
                .unwrap_or(InstallMethod::Package),
            version_query,
            distribution: None,
            extra: LinkedHashMap::new(),
            methods,
        })
    }
    pub fn build(mut self)
        -> anyhow::Result<(Settings, Box<dyn Method + 'os>)>
    {
        if self.distribution.is_none() {
            anyhow::bail!("No installable version found");
        }
        let method = self.methods.remove(&self.method)
            .expect("method exists");
        let settings = Settings {
            method: self.method,
            distribution: self.distribution.unwrap(),
            extra: self.extra,
        };
        Ok((settings, method))
    }
    pub fn auto_version(&mut self) -> anyhow::Result<()> {
        self.distribution =
            self.methods.get(&self.method).expect("method exists")
            .get_version(&self.version_query)
            .map_err(|e| {
                log::warn!("Unable to determine version: {:#}", e);
            })
            .ok();
        Ok(())
    }
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
            Cell::new(self.distribution.major_version().title()),
            Cell::new(&self.distribution.major_version().option()),
        ]));
        table.add_row(Row::new(vec![
            Cell::new("Exact version"),
            Cell::new(self.distribution.version().as_ref()),
        ]));
        for (k, v) in &self.extra {
            table.add_row(Row::new(vec![
                Cell::new(k),
                Cell::new(v),
            ]));
        }
        table.set_format(*table::FORMAT);
        table.printstd();
    }
}


