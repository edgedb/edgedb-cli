use crate::commands::backslash;
use edgeql_parser::preparser;


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

pub fn hint(input: &str, pos: usize) -> Option<String> {
    match current(input, pos) {
        (_, Current::Empty) => None,
        (_, Current::Edgeql(_)) => None,
        (off, Current::Backslash(cmd)) => {
            //todo!();
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
