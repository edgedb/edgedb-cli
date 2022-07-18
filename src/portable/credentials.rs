use std::io::{stdout, Write};
use url::Url;

use crate::options::Options;
use crate::portable::options::ShowCredentials;

pub fn show_credentials(options: &Options, c: &ShowCredentials) -> anyhow::Result<()> {
    use edgedb_client::credentials::TlsSecurity;

    let connector = options.create_connector()?;
    let builder = connector.get()?;
    let creds = builder.as_credentials()?;
    let result = if c.json {
        serde_json::to_string_pretty(&creds)?
    } else if c.insecure_dsn {
        let mut url = Url::parse(&format!(
            "edgedb://{}@{}:{}",
            creds.user,
            creds.host.unwrap_or("localhost".into()),
            creds.port,
        ))?;
        url.set_password(creds.password.as_deref()).ok();
        if let Some(database) = creds.database {
            url = url.join(&database)?;
        }
        match creds.tls_security {
            TlsSecurity::Strict => {
                url.set_query(Some(&format!("tls_security=strict")));
            }
            TlsSecurity::Insecure => {
                url.set_query(Some(&format!("tls_security=insecure")));
            }
            TlsSecurity::NoHostVerification => {
                url.set_query(Some(&format!("tls_security=no_host_verification")));
            }
            _ => {}
        }
        url.to_string()
    } else {
        format!(
            "Host: {}\n\
            Port: {}\n\
            User: {}\n\
            Password: {}\n\
            Database: {}\n\
            TLS Security: {:?}",
            creds.host.unwrap_or("localhost".into()),
            creds.port,
            creds.user,
            creds.password.map(|_| "<hidden>").unwrap_or("<none>"),
            creds.database.unwrap_or("<default>".into()),
            creds.tls_security,
        )
    };
    stdout().lock().write_all((result + "\n").as_bytes()).expect("stdout write succeeds");
    Ok(())
}
