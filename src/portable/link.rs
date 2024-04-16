use std::fmt;
use std::fs;
use std::path::PathBuf;
use std::sync::{Mutex, Arc};


use anyhow::Context;
use colorful::Colorful;
use ring::digest;

use rustls::client::WebPkiServerVerifier;
use rustls::client::danger::{ServerCertVerifier, ServerCertVerified};
use rustls::client::danger::HandshakeSignatureValid;
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{SignatureScheme, DigitallySignedStruct};

use edgedb_tokio::credentials::TlsSecurity;
use edgedb_errors::{Error, PasswordRequired, ClientNoCredentialsError};
use edgedb_tokio::{Client, tls};
use edgedb_tokio::{Builder, Config};
use rustyline::error::ReadlineError;

use crate::credentials;
use crate::hint::HintExt;
use crate::options::{Options, ConnectionOptions};
use crate::options;
use crate::portable::destroy::with_projects;
use crate::portable::local::{InstanceInfo, is_valid_local_instance_name};
use crate::portable::options::{Link, Unlink, instance_arg, InstanceName};
use crate::portable::project;
use crate::portable::ver::Build;
use crate::print;
use crate::question;
use crate::tty_password;


#[derive(Debug)]
struct InteractiveCertVerifier {
    inner: Arc<WebPkiServerVerifier>,
    cert_out: Mutex<Option<Vec<u8>>>,
    tls_security: TlsSecurity,
    system_ca_only: bool,
    non_interactive: bool,
    quiet: bool,
    trust_tls_cert: bool,
}

impl ServerCertVerifier for InteractiveCertVerifier {
    fn verify_server_cert(&self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        server_name: &ServerName,
        ocsp_response: &[u8],
        now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        use rustls::Error::InvalidCertificate;

        if let TlsSecurity::Insecure = self.tls_security {
            return Ok(ServerCertVerified::assertion());
        }
        match self.inner.verify_server_cert(
            end_entity, intermediates, server_name, ocsp_response, now)
        {
            Ok(val) => {
                return Ok(val);
            }
            Err(InvalidCertificate(cert_err))
            if matches!(cert_err, rustls::CertificateError::UnknownIssuer) => {
                // reconstruct Error for easier fallthrough
                let e = InvalidCertificate(cert_err);

                if !self.system_ca_only {
                    // Don't continue if the verification failed when the user
                    // already specified a certificate to trust
                    return Err(e);
                }

                let mut root_store = rustls::RootCertStore::empty();
                root_store.add(end_entity.clone())?;
                tls::NoHostnameVerifier::new(Arc::new(root_store))
                    .verify_server_cert(
                        end_entity, intermediates, server_name,
                        ocsp_response, now
                    )?;

                // Acquire consensus to trust the root of presented_certs chain
                let fingerprint = digest::digest(
                    &digest::SHA1_FOR_LEGACY_USE_ONLY,
                    end_entity,
                );
                if self.trust_tls_cert {
                    if !self.quiet {
                        print::warn(format!(
                            "Trusting unknown server certificate: {:?}",
                            fingerprint,
                        ));
                    }
                } else if self.non_interactive {
                    return Err(e);
                } else if let Ok(answer) = question::Confirm::new(
                    format!(
                        "Unknown server certificate: {:?}. Trust?",
                        fingerprint,
                    )
                ).default(false).ask() {
                    if !answer {
                        return Err(e);
                    }
                } else {
                    return Err(e);
                }

                // Export the cert in PEM format and return verification success
                *self.cert_out.lock().unwrap() = Some(end_entity.to_vec());
            }
            Err(e) => return Err(e),
        }

        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        self.inner.verify_tls12_signature(message, cert, dss)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        self.inner.verify_tls13_signature(message, cert, dss)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.inner.supported_verify_schemes()
    }
}

fn gen_default_instance_name(input: impl fmt::Display) -> String {
    let input = input.to_string();
    let mut name = input.strip_suffix(":5656").unwrap_or(&input)
        .chars().map(|x| match x {
            'A'..='Z' => x,
            'a'..='z' => x,
            '0'..='9' => x,
            _ => '_',
        }).collect::<String>();
    if name.is_empty() {
        return "inst1".into();
    }
    if name.chars().next().unwrap().is_ascii_digit() {
        name.insert(0, '_');
    }
    name
}

#[tokio::main(flavor = "current_thread")]
async fn connect(cfg: &edgedb_tokio::Config) -> Result<Client, Error> {
    //Connection::connect(cfg).await
    let client = edgedb_tokio::Client::new(cfg);
    client.ensure_connected().await?;

    Ok(client)
}

#[tokio::main(flavor = "current_thread")]
async fn conn_params(cmd: &Link, opts: &Options, has_branch: &mut bool) -> anyhow::Result<Config> {
    let mut builder = options::prepare_conn_params(opts)?;
    prompt_conn_params(&opts.conn_options, &mut builder, cmd, has_branch).await
}

#[tokio::main(flavor = "current_thread")]
async fn get_server_version(connection: &mut Client) -> anyhow::Result<Build> {
    let ver: String = connection.query_required_single("SELECT sys::get_version_as_str()", &()).await?;
    ver.parse()
}

#[tokio::main(flavor = "current_thread")]
async fn get_default_branch(connection: &mut Client) -> anyhow::Result<String> {
    let default_branch = connection.query_required_single::<String, _>(
        "select sys::get_current_database()",
        &()
    ).await;

    // for context why '?' isn't used, tokio swallows the error here and prints:
    // "edgedb error: ClientConnectionError: A Tokio 1.x context was found, but it is being shutdown."
    // whereas this ensures that the result is properly handled and the actual error is reported.
    if let Ok(branch) = default_branch {
        return Ok(branch);
    }

    anyhow::bail!(default_branch.unwrap_err());
}

pub fn link(cmd: &Link, opts: &Options) -> anyhow::Result<()> {
    if matches!(cmd.name, Some(InstanceName::Cloud { .. })) {
        anyhow::bail!(
            "cloud instances cannot be linked\
            \nTo connect run:\
            \n  edgedb -I {}", cmd.name.as_ref().unwrap());
    }

    let mut has_branch: bool = false;
    let config: Config = conn_params(cmd, opts, &mut has_branch)?;
    let mut creds = config.as_credentials()?;
    let root_cert_store = config.root_cert_store()?;
    let inner = WebPkiServerVerifier::builder(Arc::new(root_cert_store)).build()?;
    let verifier = Arc::new(
        InteractiveCertVerifier {
            inner,
            cert_out: Mutex::new(None),
            tls_security: creds.tls_security,
            system_ca_only: creds.tls_ca.is_none(),
            non_interactive: cmd.non_interactive,
            quiet: cmd.quiet,
            trust_tls_cert: cmd.trust_tls_cert,
        }
    );
    let mut config = config.with_cert_verifier(verifier.clone());
    let mut connect_result = connect(&config);
    if let Err(e) = connect_result {
        if e.is::<PasswordRequired>() {
            let password;

            if opts.conn_options.password_from_stdin {
                password = tty_password::read_stdin()?
            } else if !cmd.non_interactive {
                password = tty_password::read(format!(
                        "Password for '{}': ",
                        config.user().escape_default()))?;
            } else {
                return Err(e.into());
            }

            config = config.with_password(&password);
            creds.password = Some(password);
            if let Some(cert) = &*verifier.cert_out.lock().unwrap() {
                let pem = pem::encode(&pem::Pem::new("CERTIFICATE", cert.to_vec()));
                config = config.with_pem_certificates(&pem)?;
            }
            connect_result = Ok(connect(&config)?);
        } else {
            return Err(e.into());
        }
    }

    let mut connection: Client = connect_result.unwrap();
    let ver = get_server_version(&mut connection)?;

    if !has_branch && opts.conn_options.branch.is_none() && opts.conn_options.database.is_none() {
        config = config.with_database(&get_default_branch(&mut connection)?)?;

        eprintln!(
            "using the default {} '{}'",
            if ver.specific().major >= 5 { "branch" } else { "database" },
            config.database()
        )
    }

    if let Some(cert) = &*verifier.cert_out.lock().unwrap() {
        creds.tls_ca = Some(pem::encode(&pem::Pem::new("CERTIFICATE", cert.to_vec())));
    }

    let (cred_path, instance_name) = match &cmd.name {
        Some(InstanceName::Local(name)) => (credentials::path(name)?, name.clone()),
        Some(InstanceName::Cloud { .. }) => unreachable!(),
        None => {
            let default = gen_default_instance_name(config.display_addr());
            if cmd.non_interactive {
                if !cmd.quiet {
                    eprintln!("Using generated instance name: {}", &default);
                }
                (credentials::path(&default)?, default)
            } else {
                loop {
                    let name = question::String::new(
                        "Specify a new instance name for the remote server"
                    ).default(&default).ask()?;
                    if !is_valid_local_instance_name(&name) {
                        print::error(
                            "Instance name must be a valid identifier, \
                             (regex: ^[a-zA-Z_0-9]+(-[a-zA-Z_0-9]+)*$)");
                        continue;
                    }
                    break (credentials::path(&name)?, name);
                }
            }
        }
    };
    if cred_path.exists() {
        if cmd.overwrite {
            if !cmd.quiet {
                print::warn(format!("Overwriting {}", cred_path.display()));
            }
        } else if cmd.non_interactive {
            anyhow::bail!("File {} exists; aborting.", cred_path.display());
        } else {
            let mut q = question::Confirm::new_dangerous(
                format!("{} already exists! Overwrite?", cred_path.display())
            );
            q.default(false);
            if !q.ask()? {
                anyhow::bail!("Canceled.")
            }
        }
    }

    credentials::write(&cred_path, &creds)?;
    if !cmd.quiet {
        let mut msg = "Successfully linked to remote instance.".to_string();
        if print::use_color() {
            msg = format!("{}", msg.bold().light_green());
        }
        eprintln!(
            "{} To connect run:\
            \n  edgedb -I {}",
            msg,
            instance_name.escape_default(),
        );
    }
    Ok(())
}

async fn prompt_conn_params(
    options: &ConnectionOptions,
    builder: &mut Builder,
    link: &Link,
    has_branch: &mut bool
) -> anyhow::Result<Config> {
    if link.non_interactive && options.password {
        anyhow::bail!(
            "--password and --non-interactive are mutually exclusive."
        )
    }

    if link.non_interactive {
        let config = match builder.build_env().await {
            Ok(config) => config,
            Err(e) if e.is::<ClientNoCredentialsError>() => {
                return Err(anyhow::anyhow!(
                        "no connection options are specified")
                    ).hint("Remove `--non-interactive` option or specify \
                           `--host=localhost` and/or `--port=5656`. \
                           See `edgedb --help-connect` for details")?;
            }
            Err(e) => return Err(e)?,
        };
        if !link.quiet {
            eprintln!(
                "Authenticating to edgedb://{}@{}/{}",
                config.user(),
                config.display_addr(),
                config.database(),
            );
        }
        Ok(config)
    } else if options.dsn.is_none() {
        let (_, config, _) = builder.build_no_fail().await;
        if options.host.is_none() {
            builder.host(
                &question::String::new("Specify server host")
                    .default(config.host().unwrap_or("localhost"))
                    .ask()?
            )?;
        };
        if options.port.is_none() {
            builder.port(
                question::String::new("Specify server port")
                    .default(&config.port().unwrap_or(5656).to_string())
                    .ask()?
                    .parse()?
            )?;
        }
        if options.user.is_none() {
            builder.user(
                &question::String::new("Specify database user")
                    .default(config.user())
                    .ask()?
            )?;
        }

        if options.database.is_none() && options.branch.is_none() {
            match question::String::new("Specify database/branch (CTRL + D for default)").ask() {
                Ok(s) => {
                    builder.database(&s)?.branch(&s)?;
                    *has_branch = true;
                },
                Err(e) => {
                    match e.downcast_ref() {
                        Some(ReadlineError::Eof) => {}
                        Some(_) | None => anyhow::bail!(e)
                    }
                }
            };
        }

        Ok(builder.build_env().await?)
    } else {
        Ok(builder.build_env().await?)
    }
}

pub fn print_warning(name: &str, project_dirs: &[PathBuf]) {
    project::print_instance_in_use_warning(name, project_dirs);
    eprintln!("If you really want to unlink the instance, run:");
    eprintln!("  edgedb instance unlink -I {:?} --force", name);
}

pub fn unlink(options: &Unlink) -> anyhow::Result<()> {
    let name = match instance_arg(&options.name, &options.instance)? {
        InstanceName::Local(name) => name,
        inst_name => {
            return Err(
                anyhow::anyhow!("cannot unlink cloud instance {}.", inst_name)
            ).with_hint(|| format!(
                "use `edgedb instance destroy -I {}` to remove the instance",
                inst_name))?;
        }
    };
    let inst = InstanceInfo::try_read(name)?;
    if inst.is_some() {
        return Err(
            anyhow::anyhow!("cannot unlink local instance {:?}.", name)
        ).with_hint(|| format!(
            "use `edgedb instance destroy -I {}` to remove the instance",
             name))?;
    }
    with_projects(name, options.force, print_warning, || {
        fs::remove_file(credentials::path(name)?)
            .with_context(|| format!("cannot unlink {}", name))
    })?;
    Ok(())
}
