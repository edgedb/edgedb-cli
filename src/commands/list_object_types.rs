use async_std::prelude::StreamExt;

use prettytable::{Table, Row, Cell};

use edgedb_derive::Queryable;
use crate::commands::Options;
use crate::commands::filter;
use edgedb_client::client::Connection;
use crate::table;



#[derive(Queryable)]
struct TypeRow {
    name: String,
    extending: String,
}

pub async fn list_object_types(cli: &mut Connection, options: &Options,
    pattern: &Option<String>, system: bool, case_sensitive: bool)
    -> Result<(), anyhow::Error>
{
    let mut filter = Vec::with_capacity(3);
    filter.push("NOT .is_compound_type AND NOT .is_from_alias");
    if !system {
        filter.push(r###"
            NOT re_test(
                "^(?:std|schema|math|sys|cfg|cal|stdgraphql)::",
                .name)
        "###);
    }
    if pattern.is_some() {
        filter.push("re_test(<str>$0, .name)");
    }

    let query = &format!(r###"
        WITH MODULE schema
        SELECT ObjectType {{
            name,
            `extending` := to_str(array_agg(.ancestors.name), ', '),
        }}
        FILTER ({filter})
        ORDER BY .name;
    "###, filter=filter.join(") AND ("));

    let mut items = filter::query::<TypeRow>(cli,
        &query, pattern, case_sensitive).await?;
    if !options.command_line || atty::is(atty::Stream::Stdout) {
        let term_width = term_size::dimensions_stdout()
            .map(|(w, _h)| w).unwrap_or(80);
        let extending_width = (term_width-7) * 3 / 4;
        let mut table = Table::new();
        table.set_format(*table::FORMAT);
        table.set_titles(Row::new(
            ["Name", "Extending"]
            .iter().map(|x| table::header_cell(x)).collect()));
        while let Some(item) = items.next().await.transpose()? {
            table.add_row(Row::new(vec![
                Cell::new(&item.name),
                Cell::new(&textwrap::fill(&item.extending, extending_width)),
            ]));
        }
        if table.is_empty() {
            if let Some(pattern) = pattern {
                eprintln!("No object types found matching {:?}", pattern);
            } else if !system {
                eprintln!("No user-defined object types found. {}",
                    if options.command_line { "Try --system" }
                    else { r"Try \lt -s" });
            }
        } else {
            table.printstd();
        }
    } else {
        while let Some(item) = items.next().await.transpose()? {
            println!("{}\t{}", item.name, item.extending);
        }
    }
    Ok(())
}
