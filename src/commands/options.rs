use crate::print::style::Styler;
use crate::connect::Connector;


pub struct Options {
    pub command_line: bool,
    pub styler: Option<Styler>,
    pub conn_params: Connector,
}
