#[macro_use]
extern crate pest_derive;
use pest::{iterators::Pair, Parser};

use std::{
    convert::{TryFrom, TryInto},
    error::Error as StdError,
    fmt::Display,
    str::from_utf8,
};

use regex::Regex;

use tracing::{error, field::debug};

#[derive(Debug)]
/// Enumeration of all the errors that can happen while parsing a configuration line
pub enum ParsingError {
    /// The line is empty
    Empty,
    /// The line is a comment
    Comment,
    /// The [`DeviceRegex`] is invalid
    DeviceRegex(String),
    /// The [`MajMin`] is invalid
    MajMin(String),
    /// The [`Mode`] is invalid
    Mode(String),
    /// The [`EnvRegex`] is invalid
    EnvRegex(String),
    /// The [`UserGroup`] is invalid
    UserGroup(String),
    /// The [`OnCreation`] instruction is invalid
    OnCreation(String),
    /// The [`Command`] is invalid
    Command(String),
}

impl Display for ParsingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => write!(f, "empty line"),
            Self::Comment => write!(f, "comment line"),
            Self::DeviceRegex(err) => write!(f, "devicename regex error: {}", err),
            Self::MajMin(err) => write!(f, "version error: {}", err),
            Self::Mode(err) => write!(f, "mode error: {}", err),
            Self::EnvRegex(err) => write!(f, "env var regex error: {}", err),
            Self::UserGroup(err) => write!(f, "user and/or group error: {}", err),
            Self::OnCreation(err) => write!(f, "on creation instruction error: {}", err),
            Self::Command(err) => write!(f, "command error: {}", err),
        }
    }
}

impl StdError for ParsingError {}

type Error = ParsingError;

#[derive(Parser)]
#[grammar = "../assets/conf_grammar.pest"]
struct ConfParser;

#[derive(Debug)]
/// A line in the configuration file
pub struct Conf {
    /// Wether to stop is this filter matches
    stop: bool,
    /// Filter used to match the devices
    filter: Filter,
    /// User and gruop that will own the device
    user_group: UserGroup,
    /// Permissions that the specified user and group have on the device
    mode: Mode,
    /// What to do with the device node, if [`None`] it gets placed in `/dev/` with its
    /// original name
    on_creation: Option<OnCreation>,
    /// Additional command that has to be executed when creating and/or removing the node
    command: Option<Command>,
}

impl Display for Conf {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if !self.stop {
            write!(f, "-")?;
        }
        match &self.filter {
            Filter::DeviceRegex(v) => write!(f, "{}", v.regex),
            Filter::EnvRegex(v) => write!(f, "${}={}", v.var, v.regex),
            Filter::MajMin(MajMin {
                maj,
                min,
                min2: Some(min2),
            }) => write!(f, "@{},{}-{}", maj, min, min2),
            Filter::MajMin(v) => write!(f, "@{},{}", v.maj, v.min),
        }?;
        write!(
            f,
            " {}:{} {}",
            self.user_group.user,
            self.user_group.group,
            from_utf8(&self.mode.mode).unwrap()
        )?;
        if let Some(on_creation) = &self.on_creation {
            match on_creation {
                OnCreation::Move(p) => write!(f, " ={}", p),
                OnCreation::SymLink(p) => write!(f, " >{}", p),
                OnCreation::Prevent => write!(f, " !"),
            }?;
        }
        if let Some(command) = &self.command {
            match command.when {
                WhenToRun::After => write!(f, " @"),
                WhenToRun::Before => write!(f, " $"),
                WhenToRun::Both => write!(f, " *"),
            }?;
            write!(f, "{}", command.path)?;
            for arg in &command.args {
                write!(f, " {}", arg)?;
            }
        }
        Ok(())
    }
}

#[derive(Debug)]
/// Filter used for matching the devices
pub enum Filter {
    DeviceRegex(DeviceRegex),
    EnvRegex(EnvRegex),
    MajMin(MajMin),
}

#[derive(Debug)]
/// A regex used for matching devices based on their names
pub struct DeviceRegex {
    /// [`Regex`] used for matching
    regex: Regex,
}

#[derive(Debug)]
/// TODO: add docs
pub struct EnvRegex {
    var: String,
    regex: Regex,
}

#[derive(Debug)]
/// TODO: add docs
pub struct MajMin {
    maj: u8,
    min: u8,
    min2: Option<u8>,
}

#[derive(Debug)]
/// Contains the user and group names
pub struct UserGroup {
    /// Name of the user
    user: String,
    /// Name of the group
    group: String,
}

#[derive(Debug)]
/// Contains the access mode or permissions
pub struct Mode {
    /// Permissions, each value is between `b'0'` and `b'7'`
    mode: [u8; 3],
}

#[derive(Debug)]
/// Additional actions to take on creation of the device node
pub enum OnCreation {
    /// Moves/renames the device. If the path ends with `/` then the name will be stay the same
    Move(String),
    /// Same as [`OnCreation::Move`] but also creates a symlink in `/dev/` to the
    /// renamed/moved device
    SymLink(String),
    /// Prevents the creation of the device node
    Prevent,
}

#[derive(Debug)]
/// When to run the [`Command`]
pub enum WhenToRun {
    /// After creating the device
    After,
    /// Before removing te device
    Before,
    /// Both after the creation and before removing
    Both,
}

#[derive(Debug)]
pub struct Command {
    /// When to run the command
    when: WhenToRun,
    /// Path to the executable
    path: String,
    /// Command line arguments
    args: Vec<String>,
}

/// Parses every line of the configuration contained in `input` excluding invalid ones.
pub fn parse(input: &str) -> Vec<Conf> {
    Vec::new()
}

fn contains_whitespaces(s: &str) -> bool {
    s.chars().any(char::is_whitespace)
}

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        let input = include_str!("../assets/test.conf");
        for conf in super::parse(input) {
            println!("{}", conf);
        }
    }
}
