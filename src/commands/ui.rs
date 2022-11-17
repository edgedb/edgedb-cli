use std::env;
use std::io::{stdout, Write};
use std::path::PathBuf;

use fs_err as fs;
use ring::{rand, signature};

use crate::commands::ExitCode;
use crate::options::{Options, UI};
use crate::platform::data_dir;
use crate::portable::local::{instance_data_dir, NonLocalInstance};
use crate::portable::repository::USER_AGENT;
use crate::print;

pub fn show_ui(options: &Options, args: &UI) -> anyhow::Result<()> {
    let connector = options.create_connector()?;
    let builder = connector.get()?;
    let mut url = format!("http://{}:{}/ui", builder.get_host(), builder.get_port());

    let mut token = None;
    let mut is_remote = true;
    if let Some(instance) = builder.get_instance_name() {
        match read_jose_keys(instance) {
            Ok(keys) => match generate_jwt(keys) {
                Ok(t) => {
                    is_remote = false;
                    token = Some(t);
                }
                Err(e) => {
                    is_remote = false;
                    print::warn(format!("Cannot generate authToken: {:#}", e));
                }
            },
            Err(e) if e.is::<NonLocalInstance>() => {
                // Continue without token for remote instances
                log::debug!("Assuming remote instance because: {:#}", e);
            }
            Err(e) => {
                print::warn(format!("Cannot read JOSE key files: {:#})", e));
            }
        }
    }

    if args.no_server_check {
        // We'll always use HTTP if --no-server-check is specified, depending on
        // the server to redirect to HTTPS if necessary.
    } else {
        let mut use_https = false;
        if is_remote {
            let https_url = "https".to_owned() + url.strip_prefix("http").unwrap();
            match open_url(&https_url).map(|r| r.status()) {
                Ok(reqwest::StatusCode::OK) => {
                    url = https_url;
                    use_https = true;
                }
                Ok(status) => {
                    log::debug!(
                        "GET {} returned status code {}, retry HTTP.",
                        https_url,
                        status
                    );
                }
                Err(e) => {
                    log::debug!("GET {} failed: {:#}", https_url, e);
                }
            }
        }
        if !use_https {
            match open_url(&url).map(|r| r.status()) {
                Ok(reqwest::StatusCode::OK) => (),
                Ok(reqwest::StatusCode::NOT_FOUND) => {
                    print::error("the specified EdgeDB server is not serving Web UI.");
                    print::echo!(
                        "  If you have EdgeDB 2.0 and above, try to run the \
                        server with `--admin-ui=enabled`."
                    );
                    return Err(ExitCode::new(2).into());
                }
                Ok(status) => {
                    log::info!("GET {} returned status code {}", url, status);
                    print::error(
                        "the specified EdgeDB server is not serving Web UI \
                        correctly; check server log for details.",
                    );
                    return Err(ExitCode::new(3).into());
                }
                Err(e) => {
                    print::error(format!("cannot connect to {}: {:#}", url, e,));
                    return Err(ExitCode::new(4).into());
                }
            }
        }
    }

    if let Some(token) = token {
        url = format!("{}?authToken={}", url, token);
    }

    if args.print_url {
        stdout()
            .lock()
            .write_all((url + "\n").as_bytes())
            .expect("stdout write succeeds");
        Ok(())
    } else {
        match open::that(&url) {
            Ok(_) => {
                print::success("Opening URL in browser:");
                println!("{}", url);
                Ok(())
            }
            Err(e) => {
                print::error(format!("Cannot launch browser: {:#}", e));
                print::prompt("Please visit URL:");
                println!("{}", url);
                Err(ExitCode::new(1).into())
            }
        }
    }
}

#[tokio::main]
async fn open_url(url: &str) -> Result<reqwest::Response, reqwest::Error> {
    reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .danger_accept_invalid_hostnames(true)
        .build()?
        .get(url)
        .header(reqwest::header::USER_AGENT, USER_AGENT)
        .send()
        .await
}

fn read_jose_keys(name: &str) -> anyhow::Result<Vec<u8>> {
    if cfg!(windows) {
        crate::portable::windows::read_jose_keys(name)
    } else {
        let data_dir = if name == "_localdev" {
            match env::var("EDGEDB_SERVER_DEV_DIR") {
                Ok(path) => PathBuf::from(path),
                Err(_) => data_dir()?.parent().unwrap().join("_localdev"),
            }
        } else {
            instance_data_dir(name)?
        };
        if !data_dir.exists() {
            anyhow::bail!(NonLocalInstance);
        }
        Ok(fs::read(data_dir.join("edbjwskeys.pem"))?)
    }
}

fn generate_jwt<B: AsRef<[u8]>>(keys: B) -> anyhow::Result<String> {
    let rng = rand::SystemRandom::new();

    let jws_pem = pem::parse(keys)?;

    let jws = signature::EcdsaKeyPair::from_pkcs8(
        &signature::ECDSA_P256_SHA256_FIXED_SIGNING,
        jws_pem.contents.as_slice(),
    )?;
    let message = format!(
        "{}.{}",
        base64::encode_config(
            b"{\"typ\":\"JWT\",\"alg\":\"ES256\"}",
            base64::URL_SAFE_NO_PAD
        ),
        base64::encode_config(
            b"{\"edgedb.server.any_role\":true}",
            base64::URL_SAFE_NO_PAD
        ),
    );
    let signature = jws.sign(&rng, message.as_bytes())?;
    Ok(format!(
        "{}.{}",
        message,
        base64::encode_config(signature, base64::URL_SAFE_NO_PAD),
    ))
}
