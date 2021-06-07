use edgeql_parser::helpers::{quote_string, quote_name};
use crate::commands::Options;
use edgedb_client::client::Connection;
use crate::options::{RoleParams};
use crate::print;


fn process_params(options: &RoleParams) -> Result<Vec<String>, anyhow::Error> {
    let mut result = Vec::new();
    if options.set_password || options.set_password_from_stdin {
        let password = if options.set_password_from_stdin {
            rpassword::read_password()?
        } else {
            loop {
                let password = rpassword::read_password_from_tty(
                    Some(&format!("New password for '{}': ",
                                  options.role.escape_default())))?;
                let confirm = rpassword::read_password_from_tty(
                    Some(&format!("Confirm password for '{}': ",
                                  options.role.escape_default())))?;
                if password != confirm {
                    eprintln!("Password don't match");
                } else {
                    break password;
                }
            }
        };
        result.push(format!("SET password := {}", quote_string(&password)));
    }
    Ok(result)
}

pub async fn create_superuser(cli: &mut Connection, _options: &Options,
    role: &RoleParams)
    -> Result<(), anyhow::Error>
{
    let params = process_params(role)?;
    if params.is_empty() {
        print::completion(&cli.execute(
            &format!("CREATE SUPERUSER ROLE {}", quote_name(&role.role))
        ).await?);
    } else {
        print::completion(&cli.execute(
            &format!(r###"
                CREATE SUPERUSER ROLE {name} {{
                    {params}
                }}"###,
                name=quote_name(&role.role),
                params=params.join(";\n"))
        ).await?);
    }
    Ok(())
}

pub async fn alter(cli: &mut Connection, _options: &Options,
    role: &RoleParams)
    -> Result<(), anyhow::Error>
{
    let params = process_params(role)?;
    if params.is_empty() {
        return Err(anyhow::anyhow!("Please specify attribute to alter"));
    } else {
        print::completion(&cli.execute(
            &format!(r###"
                ALTER ROLE {name} {{
                    {params}
                }}"###,
                name=quote_name(&role.role),
                params=params.join(";\n"))
        ).await?);
    }
    Ok(())
}

pub async fn drop(cli: &mut Connection, _options: &Options,
    name: &str)
    -> Result<(), anyhow::Error>
{
    print::completion(&cli.execute(
        &format!("DROP ROLE {}", quote_name(name))
    ).await?);
    Ok(())
}
