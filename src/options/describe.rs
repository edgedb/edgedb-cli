pub trait Describe {
    fn describe() -> Command;
}

pub trait DescribeEnum {
    fn subcommands() -> &'static [Subcommand];
}

#[derive(Clone, Debug)]
pub struct Command {
    pub help_title: &'static str,
    pub help: &'static str,
    pub describe_subcommands: fn() -> &'static [Subcommand],
}

#[derive(Clone, Debug)]
pub struct Subcommand {
    pub name: &'static str,
    pub override_title: Option<&'static str>,
    pub override_about: Option<&'static str>,
    pub hide: bool,
    pub expand_help: bool,
    pub describe_inner: fn() -> Command,
}

impl Command {
    pub fn subcommands(&self) -> &'static [Subcommand] {
        return (self.describe_subcommands)();
    }
}

impl Subcommand {
    pub fn describe(&self) -> Command {
        let cmd = (self.describe_inner)();
        Command {
            help: self.override_about.unwrap_or(cmd.help),
            help_title: self.override_title.unwrap_or(cmd.help_title),
            describe_subcommands: cmd.describe_subcommands,
        }
    }
}

pub fn empty_subcommands() -> &'static [Subcommand] {
    return &[];
}

pub fn empty_command() -> Command {
    return Command {
        help: "",
        help_title: "",
        describe_subcommands: empty_subcommands,
    };
}
