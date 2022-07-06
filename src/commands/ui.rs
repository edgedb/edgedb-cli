use std::fs;

use ring::rand::SecureRandom;
use ring::signature::KeyPair;
use ring::{aead, agreement, digest, rand, signature};

use crate::commands::ExitCode;
use crate::options::Options;
use crate::platform::data_dir;
use crate::portable::local::instance_data_dir;
use crate::print;

pub fn show_ui(options: &Options) -> anyhow::Result<()> {
    let connector = options.create_connector()?;
    let builder = connector.get()?;
    let mut url = format!("http://{}:{}/ui", builder.get_host(), builder.get_port());
    if let Some(instance) = &options.conn_options.instance {
        match if cfg!(windows) {
            crate::portable::windows::get_ui_token(instance)
        } else {
            generate_jwt(instance)
        } {
            Ok(token) => {
                url = format!("{}?authToken={}", url, token);
            }
            Err(e) => {
                print::warn(format!("Cannot generate authToken: {:#}", e));
            }
        }
    }
    if open::that(&url).is_ok() {
        Ok(())
    } else {
        print::error("Cannot launch browser, please visit URL:");
        print::echo!("  {}", url);
        Err(ExitCode::new(1).into())
    }
}

pub fn ui_token(name: &str) -> anyhow::Result<()> {
    println!("{}", generate_jwt(name)?);
    Ok(())
}

fn generate_jwt(name: &str) -> anyhow::Result<String> {
    // Replace this ES256/ECDH-ES implementation using raw ring
    // with biscuit when the algorithms are supported in biscuit
    let rng = rand::SystemRandom::new();

    let data_dir = if name == "_localdev" {
        data_dir()?.parent().unwrap().join("_localdev")
    } else {
        instance_data_dir(name)?
    };

    let buffer = fs::read(data_dir.join("edbjwskeys.pem"))?;
    let pem = pem::parse(buffer)?;
    let jws = signature::EcdsaKeyPair::from_pkcs8(
        &signature::ECDSA_P256_SHA256_FIXED_SIGNING,
        pem.contents.as_slice(),
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

    let buffer = fs::read(data_dir.join("edbjwekeys.pem"))?;
    let pem = pem::parse(buffer)?;
    let jwe = signature::EcdsaKeyPair::from_pkcs8(
        &signature::ECDSA_P256_SHA256_FIXED_SIGNING,
        pem.contents.as_slice(),
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
    }).map_err(|_| {
        anyhow::anyhow!("Error occurred deriving key for JWT")
    })?;
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
