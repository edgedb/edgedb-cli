use std::fs;
use std::sync::{Mutex, Arc};

use anyhow::Context;
use async_std::task;
use colorful::Colorful;
use edgedb_client::{verify_server_cert, Builder};
use edgedb_client::errors::PasswordRequired;
use pem;
use ring::digest;
use rustls;
use rustls::{RootCertStore, ServerCertVerifier, ServerCertVerified, TLSError};
use webpki::DNSNameRef;

use crate::connect::Connector;
use crate::hint::HintedError;
use crate::options::{Options, ConnectionOptions};
use crate::options::{conn_params, load_tls_options, ProjectNotFound};
use crate::print;
use crate::{question, credentials};
use crate::server::reset_password::write_credentials;
use crate::server::options::Link;

struct InteractiveCertVerifier {
    cert_out: Mutex<Option<String>>,
    verify_hostname: Option<bool>,
    system_ca_only: bool,
    non_interactive: bool,
    quiet: bool,
    trust_tls_cert: bool,
}

impl InteractiveCertVerifier {
    fn new(
        non_interactive: bool,
        quiet: bool,
        verify_hostname: Option<bool>,
        system_ca_only: bool,
        trust_tls_cert: bool,
    ) -> Self {
        Self {
            cert_out: Mutex::new(None),
            verify_hostname,
            system_ca_only,
            non_interactive,
            quiet,
            trust_tls_cert,
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
        let untrusted_index = presented_certs.len() - 1;
        match verify_server_cert(roots, presented_certs) {
            Ok(cert) => {
                if self.verify_hostname.unwrap_or(self.system_ca_only) {
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
                if self.verify_hostname.unwrap_or(false) {
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

fn gen_default_instance_name(input: &dyn ToString) -> String {
    let input = input.to_string();
    input.strip_suffix(":5656").unwrap_or(&input).chars().map(|x| match x {
        'A'..='Z' => x,
        'a'..='z' => x,
        '0'..='9' => x,
        _ => '_',
    }).collect::<String>()
}

pub fn link(cmd: &Link, opts: &Options) -> anyhow::Result<()> {
    task::block_on(async_link(cmd, opts))
}

async fn async_link(cmd: &Link, opts: &Options) -> anyhow::Result<()> {
    let mut builder = match conn_params(&opts.conn_options) {
        Ok(builder) => builder,
        Err(e) => if let Some(he) = e.downcast_ref::<HintedError>() {
            if he.error.is::<ProjectNotFound>() {
                let mut builder = Builder::new();
                load_tls_options(&opts.conn_options, &mut builder)?;
                builder
            } else {
                return Err(e);
            }
        } else {
            return Err(e);
        }
    };

    prompt_conn_params(&opts.conn_options, &mut builder, cmd)?;

    let mut creds = builder.as_credentials()?;
    let mut verifier = Arc::new(
        InteractiveCertVerifier::new(
            cmd.non_interactive,
            cmd.quiet,
            creds.tls_verify_hostname,
            creds.tls_cert_data.is_none(),
            cmd.trust_tls_cert,
        )
    );
    if let Err(e) = builder.connect_with_cert_verifier(
        verifier.clone()
    ).await {
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
                InteractiveCertVerifier::new(
                    true,
                    false,
                    creds.tls_verify_hostname,
                    creds.tls_cert_data.is_none(),
                    true,
                )
            );
            Connector::new(Ok(builder)).connect_with_cert_verifier(
                verifier.clone()
            ).await?;
        } else {
            return Err(e.into());
        }
    }
    if let Some(cert) = &*verifier.cert_out.lock().unwrap() {
        creds.tls_cert_data = Some(cert.clone());
    }

    let (cred_path, instance_name) = match &cmd.name {
        Some(name) => (credentials::path(name)?, name.clone()),
        None => {
            let default = gen_default_instance_name(builder.get_addr());
            if cmd.non_interactive {
                if !cmd.quiet {
                    eprintln!("Using generated instance name: {}", &default);
                }
                (credentials::path(&default)?, default)
            } else {
                let name = question::String::new(
                    "Specify a new instance name for the remote server"
                ).default(&default).ask()?;
                (credentials::path(&name)?, name)
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

    write_credentials(&cred_path, &creds)?;
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
    let (host, port) = builder.get_addr().get_tcp_addr().ok_or_else(|| {
        anyhow::anyhow!("Cannot link to a UNIX domain socket.")
    })?;
    let (mut host, mut port) = (host.clone(), *port);
    if options.host.is_none() && host == "127.0.0.1" {
        // Workaround for the `edgedb instance link`
        // https://github.com/briansmith/webpki/issues/54
        builder.tcp_addr("localhost", port);
        host = "localhost".into();
    }

    if link.non_interactive {
        if !link.quiet {
            eprintln!(
                "Authenticating to edgedb://{}@{}/{}",
                builder.get_user(),
                builder.get_addr(),
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
            builder.tcp_addr(host, port);
        }
        if options.user.is_none() {
            builder.user(
                question::String::new("Specify the database user")
                    .default(builder.get_user())
                    .ask()?
            );
        }
        if options.database.is_none() {
            builder.database(
                question::String::new("Specify the database name")
                    .default(builder.get_database())
                    .ask()?
            );
        }
    }
    Ok(())
}

pub fn unlink(name: &str) -> anyhow::Result<()> {
    fs::remove_file(credentials::path(name)?)
        .context(format!("Cannot unlink {}", name))?;
    Ok(())
}
