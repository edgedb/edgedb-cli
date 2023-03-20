use std::env;

use anyhow::Context;

use crate::repl;


mod model;
mod tree;


pub async fn interactive(prompt: &mut repl::State, query: &str)
    -> anyhow::Result<()>
{
    let cli = prompt.connection.as_mut()
        .expect("connection established");
    let data = cli.query_required_single::<String, _>(query, &()).await?;
    render_explain(query, &data).await
}

pub async fn render_explain(_query: &str, data: &str) -> anyhow::Result<()>
{
    let json: serde_json::Value = serde_json::from_str(&data).unwrap();
    println!("JSON: {}", json);
    let jd = &mut serde_json::Deserializer::from_str(&data);
    let explain: model::Explain = serde_path_to_error::deserialize(jd)
        .context("parsing explain output")?;
    tree::print_contexts(&explain);
    if env::var_os("_EDGEDB_ANALYZE_DEBUG_PLAN")
        .map(|x| !x.is_empty()).unwrap_or(false)
    {
        tree::print_debug_plan(&explain);
    }
    tree::print_shape(&explain);
    tree::print_tree(&explain);
    Ok(())
}
