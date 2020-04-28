use std::cmp::Ordering;

use edgeql_parser::preparser;

use crate::commands::backslash;


#[derive(Debug)]
pub enum Current<'a> {
    Edgeql(&'a str),
    Backslash(&'a str),
    Empty,
}

pub struct Pair {
    value: &'static str,
    description: &'static str,
}

pub fn current<'x>(data: &'x str, pos: usize) -> (usize, Current<'x>) {
    let mut offset = 0;
    loop {
        if data[offset..].trim().is_empty() {
            return (offset, Current::Empty);
        }
        if data[offset..].trim_start().starts_with('\\') {
            let bytes = backslash::full_statement(&data[offset..]);
            if offset + bytes > pos || bytes == data.len() {
                return (offset,
                        Current::Backslash(&data[offset..][..bytes]));
            }
            offset += bytes;
        } else {
            match preparser::full_statement(&data[offset..].as_bytes(), None) {
                Ok(bytes) => {
                    if offset + bytes > pos {
                        return (offset,
                                Current::Edgeql(&data[offset..][..bytes]));
                    }
                    offset += bytes;
                }
                Err(_) => return (offset, Current::Edgeql(&data[offset..])),
            }
        }
    }
}


pub fn complete(input: &str, cursor: usize)
    -> Option<(usize, Vec<Pair>)>
{
    match current(input, cursor) {
        (_, Current::Empty) => None,
        (_, Current::Edgeql(_)) => None,
        (off, Current::Backslash(cmd)) => {
            let comp = backslash::CMD_CACHE.all_commands.iter()
                .filter(|x| x.starts_with(cmd))
                .map(|x| Pair { value: x, description: x })
                .collect();
            return Some((off, comp));
        }
    }
}

fn hint_command(cmd: &str, end: bool) -> Option<String> {
    use backslash::CMD_CACHE;
    let mut rng = CMD_CACHE.all_commands.range(cmd.to_string()..);
    if let Some(matching) = rng.next() {
        if matching.starts_with(cmd) {
            let next = rng.next().map(|x| x.starts_with(cmd)).unwrap_or(false);
            let full_match = cmd.len() == matching.len();
            if full_match || !next {
                // only single match
                if end {
                    let full_name = CMD_CACHE.aliases.get(&matching[1..]);
                    let cinfo = CMD_CACHE.commands
                        .get(*full_name.unwrap_or(&&matching[1..]))
                        .expect("command is defined");

                    let mut output = String::from(&matching[cmd.len()..]);
                    if !cinfo.options.is_empty() {
                        output.push_str(" [-");
                        output.push_str(&cinfo.options);
                        output.push(']');
                    }
                    for arg in &cinfo.arguments {
                        output.push_str(" [");
                        output.push_str(&arg.to_uppercase());
                        output.push(']');
                    }
                    if let Some(ref descr) = cinfo.description {
                        output.push_str("  -- ");
                        output.push_str(descr);
                    } else if let Some(full) = full_name {
                        output.push_str("  -- alias of \\");
                        output.push_str(full);
                    }
                    return Some(output);
                } else {
                    // TODO
                }
                return None;
            } else if end { // multiple choices possible, user is still typing
                return None
            }
        }
    }
    let mut candidates: Vec<(f64, &String)> = backslash::CMD_CACHE.all_commands
        .iter()
        .map(|pv| (strsim::jaro_winkler(cmd, pv), pv))
        .filter(|(confidence, _pv)| *confidence > 0.8)
        .collect();
    candidates.sort_by(|a, b| {
        b.0.partial_cmp(&a.0).unwrap_or(Ordering::Equal)
    });
    let options: Vec<_> = candidates.into_iter().map(|(_c, pv)| &pv[..]).collect();
    match options.len() {
        0 => Some("  ← unknown backslash command".into()),
        1..=3 => Some(format!("  ← unknown, try: {}", options.join(", "))),
        _ => {
            Some(format!("  ← unknown, try: {}, ...", options[..2].join(", ")))
        }
    }
}

pub fn hint(input: &str, pos: usize) -> Option<String> {
    use backslash::Item::*;

    match current(input, pos) {
        (_, Current::Empty) => None,
        (_, Current::Edgeql(_)) => None,
        (off, Current::Backslash(cmd)) => {
            for token in backslash::Parser::new(cmd) {
                if pos >= token.span.0 && pos <= token.span.1 {
                    match token.item {
                        Command(cmd) => {
                            return hint_command(cmd, pos == input.len())
                        }
                        _ => {}  // TODO(tailhook)
                    }
                } else {
                    // TODO(tailhook) remember command
                }
            }
            None
        }
    }
}

impl rustyline::completion::Candidate for Pair {
    fn replacement(&self) -> &str {
        self.value
    }
    fn display(&self) -> &str {
        self.description
    }
}
