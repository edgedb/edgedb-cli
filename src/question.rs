use std::borrow::Cow;
use std::io::{stdin, BufRead};

use rustyline::{Editor, Config};
use colorful::{Colorful, Color};

use anyhow::Context;


pub struct Numeric<'a, T: Clone + 'a> {
    question: Cow<'a, str>,
    options: Vec<(Cow<'a, str>, T)>,
    suffix: &'a str,
}

pub struct String<'a> {
    question: &'a str,
    default: &'a str,
    initial: Option<std::string::String>,
}

pub struct Confirm<'a> {
    question: Cow<'a, str>,
    is_dangerous: bool,
    default: Option<bool>,
}

pub struct Variant<'a, T: 'a> {
    result: T,
    input: &'a [&'a str],
    help: Cow<'a, str>,
}

pub struct Choice<'a, T: 'a> {
    question: Cow<'a, str>,
    choices: Vec<Variant<'a, T>>,
}

pub fn read_choice() -> anyhow::Result<std::string::String> {
    for line in stdin().lock().lines() {
        let line = line.context("reading user input")?;
        return Ok(line.trim().to_lowercase())
    }
    anyhow::bail!("Unexpected end of input");
}

fn print_prompt(line: &str) {
    println!(
        "{}",
        line.bold().color(Color::Orange3)
    );
}

fn print_error(line: &str) {
    eprintln!(
        "{}",
        line.bold().light_red()
    );
}

impl<'a, T: Clone + 'a> Numeric<'a, T> {
    pub fn new<Q: Into<Cow<'a, str>>>(question: Q) -> Self {
        Numeric {
            question: question.into(),
            options: Vec::new(),
            suffix: "Type a number to select an option:",
        }
    }
    pub fn option<S: Into<Cow<'a, str>>>(&mut self, name: S, value: T)
        -> &mut Self
    {
        self.options.push((name.into(), value));
        self
    }
    pub fn is_empty(&self) -> bool {
        self.options.is_empty()
    }
    pub fn ask(&self) -> anyhow::Result<T> {
        let mut editor = Editor::<()>::with_config(Config::builder().build());
        loop {
            print_prompt(&self.question);
            for (idx, (title, _)) in self.options.iter().enumerate() {
                print_prompt(
                    &format!("{}. {}", idx+1, title)
                );
            }
            print_prompt(&self.suffix);
            let value = editor.readline("> ")?;
            let choice = match value.parse::<u32>() {
                Ok(choice) => choice,
                Err(e) => {
                    print_error(
                        &format!("Error reading choice: {}", e)
                    );
                    print_prompt("Please enter a number");
                    continue;
                }
            };
            if choice == 0 || choice as usize > self.options.len() {
                print_error("Please specify a choice from the list above");
                continue;
            }
            return Ok(self.options[(choice-1) as usize].1.clone());
        }
    }
}

impl<'a> String<'a> {
    pub fn new(question: &'a str) -> String {
        String {
            question,
            default: "",
            initial: None,
        }
    }
    pub fn default(&mut self, default: &'a str) -> &mut Self {
        self.default = default;
        self
    }
    pub fn ask(&mut self) -> anyhow::Result<std::string::String> {
        if self.default.is_empty() {
            print_prompt(
                &format!("{}: ", self.question)
            );
        } else {
            print_prompt(
                &format!("{} [default: {}]: ", self.question, self.default)
            );
        }
        let mut editor = Editor::<()>::with_config(Config::builder().build());
        let initial = self.initial.as_ref().map(|s| &s[..])
            .unwrap_or(self.default);
        let mut val = editor.readline_with_initial(
            "> ",
            (initial, ""),
        )?;
        if val == "" {
            val = self.default.to_string();
        }
        self.initial = Some(val.clone());
        return Ok(val);
    }
}

impl<'a> Confirm<'a> {
    pub fn new<Q: Into<Cow<'a, str>>>(question: Q) -> Confirm<'a> {
        Confirm {
            question: question.into(),
            is_dangerous: false,
            default: None,
        }
    }
    pub fn new_dangerous<Q: Into<Cow<'a, str>>>(question: Q) -> Confirm<'a> {
        Confirm {
            question: question.into(),
            is_dangerous: true,
            default: None,
        }
    }
    pub fn default(&mut self, value: bool) -> &mut Self {
        self.default = Some(value);
        self
    }
    pub fn ask(&self) -> anyhow::Result<bool> {
        let mut editor = Editor::<()>::with_config(Config::builder().build());
        if self.is_dangerous {
            print_prompt(
                &format!("{} (type `Yes`)", self.question)
            );
        } else {
            print_prompt(
                &format!(
                    "{} [{}]", self.question, match self.default {
                        None => "y/n",
                        Some(true) => "Y/n",
                        Some(false) => "y/N",
                })
            );
        };
        let mut initial = match self.default {
            None => "",
            Some(true) => "Y",
            Some(false) => "N",
        }.to_string();
        loop {
            let val = editor.readline_with_initial("> ", (&initial, ""))?;
            if self.is_dangerous {
                match val.as_ref() {
                    "Yes" => return Ok(true),
                    _ => return Ok(false),
                }
            } else {
                match val.as_ref() {
                    "y" | "Y" | "yes" | "Yes" | "YES" => return Ok(true),
                    "n" | "N" | "no" | "No" | "NO" => return Ok(false),
                    "" if self.default.is_some() => {
                        return Ok(self.default.unwrap());
                    }
                    _ => {
                        initial = val;
                        print_error("Please answer Y or N");
                        continue;
                    }
                }
            }
        }
    }
}

impl<'a, T: Clone + 'a> Choice<'a, T> {
    pub fn new<Q: Into<Cow<'a, str>>>(question: Q) -> Self {
        Choice {
            question: question.into(),
            choices: Vec::new(),
        }
    }
    pub fn option<H: Into<Cow<'a, str>>>(&mut self,
        result: T, input: &'a [&'a str], help: H)
        -> &mut Self
    {
        self.choices.push(Variant { result, input, help: help.into() });
        self
    }
    pub fn ask(&self) -> anyhow::Result<T> {
        let mut editor = Editor::<()>::with_config(Config::builder().build());
        let options = self.choices.iter()
            .map(|c| c.input[0])
            .chain(Some("?"))
            .collect::<Vec<_>>()
            .join(",");
        loop {
            print_prompt(
                &format!("{} [{}]", self.question, options)
            );
            let val = editor.readline("> ")?;
            if matches!(val.as_ref(), "?" | "h" | "help") {
                for choice in &self.choices {
                    println!("{} - {}", choice.input.join(" or "), choice.help)
                }
                println!("h or ? - print help");
                continue;
            }
            for choice in &self.choices {
                for item in choice.input {
                    if item == &val {
                        return Ok(choice.result.clone());
                    }
                }
            }
            print_error(
                &format!("Invalid option {:?}, please use one of: [{}]",
                         val, options)
            );
        }
    }
}
