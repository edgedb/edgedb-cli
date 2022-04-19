use url::Url;

use crate::options::Options;
use crate::portable::options::{ShowCredentials, ShowCredentialsType};
use crate::print;

pub fn show_credentials(options: &Options, c: &ShowCredentials) -> anyhow::Result<()> {
    use edgedb_client::credentials::TlsSecurity;

    let connector = options.create_connector()?;
    let builder = connector.get()?;
    let mut creds = builder.as_credentials()?;
    match c._type {
        Some(ShowCredentialsType::InsecureDSN) => {
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
            print::echo!(url);
        }
        _ => {
            print::echo!(serde_json::to_string_pretty(&creds)?);
        }
    }
    Ok(())
}
