use crate::print::style::Styler;
use edgedb_client::builder;


pub struct Options {
    pub command_line: bool,
    pub styler: Option<Styler>,
    pub conn_params: builder::Builder,
}
