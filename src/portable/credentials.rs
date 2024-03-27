use std::io::{stdout, Write};
use url::Url;

use crate::options::Options;
use crate::portable::options::ShowCredentials;

pub fn show_credentials(options: &Options, c: &ShowCredentials) -> anyhow::Result<()> {
    use edgedb_tokio::credentials::TlsSecurity;

    let connector = options.block_on_create_connector()?;
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
            ("Host", creds.host.unwrap_or("localhost".to_string())),
            ("Port", creds.port.to_string()),
            ("User", creds.user),
            ("Password", creds.password.map(|_| "<hidden>".to_string()).unwrap_or("<none>".to_string())),
            ("Database", creds.database.unwrap_or("<default>".to_string())),
            ("TLS Security", format!("{:?}", creds.tls_security)),
        ]);
        None
    } {
        stdout().lock().write_all((result + "\n").as_bytes()).expect("stdout write succeeds");
    }
    Ok(())
}
