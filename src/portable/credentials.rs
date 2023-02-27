use std::io::{stdout, Write};
use url::Url;

use crate::options::Options;
use crate::portable::options::ShowCredentials;

#[derive(serde::Serialize)]
struct Credentials {
    #[serde(skip_serializing_if="Option::is_none")]
    secret_key: Option<String>,

    #[serde(flatten)]
    creds: edgedb_tokio::credentials::Credentials,
}

pub fn show_credentials(options: &Options, c: &ShowCredentials) -> anyhow::Result<()> {
    use edgedb_tokio::credentials::TlsSecurity;

    let connector = options.block_on_create_connector()?;
    let builder: &edgedb_tokio::Builder = connector.get()?;
    let creds = builder.as_credentials()?;
    let secret_key = builder.get_secret_key().map(String::from);
    if let Some(result) = if c.json {
        Some(serde_json::to_string_pretty(&Credentials {
            secret_key,
            creds,
        })?)
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
        let query = [
            (
                "tls_security",
                if creds.tls_security == TlsSecurity::Default {
                    None
                } else {
                    Some(serde_json::to_value(&creds.tls_security).unwrap().as_str().unwrap().into())
                },
            ),
            ("secret_key", secret_key),
        ]
        .iter()
        .map(|(k, v)| v.as_deref().map(|v| format!("{}={}", k, v)))
        .flatten()
        .collect::<Vec<_>>();
        if !query.is_empty() {
            let query = query.join("&");
            url.set_query(Some(&query));
        }
        Some(url.to_string())
    } else {
        crate::table::settings(&[
            ("Host", creds.host.as_deref().unwrap_or("localhost")),
            ("Port", creds.port.to_string().as_str()),
            ("User", creds.user.as_str()),
            ("Password", creds.password.map(|_| "<hidden>").unwrap_or("<none>")),
            ("Secret Key", secret_key.map(|_| "<hidden>").unwrap_or("<none>")),
            ("Database", creds.database.as_deref().unwrap_or("<default>")),
            ("TLS Security", format!("{:?}", creds.tls_security).as_str()),
        ]);
        None
    } {
        stdout().lock().write_all((result + "\n").as_bytes()).expect("stdout write succeeds");
    }
    Ok(())
}
