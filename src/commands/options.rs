use std::str::FromStr;

use crate::connect::Connector;
use crate::portable;
use crate::print::style::Styler;

pub struct Options {
    pub command_line: bool,
    pub styler: Option<Styler>,
    pub conn_params: Connector,
    pub instance_name: Option<portable::options::InstanceName>,
}

impl Options {
    pub fn infer_instance_name(&mut self) -> anyhow::Result<()> {
        self.instance_name = self
            .conn_params
            .get()?
            .instance_name()
            .map(|x| portable::options::InstanceName::from_str(&x.to_string()))
            .transpose()?;
        Ok(())
    }
}
