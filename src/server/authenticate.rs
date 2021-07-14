use std::sync::{Mutex, Arc};

use anyhow::Context;
use edgedb_client::{verify_server_cert, Builder};
use edgedb_client::errors::PasswordRequired;
use pem;
use ring::digest;
use rustls;
use rustls::{RootCertStore, ServerCertVerifier, ServerCertVerified, TLSError};
use webpki::DNSNameRef;

use crate::options::{Authenticate, Options, ConnectionOptions};
use crate::{question, credentials};
use crate::server::reset_password::write_credentials;

struct InteractiveCertVerifier {
    cert_out: Mutex<Option<String>>,
    verify_hostname: Option<bool>,
    system_ca_only: bool,
    non_interactive: bool,
    quiet: bool,
}

impl InteractiveCertVerifier {
    fn new(
        non_interactive: bool,
        quiet: bool,
        verify_hostname: Option<bool>,
        system_ca_only: bool,
    ) -> Self {
        Self {
            cert_out: Mutex::new(None),
            verify_hostname,
            system_ca_only,
            non_interactive,
            quiet,
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
                if self.non_interactive {
                    if !self.quiet {
                        eprintln!(
                            "Trusting unknown server certificate: {:?}",
                            fingerprint,
                        );
                    }
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

pub async fn authenticate(cmd: &Authenticate, opts: &Options) -> anyhow::Result<()> {
    let builder = opts.conn_params.get()?;
    let mut creds = builder.as_credentials()?;
    let mut verifier = Arc::new(
        InteractiveCertVerifier::new(
            cmd.non_interactive,
            cmd.quiet,
            creds.tls_verify_hostname,
            creds.tls_cert_data.is_none(),
        )
    );
    let r = builder.connect_with_cert_verifier(verifier.clone()).await;
    if let Err(e) = r {
        if e.is::<PasswordRequired>() && !cmd.non_interactive {
            let password = rpassword::read_password_from_tty(
                Some(&format!("Password for '{}': ",
                              builder.get_user().escape_default())))
                .context("error reading password")?;
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
                )
            );
            builder.connect_with_cert_verifier(verifier.clone()).await?;
        } else {
            return Err(e);
        }
    }
    if let Some(cert) = &*verifier.cert_out.lock().unwrap() {
        creds.tls_cert_data = Some(cert.clone());
    }

    let cred_path = match &cmd.name {
        Some(name) => credentials::path(name),
        None => {
            let default = gen_default_instance_name(builder.get_addr());
            if cmd.non_interactive {
                eprintln!("Using generated instance name: {}", &default);
                credentials::path(&default)
            } else {
                let name = question::String::new(
                    "Specify a new instance name for the remote server"
                ).default(&default).ask()?;
                credentials::path(&name)
            }
        }
    }?;
    if cred_path.exists() {
        if cmd.non_interactive {
            if !cmd.quiet {
                eprintln!("Overwriting {}", cred_path.display());
            }
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
    Ok(())
}

pub fn prompt_conn_params(
    options: &ConnectionOptions,
    builder: &mut Builder,
    auth: &Authenticate,
) -> anyhow::Result<()> {
    if auth.non_interactive && options.password {
        anyhow::bail!("Not both --password and authenticate --non-interactive")
    }
    let (host, port) = builder.get_addr().get_tcp_addr().ok_or_else(|| {
        anyhow::anyhow!("Cannot authenticate to a UNIX domain socket.")
    })?;
    let (mut host, mut port) = (host.clone(), *port);
    if options.host.is_none() && host == "127.0.0.1" {
        // Workaround for the `edgedb authenticate`
        // https://github.com/briansmith/webpki/issues/54
        builder.tcp_addr("localhost", port);
        host = "localhost".into();
    }

    if auth.non_interactive {
        if !auth.quiet {
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
