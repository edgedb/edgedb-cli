use edgeql_parser::helpers::{quote_string, quote_name};
use crate::commands::Options;
use crate::print;
use edgedb_client::client::Connection;
use crate::commands::parser::{Configure, ConfigStr, ConfigI32};
use crate::commands::parser::{AuthParameter};


async fn set_string(cli: &mut Connection, name: &str, value: &ConfigStr)
    -> Result<(), anyhow::Error>
{
    print::completion(&cli.execute(
        &format!("CONFIGURE SYSTEM SET {} := {}",
            name, quote_string(&value.value))
    ).await?);
    Ok(())
}

async fn set_i32(cli: &mut Connection, name: &str, value: &ConfigI32)
    -> Result<(), anyhow::Error>
{
    print::completion(&cli.execute(
        &format!("CONFIGURE SYSTEM SET {} := {}", name, value.value)
    ).await?);
    Ok(())
}

pub async fn configure(cli: &mut Connection, _options: &Options,
    cfg: &Configure)
    -> Result<(), anyhow::Error>
{
    use crate::commands::parser::ConfigureCommand as C;
    use crate::commands::parser::ConfigureInsert as Ins;
    use crate::commands::parser::ConfigureReset as Res;
    use crate::commands::parser::ListParameter as I;
    use crate::commands::parser::ConfigureSet as Set;
    use crate::commands::parser::ValueParameter as S;
    match &cfg.command {
        C::Insert(Ins { parameter: I::Auth(param) }) => {
            let AuthParameter { users, comment, priority, method } = param;
            let mut props = vec![
                format!("priority := {}", priority),
                format!("method := (INSERT {})", quote_name(method)),
            ];
            let users = users.iter().map(|x| quote_string(x))
                .collect::<Vec<_>>().join(", ");
            if !users.is_empty() {
                props.push(format!("user := {{ {} }}", users))
            }
            if let Some(ref comment_text) = comment {
                props.push(format!(
                    "comment := {}", quote_string(comment_text)))
            }
            print::completion(&cli.execute(&format!(r###"
                CONFIGURE SYSTEM INSERT Auth {{
                    {}
                }}
                "###,
                props.join(",\n")
            )).await?);
            Ok(())
        }
        C::Set(Set { parameter: S::ListenAddresses(param) }) => {
            print::completion(&cli.execute(
                &format!("CONFIGURE SYSTEM SET listen_addresses := {{{}}}",
                param.address.iter().map(|x| quote_string(x))
                    .collect::<Vec<_>>().join(", "))
            ).await?);
            Ok(())
        }
        C::Set(Set { parameter: S::ListenPort(param) }) => {
            print::completion(&cli.execute(
                &format!("CONFIGURE SYSTEM SET listen_port := {}", param.port)
            ).await?);
            Ok(())
        }
        C::Set(Set { parameter: S::SharedBuffers(param) }) => {
            set_string(cli, "shared_buffers", param).await
        }
        C::Set(Set { parameter: S::QueryWorkMem(param) }) => {
            set_string(cli, "query_work_mem", param).await
        }
        C::Set(Set { parameter: S::EffectiveCacheSize(param) }) => {
            set_string(cli, "effective_cache_size", param).await
        }
        C::Set(Set { parameter: S::DefaultStatisticsTarget(param) }) => {
            set_string(cli, "default_statistics_target", param).await
        }
        C::Set(Set { parameter: S::EffectiveIoConcurrency(param) }) => {
            set_string(cli, "effective_io_concurrency", param).await
        }
        C::Set(Set { parameter: S::ClientIdleTimeout(param) }) => {
            set_i32(cli, "client_idle_timeout", param).await
        }
        C::Reset(Res { parameter }) => {
            use crate::commands::parser::ConfigParameter as C;
            let name = match parameter {
                C::ListenAddresses => "listen_addresses",
                C::ListenPort => "listen_port",
                C::Auth => "Auth",
                C::SharedBuffers => "shared_buffers",
                C::QueryWorkMem => "query_work_mem",
                C::EffectiveCacheSize => "effective_cache_size",
                C::DefaultStatisticsTarget => "default_statistics_target",
                C::EffectiveIoConcurrency => "effective_io_concurrency",
                C::ClientIdleTimeout => "client_idle_timeout",
            };
            print::completion(&cli.execute(
                &format!("CONFIGURE SYSTEM RESET {}", name)
            ).await?);
            Ok(())
        }
    }
}
