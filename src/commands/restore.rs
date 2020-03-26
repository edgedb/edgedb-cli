use std::ffi::OsString;
use std::default::Default;

use anyhow::Context;
use async_std::path::Path;
use async_std::fs;
use async_std::io::{self, Read, prelude::ReadExt};

use edgedb_protocol::client_message::{ClientMessage, Dump};
use edgedb_protocol::server_message::ServerMessage;
use crate::commands::Options;
use crate::client::Client;

type Input = Box<dyn Read + Unpin + Send>;


pub async fn restore<'x>(cli: &mut Client<'x>, _options: &Options,
    filename: &Path, allow_non_empty: bool)
    -> Result<(), anyhow::Error>
{
    todo!();
}
