use crate::commands::Options;

pub async fn print(
    items: impl IntoIterator<Item = String>,
    title: &str,
    options: &Options,
) -> Result<(), anyhow::Error> {
    if !options.command_line {
        println!("{title}:");
    }
    for name in items {
        if options.command_line {
            println!("{name}");
        } else {
            println!("  {name}");
        }
    }
    Ok(())
}
