use crate::connect::Connector;
use crate::print::style::Styler;

pub struct Options {
    pub command_line: bool,
    pub styler: Option<Styler>,
    pub conn_params: Connector,
}
