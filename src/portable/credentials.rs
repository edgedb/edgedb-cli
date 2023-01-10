use std::io::{stdout, Write};
use url::Url;

use crate::options::Options;
use crate::portable::options::ShowCredentials;

pub fn show_credentials(options: &Options, c: &ShowCredentials) -> anyhow::Result<()> {
    use edgedb_client::credentials::TlsSecurity;

    let connector = options.create_connector()?;
    let builder = connector.get()?;
    let creds = builder.as_credentials()?;
    if let Some(result) = if c.json {
        Some(serde_json::to_string_pretty(&creds)?)
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
            TlsSecurity::Staging => {
                url.set_query(Some(&format!("tls_security=staging")));
            }
            TlsSecurity::Insecure => {
                url.set_query(Some(&format!("tls_security=insecure")));
            }
            TlsSecurity::NoHostVerification => {
                url.set_query(Some(&format!("tls_security=no_host_verification")));
            }
            _ => {}
        }
        Some(url.to_string())
    } else {
        crate::table::settings(&[
            ("Host", creds.host.as_deref().unwrap_or("localhost")),
            ("Port", creds.port.to_string().as_str()),
            ("User", creds.user.as_str()),
            ("Password", creds.password.map(|_| "<hidden>").unwrap_or("<none>")),
            ("Database", creds.database.as_deref().unwrap_or("<default>")),
            ("TLS Security", format!("{:?}", creds.tls_security).as_str()),
        ]);
        None
    } {
        stdout().lock().write_all((result + "\n").as_bytes()).expect("stdout write succeeds");
    }
    Ok(())
}
