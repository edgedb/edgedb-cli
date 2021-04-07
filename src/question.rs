use std::borrow::Cow;
use std::io::{stdin, BufRead};

use anyhow::Context;


pub struct Numeric<'a, T: Clone + 'a> {
    question: &'a str,
    options: Vec<(Cow<'a, str>, T)>,
    suffix: &'a str,
}

pub fn read_choice() -> anyhow::Result<String> {
    for line in stdin().lock().lines() {
        let line = line.context("reading user input")?;
        return Ok(line.trim().to_lowercase())
    }
    anyhow::bail!("Unexpected end of input");
}

impl<'a, T: Clone + 'a> Numeric<'a, T> {
    pub fn new(question: &'a str) -> Self {
        Numeric {
            question,
            options: Vec::new(),
            suffix: "Your choice?",
        }
    }
    pub fn option<S: Into<Cow<'a, str>>>(&mut self, name: S, value: T)
        -> &mut Self
    {
        self.options.push((name.into(), value));
        self
    }
    pub fn ask(&self) -> anyhow::Result<T> {
        loop {
            println!("{}", self.question);
            for (idx, (title, _)) in self.options.iter().enumerate() {
                println!("{}. {}", idx+1, title);
            }
            println!("{}", self.suffix);
            let choice = match read_choice()?.parse::<u32>() {
                Ok(choice) => choice,
                Err(e) => {
                    eprintln!("Error reading choice: {}", e);
                    println!("Please enter number");
                    continue;
                }
            };
            if choice == 0 || choice as usize > self.options.len() {
                println!("Please specify a choice from the list above");
                continue;
            }
            return Ok(self.options[(choice-1) as usize].1.clone());
        }
    }
}
