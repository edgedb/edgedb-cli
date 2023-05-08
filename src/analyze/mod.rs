use std::env;
use std::path::Path;
use std::borrow::Cow;

use anyhow::Context;
use tokio::fs;
use tokio::io::{self, AsyncWriteExt};

use crate::classify;
use crate::platform::tmp_file_path;
use crate::commands::parser::Analyze;
use crate::connect::Connection;
use crate::repl::{self, LastAnalyze};

mod model;
mod tree;

pub use model::Analysis;


pub async fn interactive(prompt: &mut repl::State, query: &str)
    -> anyhow::Result<()>
{
    let cli = prompt.connection.as_mut()
        .expect("connection established");
    let data = cli.query_required_single::<String, _>(query, &()).await?;

    if env::var_os("_EDGEDB_ANALYZE_DEBUG_JSON")
        .map(|x| !x.is_empty()).unwrap_or(false)
    {
        let json: serde_json::Value = serde_json::from_str(&data).unwrap();
        println!("JSON: {}", json);
    }
    let jd = &mut serde_json::Deserializer::from_str(&data);
    let output: Analysis = serde_path_to_error::deserialize(jd)
        .context("parsing explain output")?;

    let analyze = prompt.last_analyze.insert(LastAnalyze {
        query: query.to_owned(),
        output,
    });
    render_explain(&analyze.output)?;
    Ok(())
}

pub async fn render_expanded_explain(data: &Analysis) -> anyhow::Result<()>
{
    tree::print_tree(data);
    Ok(())
}

pub fn render_explain(explain: &Analysis) -> anyhow::Result<()>
{
    tree::print_contexts(explain);
    if env::var_os("_EDGEDB_ANALYZE_DEBUG_PLAN")
        .map(|x| !x.is_empty()).unwrap_or(false)
    {
        tree::print_debug_plan(explain);
    }
    tree::print_shape(explain);
    Ok(())
}

#[fn_error_context::context("cannot lookup path {:?}", path)]
pub async fn is_special(path: &Path) -> anyhow::Result<bool> {
    match fs::metadata(path).await {
        Ok(meta) => Ok(!meta.is_file()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e)?,
    }
}

pub async fn command(cli: &mut Connection, options: &Analyze)
    -> anyhow::Result<()>
{
    let data = if let Some(json_path) = &options.read_json {
        fs::read_to_string(&json_path).await
            .with_context(|| format!("cannot read {json_path:?}"))?

    } else {
        let Some(inner_query) = &options.query else {
            anyhow::bail!("Query argument is required");
        };
        let query = if classify::is_analyze(inner_query) {
            // allow specifying options in the query itself
            Cow::Borrowed(inner_query)
        } else {
            // but also do not make user writing `analyze` twice
            Cow::Owned(format!("analyze {inner_query}"))
        };

        cli.query_required_single::<String, _>(&query, &()).await?
    };
    if let Some(out_path) = &options.debug_output_file {
        if out_path == Path::new("-") {
            let mut out = io::stdout();
            out.write_all(data.as_bytes()).await?;
            out.flush().await?;
        } else if is_special(&out_path).await? {
            async {
                let mut out = fs::File::create(&out_path).await?;
                out.write_all(data.as_bytes()).await?;
                out.flush().await
            }.await.with_context(|| format!("error writing to {out_path:?}"))?;
        } else {
            let tmp = tmp_file_path(&out_path);
            async {
                let mut out = fs::File::create(&tmp).await?;
                out.write_all(data.as_bytes()).await?;
                out.flush().await
            }.await.with_context(|| format!("error writing to {tmp:?}"))?;
            fs::rename(&tmp, &out_path)
                .await.with_context(|| format!(
                    "rename error {tmp:?} -> {out_path:?}"
                ))?;
        }
    } else {
        let jd = &mut serde_json::Deserializer::from_str(&data);
        let output: Analysis = serde_path_to_error::deserialize(jd)
            .with_context(|| format!("parsing explain output"))?;

        render_explain(&output)?;
        if options.expand {
            render_expanded_explain(&output).await?;
        }
    }
    Ok(())
}
