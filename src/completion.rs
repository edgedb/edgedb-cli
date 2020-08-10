use std::cmp::{min, Ordering};
use std::str::FromStr;

use std::ops::Bound;
use std::borrow::Borrow;

use edgeql_parser::preparser;

use crate::commands::backslash;


#[derive(Debug)]
pub enum Current<'a> {
    Edgeql(&'a str, bool),
    Backslash(&'a str),
    Empty,
}

#[derive(Debug)]
pub enum BackslashFsm {
    Command,
    Final,
    Arguments(&'static backslash::CommandInfo, &'static [backslash::Argument]),
    Setting,
    SetValue(SettingValue)
}

#[derive(Debug)]
pub enum ValidationResult {
    Valid,
    Invalid,
    Unknown,
}

#[derive(Debug)]
pub enum SettingValue {
    Variants(&'static [String]),
    Usize,
}

pub struct Pair {
    value: &'static str,
    description: &'static str,
}

pub struct Hint {
    text: String,
    complete: usize,
}

trait SetRangeStrExt<K> {
    fn range_from<'x>(&'x self, val: &str)
        -> std::collections::btree_set::Range<'x, K>;
}

trait MapRangeStrExt<K, V> {
    fn range_from<'x>(&'x self, val: &str)
        -> std::collections::btree_map::Range<'x, K, V>;
}

pub fn current<'x>(data: &'x str, pos: usize) -> (usize, Current<'x>) {
    let mut offset = 0;
    loop {
        if preparser::is_empty(&data[offset..]) {
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
                        return (offset, Current::Edgeql(
                            &data[offset..][..bytes], true));
                    }
                    offset += bytes;
                }
                Err(_) => {
                    return (offset, Current::Edgeql(&data[offset..], false));
                }
            }
        }
    }
}


fn complete_command(input: &str) -> Vec<Pair> {
    backslash::CMD_CACHE.all_commands
        .range_from(input)
        .filter(|x| x.starts_with(input))
        .map(|x| Pair { value: x, description: x })
        .collect()
}

fn complete_setting(input: &str) -> Vec<Pair> {
    backslash::CMD_CACHE.settings.range_from(input)
        .filter(|(name, _)| name.starts_with(input))
        .map(|(name, setting)| {
            Pair {
                value: name,
                description: &setting.name_description,
            }
        })
        .collect()
}

fn complete_setting_value(input: &str, val: &SettingValue) -> Vec<Pair> {
    match val {
        SettingValue::Usize => Vec::new(),
        SettingValue::Variants(v) => v.iter()
            .filter(|x| x.starts_with(input))
            .map(|x| Pair {
                value: x,
                description: x,
            }).collect(),
    }
}

pub fn complete(input: &str, cursor: usize)
    -> Option<(usize, Vec<Pair>)>
{
    match current(input, cursor) {
        (_, Current::Empty) => None,
        (_, Current::Edgeql(..)) => None,
        (off, Current::Backslash(cmd)) => {
            use backslash::Item::*;
            use BackslashFsm as Fsm;

            let cursor = cursor.saturating_sub(off);
            let mut fsm = Fsm::Command;
            for token in backslash::Parser::new(cmd) {
                if cursor < token.span.0 {
                    // TODO(tailhook) implement completion in the middle
                    return None;
                } else if cursor <= token.span.1 {
                    match (&fsm, token.item) {
                        (Fsm::Command, Command(cmd)) => {
                            return Some((token.span.0, complete_command(cmd)));
                        }
                        (Fsm::Setting, Argument(arg)) => {
                            return Some((token.span.0, complete_setting(arg)));
                        }
                        (Fsm::SetValue(cfg), Argument(arg)) => {
                            return Some((token.span.0,
                                         complete_setting_value(arg, cfg)));
                        }
                        _ => return None,
                    }
                } else {
                    fsm = fsm.advance(token);
                }
            }
            match &fsm {
                Fsm::Command => {
                    return Some((cursor, complete_command("")));
                }
                Fsm::Setting => {
                    return Some((cursor, complete_setting("")));
                }
                Fsm::SetValue(cfg) => {
                    return Some((cursor, complete_setting_value("", cfg)));
                }
                _ => return None,
            }
        }
    }
}

fn hint_command(cmd: &str) -> Option<Hint> {
    use backslash::CMD_CACHE;
    let mut rng = CMD_CACHE.all_commands.range_from(cmd)
        .take_while(|x| x.starts_with(cmd));
    if let Some(matching) = rng.next() {
        let full_match = cmd.len() == matching.len();
        if full_match || !rng.next().is_some() {
            let full_name = CMD_CACHE.aliases.get(&matching[1..]);
            let cinfo = CMD_CACHE.commands
                .get(*full_name.unwrap_or(&&matching[1..]))
                .expect("command is defined");

            let mut output = String::from(&matching[cmd.len()..]);
            let complete = output.len() + 1;
            if !cinfo.options.is_empty() {
                output.push_str(" [-");
                output.push_str(&cinfo.options);
                output.push(']');
            }
            for arg in &cinfo.arguments {
                output.push_str(" [");
                output.push_str(&arg.name.to_uppercase());
                output.push(']');
            }
            if let Some(ref descr) = cinfo.description {
                output.push_str("  -- ");
                output.push_str(descr);
            } else if let Some(full) = full_name {
                output.push_str("  -- alias of \\");
                output.push_str(full);
            }
            return Some(Hint::new(output, complete));
        } else  { // multiple choices possible
            return None
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
    let options: Vec<_> = candidates.into_iter()
        .map(|(_c, pv)| &pv[..])
        .collect();
    let text = match options.len() {
        0 => "  ← unknown backslash command".into(),
        1..=3 => format!("  ← unknown, try: {}", options.join(", ")),
        _ => {
            format!("  ← unknown, try: {}, ...", options[..2].join(", "))
        }
    };
    Some(Hint::new(text, 0))
}

fn hint_setting_name(sname: &str) -> Option<Hint> {
    use backslash::CMD_CACHE;
    let mut rng = CMD_CACHE.settings.range(sname..)
        .take_while(|(name, _)| name.starts_with(sname));

    if let Some((matching, setting)) = rng.next() {
        let full_match = sname.len() == matching.len();
        if full_match || !rng.next().is_some() {
            let mut output = String::from(&matching[sname.len()..]);
            let complete = output.len()+1;
            if let Some(ref values) = setting.values {
                output.push_str(" [");
                output.push_str(&values.join("|"));
                output.push(']');
            } else {
                output.push_str(" [");
                output.push_str(&setting.value_name.to_uppercase());
                output.push(']');
            }
            output.push_str("  -- ");
            output.push_str(&setting.description);
            return Some(Hint::new(output, complete));
        } else  { // multiple choices possible
            return None
        }
    }
    let mut candidates: Vec<(f64, &str)> = backslash::CMD_CACHE.settings
        .iter()
        .map(|(pv, _)| (strsim::jaro_winkler(sname, pv), *pv))
        .filter(|(confidence, _pv)| *confidence > 0.8)
        .collect();
    candidates.sort_by(|a, b| {
        b.0.partial_cmp(&a.0).unwrap_or(Ordering::Equal)
    });
    let options: Vec<_> = candidates.into_iter()
        .map(|(_c, pv)| &pv[..])
        .collect();
    let text = match options.len() {
        0 => "  ← unknown setting".into(),
        1..=3 => format!("  ← unknown, try: {}", options.join(", ")),
        _ => format!("  ← unknown, try: {}, ...", options[..2].join(", ")),
    };
    Some(Hint::new(text, 0))
}

fn hint_setting_value(input: &str, val: &SettingValue) -> Option<Hint> {
    match val {
        SettingValue::Usize => None,
        SettingValue::Variants(variants) => {
            let mut matches = variants.iter().filter(|v| v.starts_with(input));
            if let Some(matching) = matches.next() {
                let full_match = input.len() == matching.len();
                if full_match || !matches.next().is_some() {
                    // TODO(tailhook) describe setting
                    return Some(Hint::new(&matching[input.len()..], 1000));
                } else  { // multiple choices possible
                    return None
                }
            }
            if variants == &["on", "off"] {
                let suggest = match input {
                    "t" | "true" | "y" | "yes" | "enable" => Some("on"),
                    "f" | "false" | "n" | "no" | "disable" => Some("off"),
                    _ => None,
                };
                if let Some(suggest) = suggest {
                    return Some(Hint::new(format!(
                        "  ← unknown boolean, did you mean: {}",
                        suggest), 0));
                }
            };
            let mut candidates: Vec<(f64, &String)> = variants
                .iter()
                .map(|pv| (strsim::jaro_winkler(input, pv), pv))
                .filter(|(confidence, _pv)| *confidence > 0.8)
                .collect();
            candidates.sort_by(|a, b| {
                b.0.partial_cmp(&a.0).unwrap_or(Ordering::Equal)
            });
            let options: Vec<_> = candidates.into_iter()
                .map(|(_c, pv)| &pv[..])
                .collect();
            let text = match options.len() {
                0 => "  ← unknown value".into(),
                1..=3 => format!("  ← unknown, try: {}", options.join(", ")),
                _ => {
                    format!("  ← unknown, try: {}, ...",
                                 options[..2].join(", "))
                }
            };
            Some(Hint::new(text, 0))
        }
    }
}

pub fn hint(input: &str, pos: usize) -> Option<Hint> {

    match current(input, pos) {
        (_, Current::Empty) => None,
        (_, Current::Edgeql(..)) => None,
        (off, Current::Backslash(cmd)) => {
            use backslash::Item::*;
            use BackslashFsm as Fsm;

            let pos = pos.saturating_sub(off);
            let mut fsm = Fsm::Command;
            for token in backslash::Parser::new(cmd) {
                if pos >= token.span.0 && pos <= token.span.1 {
                    match (&fsm, token.item) {
                        (Fsm::Command, Command(cmd)) => {
                            return hint_command(cmd);
                        }
                        (Fsm::Setting, Argument(arg)) => {
                            return hint_setting_name(arg);
                        }
                        (Fsm::SetValue(cfg), Argument(arg)) => {
                            return hint_setting_value(arg, cfg);
                        }
                        _ => return None,
                    }
                } else {
                    fsm = fsm.advance(token);
                }
            }
            None
        }
    }
}

impl BackslashFsm {
    pub fn advance(self, token: backslash::Token) -> Self {
        use BackslashFsm::*;
        use backslash::Item as T;
        match self {
            Command => match token.item {
                T::Command(x) if x == "\\set" => Setting,
                T::Command(name) => {
                    let name = &name[1..];
                    let full = backslash::CMD_CACHE.aliases.get(name)
                        .unwrap_or(&name);
                    if let Some(cmd) = backslash::CMD_CACHE.commands.get(*full)
                    {
                        if cmd.arguments.is_empty() {
                            Final
                        } else {
                            Arguments(cmd, &cmd.arguments[..])
                        }
                    } else {
                        Final
                    }
                }
                _ => Final,
            }
            Final => Final,
            Arguments(cmd, args) => match token.item {
                T::Argument(x) if x.starts_with("-") => Arguments(cmd, args),
                T::Argument(_) if args.len() <= 1 => Final,
                T::Argument(_) => Arguments(cmd, &args[1..]),
                _ => Final,
            },
            Setting => match token.item {
                T::Argument(name) => {
                    match backslash::CMD_CACHE.settings.get(name) {
                        Some(setting) => {
                            if let Some(values) = &setting.values {
                                SetValue(SettingValue::Variants(&values))
                            } else {
                                // TODO(tailhook) unhardcode \limit
                                SetValue(SettingValue::Usize)
                            }
                        }
                        None => Final,
                    }
                }
                _ => Final,
            },
            SetValue(_) => Final,
        }
    }
    pub fn validate(&self, token: &backslash::Token) -> ValidationResult {
        use backslash::CMD_CACHE;
        use BackslashFsm::*;
        use backslash::Item as T;

        match (self, &token.item) {
            (Command, T::Command(cmd)) => {
                let mut rng = CMD_CACHE.all_commands.range_from(cmd)
                    .take_while(|x| x.starts_with(cmd));
                if let Some(matching) = rng.next() {
                    if cmd.len() == matching.len() {
                        ValidationResult::Valid
                    } else {
                        ValidationResult::Unknown
                    }
                } else {
                    ValidationResult::Invalid
                }
            }
            (Setting, T::Argument(arg)) => {
                let mut rng = CMD_CACHE.settings.range_from(arg)
                    .map(|(sname, _)| sname)
                    .take_while(|x| x.starts_with(arg));
                if let Some(matching) = rng.next() {
                    if arg.len() == matching.len() {
                        ValidationResult::Valid
                    } else {
                        ValidationResult::Unknown
                    }
                } else {
                    ValidationResult::Invalid
                }
            }
            (SetValue(SettingValue::Usize), T::Argument(arg)) => {
                if let Ok(_) = usize::from_str(arg) {
                    ValidationResult::Valid
                } else {
                    ValidationResult::Invalid
                }
            }
            (SetValue(SettingValue::Variants(all)), T::Argument(arg)) => {
                if let Some(val) = all.iter().find(|x| x.starts_with(arg)) {
                    if val.len() == arg.len() {
                        ValidationResult::Valid
                    } else {
                        ValidationResult::Unknown
                    }
                } else {
                    ValidationResult::Invalid
                }
            }
            (Final, _) => ValidationResult::Invalid,
            (_, _) => ValidationResult::Unknown,
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

impl Hint {
    fn new<S: Into<String>>(text: S, complete: usize) -> Hint {
        let text = text.into();
        Hint { complete: min(complete, text.len()), text }
    }
}

impl rustyline::hint::Hint for Hint {
    fn completion(&self) -> Option<&str> {
        if self.complete > 0 {
            Some(&self.text[..self.complete])
        } else {
            None
        }
    }
    fn display(&self) -> &str {
        self.text.as_ref()
    }
}

impl<K> SetRangeStrExt<K> for std::collections::BTreeSet<K>
    where K: Borrow<str> + Ord,
{
    fn range_from<'x>(&'x self, val: &str)
        -> std::collections::btree_set::Range<'x, K>
    {
        self.range::<str, _>(
            (Bound::<&str>::Included(val), Bound::<&str>::Unbounded))
    }
}

impl<K, V> MapRangeStrExt<K, V> for std::collections::BTreeMap<K, V>
    where K: Borrow<str> + Ord,
{
    fn range_from<'x>(&'x self, val: &str)
        -> std::collections::btree_map::Range<'x, K, V>
    {
        self.range::<str, _>(
            (Bound::<&str>::Included(val), Bound::<&str>::Unbounded))
    }
}
