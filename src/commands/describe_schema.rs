use crate::commands::Options;
use crate::connect::Connection;
use crate::highlight;

pub async fn describe_schema(cli: &mut Connection, options: &Options) -> Result<(), anyhow::Error> {
    let text = cli
        .query_required_single::<String, ()>("DESCRIBE SCHEMA AS SDL", &())
        .await?;
    if let Some(ref styler) = options.styler {
        let mut out = String::with_capacity(text.len());
        highlight::edgeql(&mut out, &text, styler);
        println!("{out}");
    } else {
        println!("{text}");
    }
    Ok(())
}
