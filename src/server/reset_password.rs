use std::fs;
use std::path::Path;
use std::default::Default;
use std::convert::TryFrom;
use std::num::NonZeroU32;

use anyhow::Context;
use async_std::task;
use base64::display::Base64Display;
use edgedb_client::Builder;
use edgedb_client::credentials::Credentials;
use edgeql_parser::helpers::{quote_string, quote_name};
use fn_error_context::context;
use rand::{Rng, SeedableRng};

use crate::server;
use crate::server::options::ResetPassword;
use crate::server::detect;
use crate::server::control;
use crate::platform::{home_dir, tmp_file_name};

const PASSWORD_LENGTH: usize = 24;
const PASSWORD_CHARS: &[u8] = b"0123456789\
    abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ";
const HASH_ITERATIONS: u32 = 4096;
const SALT_LENGTH: usize = 16;

pub fn generate_password() -> String {
    let mut rng = rand::rngs::StdRng::from_entropy();
    (0..PASSWORD_LENGTH).map(|_| {
        PASSWORD_CHARS[rng.gen_range(0, PASSWORD_CHARS.len())] as char
    }).collect()
}

#[context("error reading credentials at {}", path.display())]
fn read_credentials(path: &Path) -> anyhow::Result<Credentials> {
    let data = fs::read(&path)?;
    Ok(serde_json::from_slice(&data)?)
}

#[context("cannot write credentials file {}", path.display())]
pub fn write_credentials(path: &Path, credentials: &Credentials)
    -> anyhow::Result<()>
{
    fs::create_dir_all(path.parent().unwrap())?;
    let tmp_path = path.with_file_name(tmp_file_name(path));
    fs::write(&tmp_path, serde_json::to_vec_pretty(&credentials)?)?;
    fs::rename(&tmp_path, path)?;
    Ok(())
}

pub fn reset_password(options: &ResetPassword) -> anyhow::Result<()> {
    let credentials_file = home_dir()?.join(".edgedb").join("credentials")
        .join(format!("{}.json", options.name));
    let (credentials, save, user) = if credentials_file.exists() {
        let creds = read_credentials(&credentials_file)?;
        let user = options.user.clone().unwrap_or_else(|| creds.user.clone());
        if options.no_save_credentials {
            (Some(creds), false, user)
        } else {
            let save = options.save_credentials || creds.user == user;
            (Some(creds), save, user)
        }
    } else {
        let user = options.user.clone().unwrap_or_else(|| "edgedb".into());
        (None, !options.no_save_credentials, user)
    };
    let password = if options.password_from_stdin {
        rpassword::read_password()?
    } else if options.password {
        loop {
            let password = rpassword::read_password_from_tty(
                Some(&format!("New password for '{}': ",
                              user.escape_default())))?;
            let confirm = rpassword::read_password_from_tty(
                Some(&format!("Confirm password for '{}': ",
                              user.escape_default())))?;
            if password != confirm {
                eprintln!("Password don't match");
            } else {
                break password;
            }
        }
    } else {
        generate_password()
    };

    let os = detect::current_os()?;
    let methods = os.get_available_methods()?.instantiate_all(&*os, true)?;
    let path = control::get_instance(&methods, &options.name)
        .and_then(|inst| inst.get_socket(true))
        .with_context(|| format!("cannot find instance {:?}", options.name))?;
    let mut conn_params = Builder::new();
    conn_params.user("edgedb");
    conn_params.database("edgedb");
    conn_params.unix_addr(path);
    task::block_on(async {
        let mut cli = conn_params.connect().await?;
        cli.execute(&format!(r###"
            ALTER ROLE {name} {{
                SET password := {password};
            }}"###,
            name=quote_name(&user),
            password=quote_string(&password))
        ).await
    })?;
    if save {
        let mut creds = credentials.unwrap_or_else(Default::default);
        creds.user = user.into();
        creds.password = Some(password);
        write_credentials(&credentials_file, &creds)?;
    }
    if !options.quiet {
        if save {
            eprintln!("Password is successfully changed and saved to \
                {}", credentials_file.display());
        } else {
            eprintln!("Password is successfully changed.");
        }
    }
    Ok(())
}

fn _b64(s: &[u8]) -> Base64Display {
    Base64Display::with_config(s, base64::STANDARD)
}

pub fn password_hash(password: &str) -> String {
    use ring::rand::SecureRandom;
    let mut salt = [0u8; SALT_LENGTH];
    ring::rand::SystemRandom::new().fill(&mut salt).expect("random bytes");
    return _build_verifier(password, &salt[..], HASH_ITERATIONS);
}

fn _build_verifier(password: &str, salt: &[u8], iterations: u32) -> String {
    use ring::hmac;
    use sha2::Sha256;
    use sha2::digest::Digest;

    let iterations = NonZeroU32::new(iterations).expect("non-zero iterations");
    let salted_password = scram::hash_password(password, iterations, salt);
    let key = hmac::Key::new(hmac::HMAC_SHA256, &salted_password[..]);
    let client_key = hmac::sign(&key, b"Client Key");
    let server_key = hmac::sign(&key, b"Server Key");
    let stored_key = Sha256::digest(client_key.as_ref());

    return format!(
        "SCRAM-SHA-256${iterations}:{salt}${stored_key}:{server_key}",
        iterations=iterations,
        salt=_b64(salt),
        stored_key=_b64(stored_key.as_ref()),
        server_key=_b64(server_key.as_ref()))
}

#[test]
fn test_verifier() {
    let salt = "W22ZaJ0SNY7soEsUEjb6gQ==";
    let raw_salt = base64::decode(salt).unwrap();
    let password = "pencil";
    let verifier = _build_verifier(password, &raw_salt, 4096);
    let stored_key = "WG5d8oPm3OtcPnkdi4Uo7BkeZkBFzpcXkuLmtbsT4qY=";
    let server_key = "wfPLwcE6nTWhTAmQ7tl2KeoiWGPlZqQxSrmfPwDl2dU=";

    assert_eq!(verifier,
        format!("SCRAM-SHA-256$4096:{salt}${stored_key}:{server_key}",
            salt=salt, stored_key=stored_key, server_key=server_key));
}
