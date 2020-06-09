use std::collections::HashMap;
use std::fmt;
use std::io;
use std::str;
use std::sync::Arc;
use std::time::{Instant, Duration};

use anyhow::{self, Context};
use async_std::prelude::StreamExt;
use async_std::future::timeout;
use async_std::io::prelude::WriteExt;
use async_std::net::{TcpStream, ToSocketAddrs};
use async_listen::ByteStream;
use bytes::{Bytes, BytesMut};
use scram::ScramClient;
use serde_json::from_slice;
use typemap::TypeMap;

use edgedb_protocol::client_message::{ClientMessage, ClientHandshake};
use edgedb_protocol::client_message::{Prepare, IoFormat, Cardinality};
use edgedb_protocol::client_message::{DescribeStatement, DescribeAspect};
use edgedb_protocol::client_message::{Execute, ExecuteScript};
use edgedb_protocol::codec::Codec;
use edgedb_protocol::server_message::{ServerMessage, Authentication};
use edgedb_protocol::queryable::{Queryable};
use edgedb_protocol::value::Value;
use edgedb_protocol::descriptors::OutputTypedesc;

use crate::options::Options;
use crate::reader::{QueryableDecoder, QueryResponse};
use crate::server_params::PostgresAddress;

pub use crate::reader::Reader;


pub struct Connection {
    stream: ByteStream,
}

pub struct Writer<'a> {
    stream: &'a ByteStream,
    outbuf: BytesMut,
}

pub struct Client<'a> {
    pub writer: Writer<'a>,
    pub reader: Reader<&'a ByteStream>,
    pub params: TypeMap<dyn typemap::DebugAny + Send>,
}

#[derive(Debug)]
pub struct NoResultExpected {
    pub completion_message: Bytes,
}


impl Connection {
    async fn connect_tcp<A>(addrs: A, options: &Options)
        -> Result<Connection, anyhow::Error>
        where A: ToSocketAddrs+fmt::Debug,
    {
        let start = Instant::now();
        let conn = loop {
            let cres = TcpStream::connect(&addrs).await;
            match cres {
                Err(e) if e.kind() == io::ErrorKind::ConnectionRefused => {
                    if let Some(wait) = options.wait_until_available {
                        if wait > start.elapsed() {
                            continue;
                        } else {
                            Err(e).context(format!("Can't establish \
                                                    connection for {:?}",
                                                    wait))?
                        }
                    } else {
                        Err(e).with_context(
                            || format!("Can't connect to {:?}", addrs))?;
                    }
                }
                Err(e) => {
                    Err(e).with_context(
                        || format!("Can't connect to {:?}", addrs))?;
                }
                Ok(conn) => break conn,
            }
        };
        let s = ByteStream::new_tcp_detached(conn);
        s.set_nodelay(true)?;
        Ok(Connection {
            stream: s,
        })
    }
    #[cfg(unix)]
    pub async fn from_options(options: &Options)
        -> Result<Connection, anyhow::Error>
    {
        use async_std::os::unix::net::UnixStream;

        let unix_host = options.host.contains("/");
        if options.admin || unix_host {
            let prefix = if unix_host {
                &options.host
            } else {
                "/var/run/edgedb"
            };
            let path = if prefix.contains(".s.EDGEDB") {
                // it's the full path
                prefix.into()
            } else {
                if options.admin {
                    format!("{}/.s.EDGEDB.admin.{}", prefix, options.port)
                } else {
                    format!("{}/.s.EDGEDB.{}", prefix, options.port)
                }
            };
            let start = Instant::now();
            let conn = loop {
                let cres = UnixStream::connect(&path).await;
                match cres {
                    Err(e) if e.kind() == io::ErrorKind::ConnectionRefused => {
                        if let Some(wait) = options.wait_until_available {
                            if wait > start.elapsed() {
                                continue;
                            } else {
                                Err(e).context(format!("Can't establish \
                                                        connection for {:?}",
                                                        wait))?
                            }
                        } else {
                            Err(e).with_context(|| format!(
                                "Can't connect to unix socket {:?}", path))?
                        }
                    }
                    Err(e) => {
                        Err(e).with_context(|| format!(
                            "Can't connect to unix socket {:?}", path))?
                    }
                    Ok(conn) => break conn,
                }
            };
            let s = ByteStream::new_unix_detached(conn);
            s.set_nodelay(true)?;
            Ok(Connection {
                stream: s,
            })
        } else {
            Ok(Connection::connect_tcp(
                (&options.host[..], options.port),
                options,
            ).await?)
        }
    }

    #[cfg(windows)]
    pub async fn from_options(options: &Options)
        -> anyhow::Result<Connection>
    {
        Ok(Connection::connect_tcp(
            (&options.host[..], options.port),
            options,
        ).await?)
    }

    pub async fn authenticate<'x>(&'x mut self, options: &Options,
        database: &str)
        -> Result<Client<'x>, anyhow::Error>
    {
        let (rd, stream) = (&self.stream, &self.stream);
        let reader = Reader::new(rd, options.debug_print_frames);
        let mut cli = Client {
            reader,
            writer: Writer {
                outbuf: BytesMut::with_capacity(8912),
                stream,
            },
            params: TypeMap::custom(),
        };
        let mut params = HashMap::new();
        params.insert(String::from("user"), options.user.clone());
        params.insert(String::from("database"), String::from(database));

        cli.send_messages(&[
            ClientMessage::ClientHandshake(ClientHandshake {
                major_ver: 0,
                minor_ver: 7,
                params,
                extensions: HashMap::new(),
            }),
        ]).await?;

        let mut msg = cli.reader.message().await?;
        if let ServerMessage::ServerHandshake {..} = msg {
            eprintln!("WARNING: Connection negotiantion issue {:?}", msg);
            // TODO(tailhook) react on this somehow
            msg = cli.reader.message().await?;
        }
        match msg {
            ServerMessage::Authentication(Authentication::Ok) => {}
            ServerMessage::Authentication(Authentication::Sasl { methods })
            => {
                if methods.iter().any(|x| x == "SCRAM-SHA-256") {
                    cli.scram(&options).await?;
                } else {
                    return Err(anyhow::anyhow!("No supported authentication \
                        methods: {:?}", methods));
                }
            }
            ServerMessage::ErrorResponse(err) => {
                return Err(anyhow::anyhow!("Error authenticating: {}", err));
            }
            msg => {
                return Err(anyhow::anyhow!(
                    "Error authenticating, unexpected message {:?}", msg));
            }
        }

        loop {
            let msg = cli.reader.message().await?;
            match msg {
                ServerMessage::ReadyForCommand(..) => break,
                ServerMessage::ServerKeyData(_) => {
                    // TODO(tailhook) store it somehow?
                }
                ServerMessage::ParameterStatus(par) => {
                    match &par.name[..] {
                        b"pgaddr" => {
                            let pgaddr: PostgresAddress;
                            pgaddr = match from_slice(&par.value[..]) {
                                Ok(a) => a,
                                Err(e) => {
                                    eprintln!("Can't decode param {:?}: {}",
                                        par.name, e);
                                    continue;
                                }
                            };
                            cli.params.insert::<PostgresAddress>(pgaddr);
                        }
                        _ => {},
                    }
                }
                _ => {
                    eprintln!("WARNING: unsolicited message {:?}", msg);
                }
            }
        }
        Ok(cli)
    }
}

impl<'a> Writer<'a> {

    pub async fn send_messages<'x, I>(&mut self, msgs: I)
        -> Result<(), anyhow::Error>
        where I: IntoIterator<Item=&'x ClientMessage>
    {
        self.outbuf.truncate(0);
        for msg in msgs {
            msg.encode(&mut self.outbuf)?;
        }
        self.stream.write_all(&self.outbuf[..]).await?;
        Ok(())
    }

}

impl<'a> Client<'a> {
    pub async fn scram(&mut self, options: &Options)
        -> Result<(), anyhow::Error>
    {
        use edgedb_protocol::client_message::SaslInitialResponse;
        use edgedb_protocol::client_message::SaslResponse;
        use crate::options::Password::*;

        let password = match options.password {
            NoPassword => return Err(anyhow::anyhow!("Password is required. \
                Please specify --password or --password-from-stdin on the \
                command-line.")),
            FromTerminal => {
                rpassword::read_password_from_tty(
                    Some(&format!("Password for '{}': ",
                                  options.user.escape_default())))?
            }
            Password(ref s) => s.clone(),
        };

        let scram = ScramClient::new(&options.user, &password, None);

        let (scram, first) = scram.client_first();
        self.send_messages(&[
            ClientMessage::AuthenticationSaslInitialResponse(
                SaslInitialResponse {
                method: "SCRAM-SHA-256".into(),
                data: Bytes::copy_from_slice(first.as_bytes()),
            }),
        ]).await?;
        let msg = self.reader.message().await?;
        let data = match msg {
            ServerMessage::Authentication(
                Authentication::SaslContinue { data }
            ) => data,
            ServerMessage::ErrorResponse(err) => {
                return Err(err.into());
            }
            msg => {
                return Err(anyhow::anyhow!("Bad auth response: {:?}", msg));
            }
        };
        let data = str::from_utf8(&data[..])
            .map_err(|_| anyhow::anyhow!(
                "invalid utf-8 in SCRAM-SHA-256 auth"))?;
        let scram = scram.handle_server_first(&data)
            .map_err(|e| anyhow::anyhow!("Authentication error: {}", e))?;
        let (scram, data) = scram.client_final();
        self.send_messages(&[
            ClientMessage::AuthenticationSaslResponse(
                SaslResponse {
                    data: Bytes::copy_from_slice(data.as_bytes()),
                }),
        ]).await?;
        let msg = self.reader.message().await?;
        let data = match msg {
            ServerMessage::Authentication(Authentication::SaslFinal { data })
            => data,
            ServerMessage::ErrorResponse(err) => {
                return Err(anyhow::anyhow!(err));
            }
            msg => {
                return Err(anyhow::anyhow!("Bad auth response: {:?}", msg));
            }
        };
        let data = str::from_utf8(&data[..])
            .map_err(|_| anyhow::anyhow!(
                "invalid utf-8 in SCRAM-SHA-256 auth"))?;
        scram.handle_server_final(&data)
            .map_err(|e| anyhow::anyhow!("Authentication error: {}", e))?;
        loop {
            let msg = self.reader.message().await?;
            match msg {
                ServerMessage::Authentication(Authentication::Ok) => break,
                msg => {
                    eprintln!("WARNING: unsolicited message {:?}", msg);
                }
            };
        }
        Ok(())
    }

    pub async fn send_messages<'x, I>(&mut self, msgs: I)
        -> Result<(), anyhow::Error>
        where I: IntoIterator<Item=&'x ClientMessage>
    {
        self.writer.send_messages(msgs).await
    }

    pub async fn err_sync(&mut self) -> Result<(), anyhow::Error> {
        self.writer.send_messages(&[ClientMessage::Sync]).await?;
        timeout(Duration::from_secs(10), self.reader.wait_ready()).await??;
        Ok(())
    }

    pub async fn execute<S>(&mut self, request: S)
        -> Result<Bytes, anyhow::Error>
        where S: ToString,
    {
        self.send_messages(&[
            ClientMessage::ExecuteScript(ExecuteScript {
                headers: HashMap::new(),
                script_text: request.to_string(),
            }),
        ]).await?;
        let status = loop {
            match self.reader.message().await? {
                ServerMessage::CommandComplete(c) => {
                    self.reader.wait_ready().await?;
                    break c.status_data;
                }
                ServerMessage::ErrorResponse(err) => {
                    return Err(anyhow::anyhow!(err));
                }
                msg => {
                    eprintln!("WARNING: unsolicited message {:?}", msg);
                }
            }
        };
        Ok(status)
    }

    async fn _query(&mut self, request: &str, arguments: &Value,
        io_format: IoFormat)
        -> Result<OutputTypedesc, anyhow::Error >
    {
        let statement_name = Bytes::from_static(b"");

        self.send_messages(&[
            ClientMessage::Prepare(Prepare {
                headers: HashMap::new(),
                io_format,
                expected_cardinality: Cardinality::Many,
                statement_name: statement_name.clone(),
                command_text: String::from(request),
            }),
            ClientMessage::Sync,
        ]).await?;

        loop {
            let msg = self.reader.message().await?;
            match msg {
                ServerMessage::PrepareComplete(..) => {
                    self.reader.wait_ready().await?;
                    break;
                }
                ServerMessage::ErrorResponse(err) => {
                    self.reader.wait_ready().await?;
                    return Err(anyhow::anyhow!(err));
                }
                _ => {
                    return Err(anyhow::anyhow!(
                        "Unsolicited message {:?}", msg));
                }
            }
        }

        self.send_messages(&[
            ClientMessage::DescribeStatement(DescribeStatement {
                headers: HashMap::new(),
                aspect: DescribeAspect::DataDescription,
                statement_name: statement_name.clone(),
            }),
            ClientMessage::Flush,
        ]).await?;

        let data_description = loop {
            let msg = self.reader.message().await?;
            match msg {
                ServerMessage::CommandDataDescription(data_desc) => {
                    break data_desc;
                }
                ServerMessage::ErrorResponse(err) => {
                    self.reader.wait_ready().await?;
                    return Err(anyhow::anyhow!(err));
                }
                _ => {
                    return Err(anyhow::anyhow!(
                        "Unsolicited message {:?}", msg));
                }
            }
        };
        let desc = data_description.output()?;
        let incodec = data_description.input()?.build_codec()?;

        let mut arg_buf = BytesMut::with_capacity(8);
        incodec.encode(&mut arg_buf, &arguments)?;

        self.send_messages(&[
            ClientMessage::Execute(Execute {
                headers: HashMap::new(),
                statement_name: statement_name.clone(),
                arguments: arg_buf.freeze(),
            }),
            ClientMessage::Sync,
        ]).await?;
        Ok(desc)
    }

    pub async fn query<R>(&mut self, request: &str, arguments: &Value)
        -> Result<
            QueryResponse<'_, &'a ByteStream, QueryableDecoder<R>>,
            anyhow::Error
        >
        where R: Queryable,
    {
        let desc = self._query(request, arguments, IoFormat::Binary).await?;
        match desc.root_pos() {
            Some(root_pos) => {
                R::check_descriptor(
                    &desc.as_queryable_context(), root_pos)?;
                Ok(self.reader.response(QueryableDecoder::new()))
            }
            None => {
                Err(NoResultExpected {
                    completion_message: self._process_exec().await?
                })?
            }
        }
    }

    pub async fn query_json(&mut self, request: &str, arguments: &Value)
        -> Result<
            QueryResponse<'_, &'a ByteStream, QueryableDecoder<String>>,
            anyhow::Error
        >
    {
        let desc = self._query(request, arguments, IoFormat::Json).await?;
        match desc.root_pos() {
            Some(root_pos) => {
                String::check_descriptor(
                    &desc.as_queryable_context(), root_pos)?;
                Ok(self.reader.response(QueryableDecoder::new()))
            }
            None => {
                Err(NoResultExpected {
                    completion_message: self._process_exec().await?
                })?
            }
        }
    }

    pub async fn query_json_els(&mut self, request: &str, arguments: &Value)
        -> Result<
            QueryResponse<'_, &'a ByteStream, QueryableDecoder<String>>,
            anyhow::Error
        >
    {
        let desc = self._query(request, arguments,
            IoFormat::JsonElements).await?;
        match desc.root_pos() {
            Some(root_pos) => {
                String::check_descriptor(
                    &desc.as_queryable_context(), root_pos)?;
                Ok(self.reader.response(QueryableDecoder::new()))
            }
            None => {
                Err(NoResultExpected {
                    completion_message: self._process_exec().await?
                })?
            }
        }
    }

    pub async fn query_dynamic(&mut self, request: &str, arguments: &Value)
        -> Result<
            QueryResponse<'_, &'a ByteStream, Arc<dyn Codec>>,
            anyhow::Error
        >
    {
        let desc = self._query(request, arguments, IoFormat::Binary).await?;
        let codec = desc.build_codec()?;
        Ok(self.reader.response(codec))
    }

    pub async fn _process_exec(&mut self) -> Result<Bytes, anyhow::Error> {
        let status = loop {
            match self.reader.message().await? {
                ServerMessage::CommandComplete(c) => {
                    self.reader.wait_ready().await?;
                    break c.status_data;
                }
                ServerMessage::ErrorResponse(err) => {
                    return Err(anyhow::anyhow!(err));
                }
                ServerMessage::Data(_) => { }
                msg => {
                    eprintln!("WARNING: unsolicited message {:?}", msg);
                }
            }
        };
        Ok(status)
    }

    #[allow(dead_code)]
    pub async fn execute_args(&mut self, request: &str, arguments: &Value)
        -> Result<Bytes, anyhow::Error>
    {
        self._query(request, arguments, IoFormat::Binary).await?;
        return self._process_exec().await;
    }

    pub async fn get_version(&mut self) -> Result<String, anyhow::Error> {
        let mut q = self.query::<String>(
            "SELECT sys::get_version_as_str()",
            &Value::empty_tuple(),
        ).await?;
        let mut fetched_version = None;
        while let Some(ver) = q.next().await.transpose()? {
            fetched_version = Some(ver);
        }
        return fetched_version
            .ok_or_else(|| anyhow::anyhow!("Can't fetch version"));
    }
}


impl std::error::Error for NoResultExpected {}

impl fmt::Display for NoResultExpected {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "no result expected: {}",
            String::from_utf8_lossy(&self.completion_message[..]))
    }
}
