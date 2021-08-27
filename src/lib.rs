#[macro_use]
extern crate pest_derive;
use pest::{iterators::Pair, Parser};
use regex::Regex;
use std::{convert::TryInto, fmt::Display, str::from_utf8};
use tracing::error;

#[derive(Parser)]
#[grammar = "../assets/conf_grammar.pest"]
struct ConfParser;

#[derive(Debug)]
/// A line in the configuration file
pub struct Conf {
    /// Wether to stop is this filter matches
    stop: bool,
    envmatches: Vec<EnvMatch>,
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

impl Conf {
    fn from_rule(v: Pair<'_, Rule>) -> Result<Self, regex::Error> {
        debug_assert_eq!(v.as_rule(), Rule::rule);
        let mut conf = v.into_inner();
        let matcher = conf.next().unwrap();
        debug_assert_eq!(matcher.as_rule(), Rule::matcher);
        let mut matcher = matcher.into_inner();
        let stop = matcher
            .peek()
            .filter(|r| r.as_rule() != Rule::stop)
            .is_some();
        if !stop {
            matcher.next();
        }
        let mut envmatches = Vec::new();
        while matcher.peek().unwrap().as_rule() == Rule::env_match {
            let envmatch = EnvMatch::from_rule(matcher.next().unwrap())?;
            envmatches.push(envmatch);
        }
        let filter = matcher.next().unwrap();
        let filter = match filter.as_rule() {
            Rule::majmin => Filter::MajMin(MajMin::from_rule(filter)),
            Rule::device_regex => Filter::DeviceRegex(DeviceRegex::from_rule(filter)?),
            r => unreachable!("{:?}", r),
        };
        let user_group = UserGroup::from_rule(conf.next().unwrap());
        let mode = Mode::from_rule(conf.next().unwrap());

        let mut on_creation = None;
        let mut command = None;
        if let Some(next) = conf.next() {
            match next.as_rule() {
                Rule::on_creation => {
                    on_creation = Some(OnCreation::from_rule(next));
                    command = conf.next().map(Command::from_rule);
                }
                Rule::command => command = Some(Command::from_rule(next)),
                _ => unreachable!(),
            };
        }
        Ok(Self {
            stop,
            envmatches,
            filter,
            user_group,
            mode,
            on_creation,
            command,
        })
    }
}

impl Display for Conf {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if !self.stop {
            write!(f, "-")?;
        }
        for envmatch in &self.envmatches {
            write!(f, "{}={};", envmatch.envvar, envmatch.regex)?;
        }
        match &self.filter {
            Filter::DeviceRegex(DeviceRegex {
                regex,
                envvar: Some(var),
            }) => write!(f, "${}={}", var, regex),
            Filter::DeviceRegex(v) => write!(f, "{}", v.regex),
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
pub struct EnvMatch {
    envvar: String,
    regex: Regex,
}

impl EnvMatch {
    fn from_rule(v: Pair<'_, Rule>) -> Result<Self, regex::Error> {
        debug_assert_eq!(v.as_rule(), Rule::env_match);
        let mut envmatch = v.into_inner();
        let envvar = envvar_from_rule(envmatch.next().unwrap()).into();
        let regex = regex_from_rule(envmatch.next().unwrap())?;
        Ok(Self { envvar, regex })
    }
}

#[derive(Debug)]
/// Filter used for matching the devices
pub enum Filter {
    DeviceRegex(DeviceRegex),
    MajMin(MajMin),
}

#[derive(Debug)]
/// A regex used for matching devices based on their names
pub struct DeviceRegex {
    envvar: Option<String>,
    /// [`Regex`] used for matching
    regex: Regex,
}

impl DeviceRegex {
    fn from_rule(v: Pair<'_, Rule>) -> Result<Self, regex::Error> {
        debug_assert_eq!(v.as_rule(), Rule::device_regex);
        let mut devregex = v.into_inner();
        let envvar = devregex.next().unwrap();
        match envvar.as_rule() {
            Rule::envvar => {
                let envvar = Some(envvar_from_rule(envvar).into());
                let regex = regex_from_rule(devregex.next().unwrap())?;
                Ok(Self { envvar, regex })
            }
            Rule::regex => Ok(Self {
                envvar: None,
                regex: regex_from_rule(envvar)?,
            }),
            _ => unreachable!(),
        }
    }
}

#[derive(Debug)]
/// TODO: add docs
pub struct MajMin {
    maj: u8,
    min: u8,
    min2: Option<u8>,
}

impl MajMin {
    fn from_rule(v: Pair<'_, Rule>) -> Self {
        debug_assert_eq!(v.as_rule(), Rule::majmin);
        let mut majmin = v.into_inner();
        let maj = u8_from_rule(majmin.next().unwrap());
        let min = u8_from_rule(majmin.next().unwrap());
        let min2 = majmin.next().map(u8_from_rule);
        Self { maj, min, min2 }
    }
}

#[derive(Debug)]
/// Contains the user and group names
pub struct UserGroup {
    /// Name of the user
    user: String,
    /// Name of the group
    group: String,
}

impl UserGroup {
    fn from_rule(v: Pair<'_, Rule>) -> Self {
        debug_assert_eq!(v.as_rule(), Rule::usergroup);
        let mut usergroup = v.into_inner();
        let user = name_from_rule(usergroup.next().unwrap()).into();
        let group = name_from_rule(usergroup.next().unwrap()).into();
        Self { user, group }
    }
}

#[derive(Debug)]
/// Contains the access mode or permissions
pub struct Mode {
    /// Permissions, each value is between `b'0'` and `b'7'`
    mode: [u8; 3],
}

impl Mode {
    fn from_rule(v: Pair<'_, Rule>) -> Self {
        debug_assert_eq!(v.as_rule(), Rule::mode);
        let mode = v.as_str().as_bytes().try_into().unwrap();
        Self { mode }
    }
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

impl OnCreation {
    fn from_rule(v: Pair<'_, Rule>) -> Self {
        debug_assert_eq!(v.as_rule(), Rule::on_creation);
        let oc = v.into_inner().next().unwrap();
        match oc.as_rule() {
            Rule::move_to => Self::Move(path_from_rule(oc.into_inner().next().unwrap()).into()),
            Rule::symlink => Self::SymLink(path_from_rule(oc.into_inner().next().unwrap()).into()),
            Rule::prevent => Self::Prevent,
            _ => unreachable!(),
        }
    }
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

impl WhenToRun {
    fn from_rule(v: Pair<'_, Rule>) -> Self {
        debug_assert_eq!(v.as_rule(), Rule::when);
        let rule = v.into_inner().next().unwrap().as_rule();
        match rule {
            Rule::after => Self::After,
            Rule::before => Self::Before,
            Rule::both => Self::Both,
            _ => unreachable!(),
        }
    }
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

impl Command {
    fn from_rule(v: Pair<'_, Rule>) -> Self {
        debug_assert_eq!(v.as_rule(), Rule::command);
        let mut command = v.into_inner();
        let mut exec = command.next().unwrap().into_inner();
        let when = WhenToRun::from_rule(exec.next().unwrap());
        let path = path_from_rule(exec.next().unwrap()).into();
        let args = command.map(arg_from_rule).map(String::from).collect();
        Self { when, path, args }
    }
}

fn path_from_rule(v: Pair<'_, Rule>) -> &str {
    debug_assert_eq!(v.as_rule(), Rule::path);
    v.as_str()
}

fn arg_from_rule(v: Pair<'_, Rule>) -> &str {
    debug_assert_eq!(v.as_rule(), Rule::arg);
    v.as_str()
}

fn name_from_rule(v: Pair<'_, Rule>) -> &str {
    debug_assert_eq!(v.as_rule(), Rule::name);
    v.as_str()
}

fn envvar_from_rule(v: Pair<'_, Rule>) -> &str {
    debug_assert_eq!(v.as_rule(), Rule::envvar);
    v.as_str()
}

fn regex_from_rule(v: Pair<'_, Rule>) -> Result<Regex, regex::Error> {
    debug_assert_eq!(v.as_rule(), Rule::regex);
    Regex::new(v.as_str())
}

fn u8_from_rule(v: Pair<'_, Rule>) -> u8 {
    debug_assert_eq!(v.as_rule(), Rule::u8);
    match v.as_str().parse() {
        Ok(v) => v,
        Err(_) => unreachable!(),
    }
}

/// Parses every line of the configuration contained in `input` excluding invalid ones.
pub fn parse(input: &str) -> Vec<Conf> {
    input
        .lines()
        .map(|line| ConfParser::parse(Rule::line, line))
        .filter_map(|res| res.map_err(|err| error!("parsing error: {}", err)).ok())
        .map(|mut v| v.next().unwrap().into_inner().next().unwrap())
        .filter(|r| r.as_rule() == Rule::rule)
        .map(Conf::from_rule)
        .filter_map(|conf| conf.map_err(|err| error!("regex error: {}", err)).ok())
        .collect()
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
