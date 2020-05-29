use linked_hash_map::LinkedHashMap;
use prettytable::{Table, Row, Cell};

use crate::server::options::Install;
use crate::server::os_trait::{CurrentOs, Method};
use crate::server::detect::VersionQuery;
use crate::server::version::Version;
use crate::server::methods::InstallMethod;
use crate::table;


#[derive(Debug)]
pub struct SettingsBuilder<'a> {
    pub method: InstallMethod,
    pub version_query: VersionQuery,
    pub package_name: Option<String>,
    pub major_version: Option<Version<String>>,
    pub version: Option<Version<String>>,
    pub extra: LinkedHashMap<String, String>,
    pub os: &'a dyn CurrentOs,
    pub methods: LinkedHashMap<InstallMethod, Box<dyn Method + 'a>>,
}

#[derive(Debug)]
pub struct Settings {
    pub method: InstallMethod,
    pub package_name: String,
    pub major_version: Version<String>,
    pub version: Version<String>,
    pub nightly: bool,
    pub extra: LinkedHashMap<String, String>,
}

impl<'os> SettingsBuilder<'os> {
    pub fn new(os: &'os dyn CurrentOs, options: &Install,
        methods: LinkedHashMap<InstallMethod, Box<dyn Method + 'os>>)
        -> Result<SettingsBuilder<'os>, anyhow::Error>
    {
        let version_query = VersionQuery::new(
            options.nightly, &options.version);
        Ok(SettingsBuilder {
            os,
            method: options.method.clone()
                .or_else(|| methods.keys().next().cloned())
                .unwrap_or(InstallMethod::Package),
            version_query,
            package_name: None,
            major_version: None,
            version: None,
            extra: LinkedHashMap::new(),
            methods,
        })
    }
    pub fn build(mut self)
        -> anyhow::Result<(Settings, Box<dyn Method + 'os>)>
    {
        let method = self.methods.remove(&self.method)
            .expect("method exists");
        let settings = Settings {
            method: self.method,
            package_name: self.package_name.unwrap(),
            major_version: self.major_version.unwrap(),
            version: self.version.unwrap(),
            nightly: self.version_query.is_nightly(),
            extra: self.extra,
        };
        Ok((settings, method))
    }
    pub fn auto_version(&mut self) -> anyhow::Result<()> {
        let res = self.methods.get(&self.method).expect("method exists")
            .get_version(&self.version_query)
            .map_err(|e| {
                log::warn!("Unable to determine version: {:#}", e);
            })
            .ok();
        if let Some(res) = res {
            self.version = Some(res.version);
            self.package_name = Some(res.package_name);
            self.major_version = Some(res.major_version);
        }
        Ok(())
    }
}

impl Settings {
    pub fn print(&self) {
        let mut table = Table::new();
        let version_opt = format!("--version={}", self.major_version);
        table.add_row(Row::new(vec![
            Cell::new("Installation method"),
            Cell::new(self.method.title()),
            Cell::new(self.method.option()),
        ]));
        table.add_row(Row::new(vec![
            Cell::new("Major version"),
            Cell::new(self.major_version.num()),
            Cell::new(if self.nightly {
                "--nightly"
            } else {
                &version_opt
            }),
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
        table.set_format(*table::FORMAT);
        table.printstd();
    }
}


