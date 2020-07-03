use crate::print::style::Styler;
use crate::client;


pub struct Options {
    pub command_line: bool,
    pub styler: Option<Styler>,
    pub conn_params: client::Builder,
}
