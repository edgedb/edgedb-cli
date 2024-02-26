use std::fs;
use std::path::Path;
use std::default::Default;
use std::num::NonZeroU32;

use base64::display::Base64Display;
use fn_error_context::context;
use rand::{Rng, SeedableRng};

use edgedb_tokio::credentials::Credentials;
use edgeql_parser::helpers::{quote_string, quote_name};

use crate::credentials;
use crate::commands::ExitCode;
use crate::connect::Connection;
use crate::portable::local::InstanceInfo;
use crate::portable::options::{ResetPassword, instance_arg, InstanceName};
use crate::print;
use crate::tty_password;


const PASSWORD_LENGTH: usize = 24;
const PASSWORD_CHARS: &[u8] = b"0123456789\
    abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ";
const HASH_ITERATIONS: u32 = 4096;
const SALT_LENGTH: usize = 16;

pub fn generate_password() -> String {
    let mut rng = rand::rngs::StdRng::from_entropy();
    (0..PASSWORD_LENGTH).map(|_| {
        PASSWORD_CHARS[rng.gen_range(0..PASSWORD_CHARS.len())] as char
    }).collect()
}

#[context("error reading credentials at {}", path.display())]
fn read_credentials(path: &Path) -> anyhow::Result<Credentials> {
    let data = fs::read(&path)?;
    Ok(serde_json::from_slice(&data)?)
}


pub fn reset_password(options: &ResetPassword) -> anyhow::Result<()> {
    let name = match instance_arg(&options.name, &options.instance)? {
        InstanceName::Local(name) => {
            if cfg!(windows) {
                return crate::portable::windows::reset_password(options, name);
            } else {
                name
            }
        },
        InstanceName::Cloud { .. } => {
            print::error("This operation is not supported on cloud instances yet.");
            return Err(ExitCode::new(1))?;
        },
    };
    let credentials_file = credentials::path(name)?;
    let (creds, save, user) = if credentials_file.exists() {
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
        tty_password::read_stdin()?
    } else if options.password {
        loop {
            let password = tty_password::read(
                format!("New password for '{}': ", user.escape_default()))?;
            let confirm = tty_password::read(
                format!("Confirm password for '{}': ", user.escape_default()))?;
            if password != confirm {
                print::error("Passwords do not match");
            } else {
                break password;
            }
        }
    } else {
        generate_password()
    };

    let inst = InstanceInfo::read(name)?;
    tokio::runtime::Builder::new_current_thread().enable_all().build()?
        .block_on(async {
            let conn_params = inst.admin_conn_params()?.constrained_build()?;
            let mut cli = Connection::connect(&conn_params).await?;
            cli.execute(
                &format!(r###"
                    ALTER ROLE {name} {{
                        SET password := {password};
                    }}"###,
                    name=quote_name(&user),
                    password=quote_string(&password)
                ),
                &(),
            ).await?;
            Ok::<_, anyhow::Error>(())
        })?;

    if save {
        let mut creds = creds.unwrap_or_else(Default::default);
        creds.user = user.into();
        creds.password = Some(password);
        credentials::write(&credentials_file, &creds)?;
    }
    if !options.quiet {
        if save {
            print::success_msg(
                "Password was successfully changed and saved to",
                credentials_file.display(),
            );
        } else {
            print::success("Password was successfully changed.");
        }
    }
    Ok(())
}

fn _b64(s: &[u8]) -> Base64Display<base64::engine::GeneralPurpose> {
    Base64Display::new(s, &base64::engine::general_purpose::STANDARD)
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
    use base64::Engine;
    use base64::engine::general_purpose::STANDARD;

    let salt = "W22ZaJ0SNY7soEsUEjb6gQ==";
    let raw_salt = STANDARD.decode(salt).unwrap();
    let password = "pencil";
    let verifier = _build_verifier(password, &raw_salt, 4096);
    let stored_key = "WG5d8oPm3OtcPnkdi4Uo7BkeZkBFzpcXkuLmtbsT4qY=";
    let server_key = "wfPLwcE6nTWhTAmQ7tl2KeoiWGPlZqQxSrmfPwDl2dU=";

    assert_eq!(verifier,
        format!("SCRAM-SHA-256$4096:{salt}${stored_key}:{server_key}",
            salt=salt, stored_key=stored_key, server_key=server_key));
}
