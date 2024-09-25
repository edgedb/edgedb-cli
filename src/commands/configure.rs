use std::fmt::Display;

use crate::commands::parser::{AuthParameter, ConfigStr, ConfigStrs, Configure, ListenAddresses};
use crate::commands::Options;
use crate::connect::Connection;
use crate::print;
use edgeql_parser::helpers::{quote_name, quote_string};

async fn set(
    cli: &mut Connection,
    name: &str,
    cast: Option<&str>,
    value: impl Display,
) -> Result<(), anyhow::Error> {
    let cast = cast.unwrap_or_default();
    let query = format!("CONFIGURE INSTANCE SET {name} := {cast}{value}");
    print::completion(&cli.execute(&query, &()).await?);
    Ok(())
}

pub async fn configure(
    cli: &mut Connection,
    _options: &Options,
    cfg: &Configure,
) -> Result<(), anyhow::Error> {
    use crate::commands::parser::ConfigureCommand as C;
    use crate::commands::parser::ConfigureInsert as Ins;
    use crate::commands::parser::ConfigureReset as Res;
    use crate::commands::parser::ConfigureSet as Set;
    use crate::commands::parser::ListParameter as I;
    use crate::commands::parser::ValueParameter as S;
    match &cfg.command {
        C::Insert(Ins {
            parameter: I::Auth(param),
        }) => {
            let AuthParameter {
                users,
                comment,
                priority,
                method,
            } = param;
            let mut props = vec![
                format!("priority := {}", priority),
                format!("method := (INSERT {})", quote_name(method)),
            ];
            let users = users
                .iter()
                .map(|x| quote_string(x))
                .collect::<Vec<_>>()
                .join(", ");
            if !users.is_empty() {
                props.push(format!("user := {{ {} }}", users))
            }
            if let Some(ref comment_text) = comment {
                props.push(format!("comment := {}", quote_string(comment_text)))
            }
            print::completion(
                &cli.execute(
                    &format!(
                        r###"
                CONFIGURE INSTANCE INSERT Auth {{
                    {}
                }}
                "###,
                        props.join(",\n")
                    ),
                    &(),
                )
                .await?,
            );
            Ok(())
        }
        C::Set(Set {
            parameter: S::ListenAddresses(ListenAddresses { address }),
        }) => {
            let addresses = address
                .iter()
                .map(|x| quote_string(x))
                .collect::<Vec<_>>()
                .join(", ");
            print::completion(
                &cli.execute(
                    &format!("CONFIGURE INSTANCE SET listen_addresses := {{{addresses}}}"),
                    &(),
                )
                .await?,
            );
            Ok(())
        }
        C::Set(Set {
            parameter: S::ListenPort(param),
        }) => {
            print::completion(
                &cli.execute(
                    &format!("CONFIGURE INSTANCE SET listen_port := {}", param.port),
                    &(),
                )
                .await?,
            );
            Ok(())
        }
        C::Set(Set {
            parameter: S::SharedBuffers(ConfigStr { value }),
        }) => set(cli, "shared_buffers", Some("<cfg::memory>"), value).await,
        C::Set(Set {
            parameter: S::QueryWorkMem(ConfigStr { value }),
        }) => set(cli, "query_work_mem", Some("<cfg::memory>"), value).await,
        C::Set(Set {
            parameter: S::MaintenanceWorkMem(ConfigStr { value }),
        }) => set(cli, "maintenance_work_mem", Some("<cfg::memory>"), value).await,
        C::Set(Set {
            parameter: S::EffectiveCacheSize(ConfigStr { value }),
        }) => set(cli, "effective_cache_size", Some("<cfg::memory>"), value).await,
        C::Set(Set {
            parameter: S::DefaultStatisticsTarget(ConfigStr { value }),
        }) => set(cli, "default_statistics_target", None, value).await,
        C::Set(Set {
            parameter: S::EffectiveIoConcurrency(ConfigStr { value }),
        }) => set(cli, "effective_io_concurrency", None, value).await,
        C::Set(Set {
            parameter: S::SessionIdleTimeout(ConfigStr { value }),
        }) => {
            set(
                cli,
                "session_idle_timeout",
                Some("<duration>"),
                format!("'{value}'"),
            )
            .await
        }
        C::Set(Set {
            parameter: S::SessionIdleTransactionTimeout(ConfigStr { value }),
        }) => {
            set(
                cli,
                "session_idle_transaction_timeout",
                Some("<duration>"),
                format!("'{value}'"),
            )
            .await
        }
        C::Set(Set {
            parameter: S::QueryExecutionTimeout(ConfigStr { value }),
        }) => {
            set(
                cli,
                "query_execution_timeout",
                Some("<duration>"),
                format!("'{value}'"),
            )
            .await
        }
        C::Set(Set {
            parameter: S::AllowBareDdl(ConfigStr { value }),
        }) => set(cli, "allow_bare_ddl", None, format!("'{value}'")).await,
        C::Set(Set {
            parameter: S::ApplyAccessPolicies(ConfigStr { value }),
        }) => set(cli, "apply_access_policies", None, value).await,
        C::Set(Set {
            parameter: S::AllowUserSpecifiedId(ConfigStr { value }),
        }) => set(cli, "allow_user_specified_id", None, value).await,
        C::Set(Set {
            parameter: S::CorsAllowOrigins(ConfigStrs { values }),
        }) => {
            let values = values
                .iter()
                .map(|x| quote_string(x))
                .collect::<Vec<_>>()
                .join(", ");
            print::completion(
                &cli.execute(
                    &format!("CONFIGURE INSTANCE SET cors_allow_origins := {{{values}}}"),
                    &(),
                )
                .await?,
            );
            Ok(())
        }
        C::Set(Set {
            parameter: S::AutoRebuildQueryCache(ConfigStr { value }),
        }) => set(cli, "auto_rebuild_query_cache", None, value).await,
        C::Set(Set {
            parameter: S::AutoRebuildQueryCacheTimeout(ConfigStr { value }),
        }) => {
            set(
                cli,
                "auto_rebuild_query_cache_timeout",
                Some("<duration>"),
                format!("'{value}'"),
            )
            .await
        }
        C::Set(Set {
            parameter: S::StoreMigrationSdl(ConfigStr { value }),
        }) => set(cli, "store_migration_sdl", None, format!("'{value}'")).await,
        C::Set(Set {
            parameter: S::NetHttpMaxConnections(ConfigStr { value }),
        }) => set(cli, "net_http_max_connections", None, value).await,
        C::Reset(Res { parameter }) => {
            use crate::commands::parser::ConfigParameter as C;
            let name = match parameter {
                C::ListenAddresses => "listen_addresses",
                C::ListenPort => "listen_port",
                C::Auth => "Auth",
                C::SharedBuffers => "shared_buffers",
                C::QueryWorkMem => "query_work_mem",
                C::MaintenanceWorkMem => "maintenance_work_mem",
                C::EffectiveCacheSize => "effective_cache_size",
                C::DefaultStatisticsTarget => "default_statistics_target",
                C::EffectiveIoConcurrency => "effective_io_concurrency",
                C::SessionIdleTimeout => "session_idle_timeout",
                C::SessionIdleTransactionTimeout => "session_idle_transaction_timeout",
                C::QueryExecutionTimeout => "query_execution_timeout",
                C::AllowBareDdl => "allow_bare_ddl",
                C::ApplyAccessPolicies => "apply_access_policies",
                C::AllowUserSpecifiedId => "allow_user_specified_id",
                C::CorsAllowOrigins => "cors_allow_origins",
                C::AutoRebuildQueryCache => "auto_rebuild_query_cache",
                C::AutoRebuildQueryCacheTimeout => "auto_rebuild_query_cache_timeout",
                C::StoreMigrationSdl => "store_migration_sdl",
                C::NetHttpMaxConnections => "net_http_max_connections",
            };
            print::completion(
                &cli.execute(&format!("CONFIGURE INSTANCE RESET {name}"), &())
                    .await?,
            );
            Ok(())
        }
    }
}
