use std::env;

use anyhow::Context;

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
