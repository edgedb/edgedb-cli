use std::env;

use anyhow::Context;
use rcgen::{Certificate, CertificateParams, DistinguishedName, DnType};

use crate::options::Authenticate;
use crate::{question, credentials};
use crate::server::reset_password::write_credentials;

use edgedb_client::credentials::Credentials;


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

pub fn authenticate(cmd: &Authenticate) -> anyhow::Result<()> {
    if !cmd.generate_dev_cert {
        anyhow::bail!("Not implemented.")
    }
    let (host, port) = match cmd.host.split_once(':') {
        Some((host, port)) => (host, port.parse::<u16>().context("Illegal port")?),
        None => anyhow::bail!("<host:port> format mismatch."),
    };

    let cred_path = match &cmd.name {
        Some(name) => credentials::path(name),
        None => {
            if cmd.assume_yes {
                anyhow::bail!("Instance name required.")
            }
            let mut q = question::String::new(
                "Specify a new instance name for the remote server"
            );
            let default = cmd.host.chars().map(|x| match x {
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
    let mut creds = Credentials::default();
    if host.len() > 0 {
        creds.host = Some(host.to_string());
    }
    creds.port = port;

    creds.user = match &cmd.user {
        Some(user) => user.into(),
        None => if cmd.assume_yes {
            anyhow::bail!("Database user is required")
        } else {
            question::String::new(
                "Specify a database user to log into the remote server"
            ).default("edgedb").ask()?
        },
    };

    creds.password = match &cmd.password {
        Some(password) => Some(password.into()),
        None => if let Ok(password) = env::var("EDGEDB_PASSWORD") {
            Some(password)
        } else {
            None
        }
    };

    creds.database = cmd.database.clone();

    if cmd.generate_dev_cert {
        let (cert_pem, key_pem) = generate_self_signed_cert()?;
        creds.tls_certdata = Some(cert_pem.clone());
        print!("{}", key_pem);
        print!("{}", cert_pem);
    }
    write_credentials(&cred_path, &creds)?;
    Ok(())
}
