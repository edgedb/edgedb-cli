use std::sync::{Mutex, Arc};

use edgedb_client::verify_server_cert;
use pem;
use ring::digest;
use rcgen::{Certificate, CertificateParams, DistinguishedName, DnType};
use rustls;
use rustls::{RootCertStore, ServerCertVerifier, ServerCertVerified, TLSError};
use webpki::DNSNameRef;

use crate::options::{Authenticate, Options};
use crate::{question, credentials};
use crate::server::reset_password::write_credentials;

struct InteractiveCertVerifier {
    cert_out: Mutex<Option<String>>,
    verify_hostname: Option<bool>,
    prompt: bool,
    assume_yes: bool,
}

impl InteractiveCertVerifier {
    fn new(
        assume_yes: bool, verify_hostname: Option<bool>, prompt: bool,
    ) -> Self {
        Self {
            cert_out: Mutex::new(None),
            verify_hostname: verify_hostname,
            prompt,
            assume_yes,
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
                // `self.prompt == true` means no cert was provided to the CLI,
                // in which case we should check the hostname
                if self.verify_hostname.unwrap_or(self.prompt) {
                    cert.verify_is_valid_for_dns_name(dns_name)
                        .map_err(TLSError::WebPKIError)?;
                }
            }
            Err(e) => {
                // Bail out if we shouldn't ask the user to trust any cert
                if !self.prompt {
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
                if self.assume_yes {
                    eprintln!(
                        "Trusting unknown server certificate: {:?}",
                        fingerprint,
                    );
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

pub async fn authenticate(cmd: &Authenticate, opts: &Options) -> anyhow::Result<()> {
    let builder = opts.conn_params.get()?;
    let mut creds = builder.as_credentials()?;
    let verifier = Arc::new(
        InteractiveCertVerifier::new(
            cmd.assume_yes,
            creds.tls_verify_hostname,
            creds.tls_cert_data.is_none(),
        )
    );
    builder.connect_with_cert_verifier(Some(verifier.clone())).await?;
    if let Some(cert) = &*verifier.cert_out.lock().unwrap() {
        creds.tls_cert_data = Some(cert.clone());
    }

    let cred_path = match &cmd.name {
        Some(name) => credentials::path(name),
        None => {
            if cmd.assume_yes {
                anyhow::bail!("Instance name required.")
            }
            let mut q = question::String::new(
                "Specify a new instance name for the remote server"
            );
            let default = builder.get_addr().to_string().chars().map(|x| match x {
                'A'..='Z' => x,
                'a'..='z' => x,
                '0'..='9' => x,
                _ => '_',
            }).collect::<String>();
            q.default(&default);
            credentials::path(&q.ask()?)
        }
    }?;
    if cred_path.exists() {
        if cmd.assume_yes {
            eprintln!("{} will be overwritten!", cred_path.display());
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

pub fn generate_self_signed_cert() -> anyhow::Result<(String, String)> {
    let mut distinguished_name = DistinguishedName::new();
    distinguished_name.push(DnType::CommonName, "EdgeDB Development Server");
    let mut cert_params = CertificateParams::new(vec!["127.0.0.1".to_string(), "localhost".to_string()]);
    cert_params.distinguished_name = distinguished_name;
    let cert = Certificate::from_params(cert_params)?;

    let cert_pem = cert.serialize_pem()?;
    let key_pem = cert.serialize_private_key_pem();
    Ok((cert_pem, key_pem))
}

pub fn generate_dev_cert() -> anyhow::Result<()> {
    let (cert_pem, key_pem) = generate_self_signed_cert()?;
    print!("{}", key_pem);
    print!("{}", cert_pem);
    Ok(())
}
