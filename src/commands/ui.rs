use std::io::{stdout, Write};

use anyhow::Context;

use crate::commands::ExitCode;
use crate::options::{Options, UI};
use crate::portable::local;
use crate::portable::repository::USER_AGENT;
use crate::print;


pub fn show_ui(options: &Options, args: &UI) -> anyhow::Result<()> {
    let connector = options.block_on_create_connector()?;
    let cfg = connector.get()?;

    let local_info = cfg.local_instance_name()
        .map(local::InstanceInfo::try_read).transpose()?.flatten();
    let is_remote = !local_info.is_some();

    let token = if let Some(key) = cfg.secret_key() {
        Some(key.to_owned())
    } else if let Some(instance) = local_info {
        let ver = instance.get_version()?.specific();
        let legacy = ver < "3.0-alpha.1".parse().unwrap();
        jwt::LocalJWT::new(instance.name, legacy).generate()
            .map_err(|e| {
                log::warn!("Cannot generate authToken: {:#}", e);
            })
            .ok()
    } else if matches!(cfg.local_instance_name(), Some("_localdev")) {
        jwt::LocalJWT::new("_localdev", false).generate()
            .map_err(|e| {
                log::warn!("Cannot generate authToken: {:#}", e);
            })
            .ok()
    } else {
        None
    };

    let mut url = cfg.http_url(false).map(|s| s + "/ui")
        .context("connected via unix socket")?;
    if args.no_server_check {
        // We'll always use HTTP if --no-server-check is specified, depending on
        // the server to redirect to HTTPS if necessary.
    } else {
        let mut use_https = false;
        if is_remote {
            let https_url = cfg.http_url(true).map(|u| u + "/ui")
                .context("connected via unix socket")?;
            match open_url(&https_url).map(|r| r.status()) {
                Ok(reqwest::StatusCode::OK) => {
                    url = https_url;
                    use_https = true;
                }
                Ok(status) => {
                    print::echo!(
                        "{} returned status code {}, retry HTTP.",
                        https_url,
                        status
                    );
                }
                Err(e) => {
                    print::echo!("Failed to probe {}: {:#}, retry HTTP.", https_url, e);
                }
            }
        }
        if !use_https {
            match open_url(&url).map(|r| r.status()) {
                Ok(reqwest::StatusCode::OK) => {}
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
                    print::error(format!("cannot connect to {}: {:#}",
                                         url, e,));
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

mod jwt {
    use std::env;
    use std::path::PathBuf;

    use fs_err as fs;
    use ring::rand::SecureRandom;
    use ring::signature::KeyPair;
    use ring::{aead, agreement, digest, rand, signature};

    use crate::platform::data_dir;
    use crate::portable::local::{instance_data_dir, NonLocalInstance};

    #[derive(Debug, thiserror::Error)]
    #[error("Cannot read JOSE key file(s)")]
    pub struct ReadKeyError(anyhow::Error);

    pub struct LocalJWT {
        instance_name: String,
        legacy: bool,
        rng: rand::SystemRandom,
        jws_key: Option<Vec<u8>>,
        jwe_key: Option<Vec<u8>>,
    }

    impl LocalJWT {
        pub fn new(instance_name: impl Into<String>, legacy: bool) -> Self {
            let instance_name = instance_name.into();
            let rng = rand::SystemRandom::new();
            Self {
                instance_name,
                legacy,
                rng,
                jws_key: None,
                jwe_key: None,
            }
        }

        #[cfg(windows)]
        fn read_keys(&mut self) -> anyhow::Result<()> {
            use crate::portable::windows;
            if self.legacy {
                let (jws_key, jwe_key) = windows::read_jose_keys_legacy(&self.instance_name)?;
                self.jws_key = Some(jws_key);
                self.jwe_key = Some(jwe_key);
            } else {
                self.jws_key = Some(windows::read_jws_key(&self.instance_name)?);
            }
            Ok(())
        }
        #[cfg(not(windows))]
        fn read_keys(&mut self) -> anyhow::Result<()> {
            let data_dir = if self.instance_name == "_localdev" {
                match env::var("EDGEDB_SERVER_DEV_DIR") {
                    Ok(path) => PathBuf::from(path),
                    Err(_) => data_dir()?.parent().unwrap().join("_localdev"),
                }
            } else {
                instance_data_dir(&self.instance_name)?
            };
            if !data_dir.exists() {
                anyhow::bail!(NonLocalInstance);
            }
            self.jws_key = Some(fs::read(data_dir.join("edbjwskeys.pem"))?);
            if self.legacy {
                self.jwe_key = Some(fs::read(data_dir.join("edbjwekeys.pem"))?);
            }
            Ok(())
        }

        pub fn generate(&mut self) -> anyhow::Result<String> {
            self.read_keys().map_err(ReadKeyError)?;

            let token = self.generate_token()?;
            if !self.legacy {
                return Ok(format!("edbt_{}", token));
            }

            self.generate_legacy_token(token)
        }

        fn generate_token(&mut self) -> anyhow::Result<String> {
            let jws_pem = pem::parse(self.jws_key.as_deref().expect("jws_key not set"))?;

            let jws = signature::EcdsaKeyPair::from_pkcs8(
                &signature::ECDSA_P256_SHA256_FIXED_SIGNING,
                jws_pem.contents.as_slice(),
            )?;
            let message = format!(
                "{}.{}",
                base64::encode_config(
                    b"{\"typ\":\"JWT\",\"alg\":\"ES256\"}",
                    base64::URL_SAFE_NO_PAD,
                ),
                base64::encode_config(
                    b"{\"edgedb.server.any_role\":true}",
                    base64::URL_SAFE_NO_PAD,
                ),
            );
            let signature = jws.sign(&self.rng, message.as_bytes())?;
            Ok(format!(
                "{}.{}",
                message,
                base64::encode_config(signature, base64::URL_SAFE_NO_PAD),
            ))
        }

        fn generate_legacy_token(&self, signed_token: String) -> anyhow::Result<String> {
            // Replace this ES256/ECDH-ES implementation using raw ring
            // with biscuit when the algorithms are supported in biscuit
            let jwe_pem = pem::parse(self.jwe_key.as_deref().expect("jwe_key not set"))?;
            let jwe = signature::EcdsaKeyPair::from_pkcs8(
                &signature::ECDSA_P256_SHA256_FIXED_SIGNING,
                jwe_pem.contents.as_slice(),
            )?;

            let priv_key =
                agreement::EphemeralPrivateKey::generate(&agreement::ECDH_P256, &self.rng)?;
            let pub_key =
                agreement::UnparsedPublicKey::new(&agreement::ECDH_P256, jwe.public_key().as_ref());
            let epk = priv_key.compute_public_key()?.as_ref().to_vec();
            let cek = agreement::agree_ephemeral(priv_key, &pub_key, (), |key_material| {
                let mut ctx = digest::Context::new(&digest::SHA256);
                ctx.update(&[0, 0, 0, 1]);
                ctx.update(key_material);
                ctx.update(&[0, 0, 0, 7]); // AlgorithmID
                ctx.update(b"A256GCM");
                ctx.update(&[0, 0, 0, 0]); // PartyUInfo
                ctx.update(&[0, 0, 0, 0]); // PartyVInfo
                ctx.update(&[0, 0, 1, 0]); // SuppPubInfo (bitsize=256)
                Ok(ctx.finish())
            })
            .map_err(|_| anyhow::anyhow!("Error occurred deriving key for JWT"))?;
            let enc_key =
                aead::LessSafeKey::new(aead::UnboundKey::new(&aead::AES_256_GCM, cek.as_ref())?);
            let x = base64::encode_config(&epk[1..33], base64::URL_SAFE_NO_PAD);
            let y = base64::encode_config(&epk[33..], base64::URL_SAFE_NO_PAD);
            let protected = format!(
                "{{\
                    \"alg\":\"ECDH-ES\",\"enc\":\"A256GCM\",\"epk\":{{\
                        \"crv\":\"P-256\",\"kty\":\"EC\",\"x\":\"{}\",\"y\":\"{}\"\
                    }}\
                }}",
                x, y
            );
            let protected = base64::encode_config(protected.as_bytes(), base64::URL_SAFE_NO_PAD);
            let mut nonce = vec![0; 96 / 8];
            self.rng.fill(&mut nonce)?;
            let mut in_out = signed_token.as_bytes().to_vec();
            let tag = enc_key.seal_in_place_separate_tag(
                aead::Nonce::try_assume_unique_for_key(&nonce)?,
                aead::Aad::from(protected.clone()),
                &mut in_out,
            )?;

            Ok(format!(
                "{}..{}.{}.{}",
                protected,
                base64::encode_config(nonce, base64::URL_SAFE_NO_PAD),
                base64::encode_config(in_out, base64::URL_SAFE_NO_PAD),
                base64::encode_config(tag.as_ref(), base64::URL_SAFE_NO_PAD),
            ))
        }
    }
}
