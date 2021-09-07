use async_std::prelude::StreamExt;

use prettytable::{Table, Row, Cell};

use edgedb_derive::Queryable;
use crate::commands::Options;
use crate::commands::filter;
use edgedb_client::client::Connection;
use crate::table;



#[derive(Queryable)]
struct ScalarType {
    name: String,
    extending: String,
    kind: String,
}

pub async fn list_scalar_types<'x>(cli: &mut Connection, options: &Options,
    pattern: &Option<String>, system: bool, case_sensitive: bool)
    -> Result<(), anyhow::Error>
{
    let filter = match (pattern, system) {
        (None, true) => "FILTER NOT .is_from_alias",
        (None, false) => {
            r#"FILTER NOT
                re_test("^(?:std|schema|math|sys|cfg|cal|stdgraphql)::",
                .name)"#
        }
        (Some(_), true) => {
            "FILTER NOT .is_from_alias AND re_test(<str>$0, .name)"
        }
        (Some(_), false) => {
            r#"FILTER NOT .is_from_alias
                AND re_test(<str>$0, .name) AND
                NOT re_test("^(?:std|schema|math|sys|cfg|cal|stdgraphql)::",
                .name)"#
        }
    };

    let query = &format!(r###"
        WITH MODULE schema
        SELECT ScalarType {{
            name,
            `extending` := to_str(array_agg(.bases.name), ', '),
            kind := (
                'enum' IF 'std::anyenum' IN .ancestors.name ELSE
                'sequence' IF 'std::sequence' IN .ancestors.name ELSE
                'normal'
            ),
        }}
        {filter}
        ORDER BY .name;
    "###, filter=filter);

    let mut items = filter::query::<ScalarType>(cli,
        &query, &pattern, case_sensitive).await?;
    if !options.command_line || atty::is(atty::Stream::Stdout) {
        let term_width = term_size::dimensions_stdout()
            .map(|(w, _h)| w).unwrap_or(80);
        let extending_width = (term_width-10) / 2;
        let mut table = Table::new();
        table.set_format(*table::FORMAT);
        table.set_titles(Row::new(
            ["Name", "Extending", "Kind"]
            .iter().map(|x| table::header_cell(x)).collect()));
        while let Some(item) = items.next().await.transpose()? {
            table.add_row(Row::new(vec![
                Cell::new(&item.name),
                Cell::new(&textwrap::fill(&item.extending, extending_width)),
                Cell::new(&item.kind),
            ]));
        }
        if table.is_empty() {
            if let Some(pattern) = pattern {
                eprintln!("No scalar types found matching {:?}", pattern);
            } else if !system {
                eprintln!("No user-defined scalar types found. {}",
                    if options.command_line { "Try --system" }
                    else { r"Try \lT -s" });
            }
        } else {
            table.printstd();
        }
    } else {
        while let Some(item) = items.next().await.transpose()? {
            println!("{}\t{}\t{}", item.name, item.extending, item.kind);
        }
    }
    Ok(())
}
