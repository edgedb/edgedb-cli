use std::env;
use std::io::{stdout, Write};
use std::path::PathBuf;
use std::str::FromStr;
use std::time::Duration;

use async_std::future::timeout;
use async_std::prelude::FutureExt;
use async_std::task;
use edgedb_client::Builder;
use fs_err as fs;
use ring::rand::SecureRandom;
use ring::signature::KeyPair;
use ring::{aead, agreement, digest, rand, signature};

use crate::commands::ExitCode;
use crate::options::{Options, UI};
use crate::platform::data_dir;
use crate::portable::local::{instance_data_dir, InstanceInfo};
use crate::portable::ver::Specific;
use crate::print;

pub fn show_ui(options: &Options, args: &UI) -> anyhow::Result<()> {
    let connector = options.create_connector()?;
    let builder = connector.get()?;
    let mut url = format!("http://{}:{}/ui", builder.get_host(), builder.get_port());

    if !args.skip_version_check {
        let version = if let Some(meta) = builder
            .get_instance_name()
            .map(InstanceInfo::try_read)
            .unwrap_or(Ok(None))?
        {
            meta.get_version()?.specific()
        } else {
            task::block_on(get_remote_version(builder))?
        };
        if version.major < 2 {
            print::error(format!(
                "the specified instance runs EdgeDB v{}, which does not \
                include the Web UI; consider upgrading the instance to \
                EdgeDB 2.x or later",
                version,
            ));
            return Err(ExitCode::new(2).into());
        }
    }

    if let Some(instance) = builder.get_instance_name() {
        match if cfg!(windows) {
            crate::portable::windows::read_jose_keys(instance)
        } else {
            read_jose_keys(instance)
        } {
            Ok(keys) => match generate_jwt(keys) {
                Ok(token) => {
                    url = format!("{}?authToken={}", url, token);
                }
                Err(e) => {
                    print::warn(format!("Cannot generate authToken: {:#}", e));
                }
            },
            Err(e) => {
                // Ignore the error assuming it's a remote instance
                log::debug!(
                    "Cannot find JOSE key files ({:#}), assuming remote instance.",
                    e,
                )
            }
        }
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

async fn _get_remote_version(builder: &Builder) -> anyhow::Result<String> {
    Ok(builder.build()?.connect().await?.get_version().await?)
}

async fn get_remote_version(builder: &Builder) -> anyhow::Result<Specific> {
    let ver = timeout(Duration::from_secs(15), _get_remote_version(builder))
        .race(async {
            task::sleep(Duration::from_millis(300)).await;
            if atty::is(atty::Stream::Stderr) {
                eprintln!("{}", "Trying to connect...");
            }
            async_std::future::pending().await
        })
        .await??;
    Ok(Specific::from_str(&ver)?)
}

fn read_jose_keys(name: &str) -> anyhow::Result<(Vec<u8>, Vec<u8>)> {
    let data_dir = if name == "_localdev" {
        match env::var("EDGEDB_SERVER_DEV_DIR") {
            Ok(path) => PathBuf::from(path),
            Err(_) => data_dir()?.parent().unwrap().join("_localdev"),
        }
    } else {
        instance_data_dir(name)?
    };
    Ok((
        fs::read(data_dir.join("edbjwskeys.pem"))?,
        fs::read(data_dir.join("edbjwekeys.pem"))?,
    ))
}

fn generate_jwt<B: AsRef<[u8]>>(keys: (B, B)) -> anyhow::Result<String> {
    // Replace this ES256/ECDH-ES implementation using raw ring
    // with biscuit when the algorithms are supported in biscuit
    let rng = rand::SystemRandom::new();

    let jws_pem = pem::parse(keys.0)?;
    let jwe_pem = pem::parse(keys.1)?;

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
    let signed_token = format!(
        "{}.{}",
        message,
        base64::encode_config(signature, base64::URL_SAFE_NO_PAD),
    );

    let jwe = signature::EcdsaKeyPair::from_pkcs8(
        &signature::ECDSA_P256_SHA256_FIXED_SIGNING,
        jwe_pem.contents.as_slice(),
    )?;

    let priv_key = agreement::EphemeralPrivateKey::generate(&agreement::ECDH_P256, &rng)?;
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
    let enc_key = aead::LessSafeKey::new(aead::UnboundKey::new(&aead::AES_256_GCM, cek.as_ref())?);
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
    rng.fill(&mut nonce)?;
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
