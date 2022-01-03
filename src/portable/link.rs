use std::fmt;
use std::fs;
use std::sync::{Mutex, Arc};

use anyhow::Context;
use async_std::task;
use colorful::Colorful;
use pem;
use ring::digest;
use rustls;
use rustls::{RootCertStore, ServerCertVerifier, ServerCertVerified, TLSError};
use webpki::DNSNameRef;

use edgedb_client::{tls::verify_server_cert, Builder};
use edgedb_client::errors::{Error, PasswordRequired, ClientNoCredentialsError};
use edgedb_client::credentials::TlsSecurity;

use crate::connect::Connector;
use crate::credentials;
use crate::hint::{HintedError, HintExt};
use crate::options::{Options, ConnectionOptions};
use crate::options::{conn_params, load_tls_options};
use crate::portable::local::{InstanceInfo, is_valid_name};
use crate::portable::options::{Link, Unlink};
use crate::print;
use crate::question;


struct InteractiveCertVerifier {
    cert_out: Mutex<Option<String>>,
    tls_security: TlsSecurity,
    system_ca_only: bool,
    non_interactive: bool,
    quiet: bool,
    trust_tls_cert: bool,
}

impl InteractiveCertVerifier {
    fn verify_hostname(&self, default: bool) -> bool {
        match self.tls_security {
            TlsSecurity::Default => default,
            TlsSecurity::Strict => true,
            _ => false,
        }
    }
}

impl ServerCertVerifier for InteractiveCertVerifier {
    fn verify_server_cert(&self,
                          roots: &RootCertStore,
                          presented_certs: &[rustls::Certificate],
                          dns_name: DNSNameRef,
                          _ocsp_response: &[u8],
    ) -> Result<ServerCertVerified, TLSError> {
        if let TlsSecurity::Insecure = self.tls_security {
            return Ok(ServerCertVerified::assertion());
        }
        let untrusted_index = presented_certs.len() - 1;
        match verify_server_cert(roots, presented_certs) {
            Ok(cert) => {
                if self.verify_hostname(self.system_ca_only) {
                    cert.verify_is_valid_for_dns_name(dns_name)
                        .map_err(TLSError::WebPKIError)?;
                }
            }
            Err(e) => {
                if !self.system_ca_only {
                    // Don't continue if the verification failed when the user
                    // already specified a certificate to trust
                    return Err(e);
                }

                // Make sure the verification with the to-be-trusted cert
                // trusted is a success before asking the user
                let mut roots = RootCertStore::empty();
                roots.add(&presented_certs[untrusted_index])
                    .map_err(TLSError::WebPKIError)?;
                let cert = verify_server_cert(&roots, presented_certs)?;
                if self.verify_hostname(false) {
                    cert.verify_is_valid_for_dns_name(dns_name)
                        .map_err(TLSError::WebPKIError)?;
                }

                // Acquire consensus to trust the root of presented_certs chain
                let fingerprint = digest::digest(
                    &digest::SHA1_FOR_LEGACY_USE_ONLY,
                    &presented_certs[untrusted_index].0
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
                } else {
                    if let Ok(answer) = question::Confirm::new(
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
                }

                // Export the cert in PEM format and return verification success
                *self.cert_out.lock().unwrap() = Some(
                    pem::encode(&pem::Pem {
                        tag: "CERTIFICATE".into(),
                        contents: presented_certs[untrusted_index].0.clone(),
                    })
                );
            }
        }

        Ok(ServerCertVerified::assertion())
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
    if matches!(name.chars().next().unwrap(), '0'..='9') {
        name.insert(0, '_');
    }
    return name;
}

fn is_no_credentials_error(mut e: &anyhow::Error) -> bool {
    if let Some(he) = e.downcast_ref::<HintedError>() {
        e = &he.error;
    }
    if let Some(e) = e.downcast_ref::<Error>() {
        return e.is::<ClientNoCredentialsError>();
    }
    return false;
}

pub fn link(cmd: &Link, opts: &Options) -> anyhow::Result<()> {
    let mut builder = match conn_params(&opts.conn_options) {
        Ok(builder) => builder,
        Err(e) if is_no_credentials_error(&e) => {
            Builder::uninitialized()
        }
        Err(e) => {
            return Err(e);
        }
    };

    prompt_conn_params(&opts.conn_options, &mut builder, cmd)?;
    load_tls_options(&opts.conn_options, &mut builder)?;

    let mut creds = builder.as_credentials()?;
    let mut verifier = Arc::new(
        InteractiveCertVerifier {
            cert_out: Mutex::new(None),
            tls_security: creds.tls_security,
            system_ca_only: creds.tls_ca.is_none(),
            non_interactive: cmd.non_interactive,
            quiet: cmd.quiet,
            trust_tls_cert: cmd.trust_tls_cert,
        }
    );
    let connect_result = task::block_on(
        builder.connect_with_cert_verifier(verifier.clone()));
    if let Err(e) = connect_result {
        if e.is::<PasswordRequired>() {
            let password;

            if opts.conn_options.password_from_stdin {
                password = rpassword::read_password()?;
            } else if !cmd.non_interactive {
                password = rpassword::read_password_from_tty(
                    Some(&format!("Password for '{}': ",
                                builder.get_user().escape_default())))
                    .context("error reading password")?;
            } else {
                return Err(e.into());
            }

            let mut builder = builder.clone();
            builder.password(&password);
            if let Some(cert) = &*verifier.cert_out.lock().unwrap() {
                builder.pem_certificates(cert)?;
            }
            creds = builder.as_credentials()?;
            verifier = Arc::new(
                InteractiveCertVerifier {
                    cert_out: Mutex::new(None),
                    tls_security: creds.tls_security,
                    system_ca_only: creds.tls_ca.is_none(),
                    non_interactive: true,
                    quiet: false,
                    trust_tls_cert: true,
                }
            );
            task::block_on(Connector::new(Ok(builder))
                           .connect_with_cert_verifier(verifier.clone()))?;
        } else {
            return Err(e.into());
        }
    }
    if let Some(cert) = &*verifier.cert_out.lock().unwrap() {
        creds.tls_ca = Some(cert.clone());
    }

    let (cred_path, instance_name) = match &cmd.name {
        Some(name) => (credentials::path(name)?, name.clone()),
        None => {
            let default = gen_default_instance_name(builder.display_addr());
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
                    if !is_valid_name(&name) {
                        print::error(
                            "Instance name must be a valid identifier, \
                             (regex: ^[a-zA-Z_][a-zA-Z_0-9]*$)");
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
            anyhow::bail!("File {} exists; abort.", cred_path.display());
        } else {
            let mut q = question::Confirm::new_dangerous(
                format!("{} exists! Overwrite?", cred_path.display())
            );
            q.default(false);
            if !q.ask()? {
                anyhow::bail!("Cancelled.")
            }
        }
    }

    task::block_on(credentials::write(&cred_path, &creds))?;
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

fn prompt_conn_params(
    options: &ConnectionOptions,
    builder: &mut Builder,
    link: &Link,
) -> anyhow::Result<()> {
    if link.non_interactive && options.password {
        anyhow::bail!(
            "--password and --non-interactive are mutually exclusive."
        )
    }
    if builder.get_unix_path().is_some() {
        anyhow::bail!("Cannot link to a UNIX domain socket.")
    };
    let mut host = builder.get_host().to_string();
    let mut port = builder.get_port();

    if link.non_interactive {
        if !builder.is_initialized() {
            return Err(anyhow::anyhow!("no connection options are specified"))
                .hint("Remove `--non-interactive` option or specify \
                      `--host=localhost` and/or `--port=5656`. \
                      See `edgedb --help-connect` for details")?;
        }
        if !link.quiet {
            eprintln!(
                "Authenticating to edgedb://{}@{}/{}",
                builder.get_user(),
                builder.display_addr(),
                builder.get_database(),
            );
        }
    } else {
        if options.host.is_none() {
            host = question::String::new("Specify the host of the server")
                .default(&host)
                .ask()?
        };
        if options.port.is_none() {
            port = question::String::new("Specify the port of the server")
                .default(&port.to_string())
                .ask()?
                .parse()?
        }
        if options.host.is_none() || options.port.is_none() {
            builder.host_port(Some(host), Some(port));
        }
        if let Some(user) = &options.user {
            builder.user(user);
        } else {
            builder.user(
                question::String::new("Specify the database user")
                    .default(builder.get_user())
                    .ask()?
            );
        }
        if let Some(database) = &options.database {
            builder.database(database);
        } else {
            builder.database(
                question::String::new("Specify the database name")
                    .default(builder.get_database())
                    .ask()?
            );
        }
    }
    Ok(())
}

pub fn unlink(options: &Unlink) -> anyhow::Result<()> {
    let inst = InstanceInfo::try_read(&options.name)?;
    if inst.is_some() {
        return Err(
            anyhow::anyhow!("cannot unlink local instance {:?}.", options.name)
        ).with_hint(|| format!(
            "use `edgedb instance destroy {}` to remove the instance",
             options.name))?;
    }
    fs::remove_file(credentials::path(&options.name)?)
        .with_context(|| format!("cannot unlink {}", options.name))?;
    Ok(())
}
