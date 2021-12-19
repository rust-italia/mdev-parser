#[macro_use]
extern crate pest_derive;
use pest::{iterators::Pair, Parser};
use regex::Regex;
use std::iter::once;
use std::path::PathBuf;
use std::{fmt::Display, num::ParseIntError};
use tracing::error;

#[derive(Parser)]
#[grammar = "../assets/conf_grammar.pest"]
struct ConfParser;

#[derive(Debug, PartialEq)]
/// A line in the configuration file
pub struct Conf {
    /// Whether to stop is this filter matches
    pub stop: bool,
    pub envmatches: Vec<EnvMatch>,
    /// Filter used to match the devices
    pub filter: Filter,
    /// User that will own the device
    pub user: String,
    /// Group that will own the device
    pub group: String,
    /// Permissions that the specified user and group have on the device
    pub mode: u32,
    /// What to do with the device node, if [`None`] it gets placed in `/dev/` with its
    /// original name
    pub on_creation: Option<OnCreation>,
    /// Additional command that has to be executed when creating and/or removing the node
    pub command: Option<Command>,
}

impl Conf {
    fn from_rule(v: Pair<'_, Rule>) -> anyhow::Result<Self> {
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
            Rule::majmin => Filter::MajMin(MajMin::from_rule(filter)?),
            Rule::device_regex => Filter::DeviceRegex(DeviceRegex::from_rule(filter)?),
            _ => unreachable!(),
        };
        let (user, group) = user_group_from_rule(conf.next().unwrap());
        let mode = mode_from_rule(conf.next().unwrap());

        let (on_creation, command) = match conf.next() {
            Some(next) if next.as_rule() == Rule::on_creation => (
                Some(OnCreation::from_rule(next)),
                conf.next().map(Command::from_rule),
            ),
            Some(next) if next.as_rule() == Rule::command => (None, Some(Command::from_rule(next))),
            None => (None, None),
            _ => unreachable!(),
        };
        Ok(Self {
            stop,
            envmatches,
            filter,
            user,
            group,
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
        write!(f, " {}:{} {:03o}", self.user, self.group, self.mode,)?;
        if let Some(on_creation) = &self.on_creation {
            match on_creation {
                OnCreation::Move(p) => write!(f, " ={}", p.display()),
                OnCreation::SymLink(p) => write!(f, " >{}", p.display()),
                OnCreation::Prevent => write!(f, " !"),
            }?;
        }
        if let Some(command) = &self.command {
            let when = match command.when {
                WhenToRun::After => '@',
                WhenToRun::Before => '$',
                WhenToRun::Both => '*',
            };
            write!(f, " {}{}", when, command.path)?;
            for arg in &command.args {
                write!(f, " {}", arg)?;
            }
        }
        Ok(())
    }
}

impl Default for Conf {
    fn default() -> Self {
        let filter = Filter::DeviceRegex(DeviceRegex {
            envvar: None,
            regex: Regex::new(".*").unwrap(),
        });
        Conf {
            stop: false,
            envmatches: vec![],
            filter,
            user: "root".to_string(),
            group: "root".to_string(),
            mode: 0o660,
            on_creation: None,
            command: None,
        }
    }
}

#[derive(Debug)]
pub struct EnvMatch {
    pub envvar: String,
    pub regex: Regex,
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

impl PartialEq for EnvMatch {
    fn eq(&self, other: &Self) -> bool {
        self.envvar == other.envvar && self.regex.as_str() == other.regex.as_str()
    }
}

#[derive(Debug, PartialEq)]
/// Filter used for matching the devices
pub enum Filter {
    DeviceRegex(DeviceRegex),
    MajMin(MajMin),
}

impl From<DeviceRegex> for Filter {
    fn from(v: DeviceRegex) -> Self {
        Self::DeviceRegex(v)
    }
}

impl From<MajMin> for Filter {
    fn from(v: MajMin) -> Self {
        Self::MajMin(v)
    }
}

#[derive(Debug)]
/// A regex used for matching devices based on their names
pub struct DeviceRegex {
    pub envvar: Option<String>,
    /// [`Regex`] used for matching
    pub regex: Regex,
}

impl DeviceRegex {
    fn from_rule(v: Pair<'_, Rule>) -> Result<Self, regex::Error> {
        debug_assert_eq!(v.as_rule(), Rule::device_regex);
        let mut devregex = v.into_inner();
        let envvar = devregex.next().unwrap();
        let (envvar, regex) = match envvar.as_rule() {
            Rule::envvar => (
                Some(envvar_from_rule(envvar).into()),
                regex_from_rule(devregex.next().unwrap())?,
            ),
            Rule::regex => (None, regex_from_rule(envvar)?),
            _ => unreachable!(),
        };
        Ok(Self { envvar, regex })
    }
}

impl PartialEq for DeviceRegex {
    fn eq(&self, other: &Self) -> bool {
        self.envvar == other.envvar && self.regex.as_str() == other.regex.as_str()
    }
}

#[derive(Debug, PartialEq)]
/// TODO: add docs
pub struct MajMin {
    pub maj: u32,
    pub min: u32,
    pub min2: Option<u32>,
}

impl MajMin {
    fn from_rule(v: Pair<'_, Rule>) -> anyhow::Result<Self> {
        debug_assert_eq!(v.as_rule(), Rule::majmin);
        let mut majmin = v.into_inner();
        let maj = u32_from_rule(majmin.next().unwrap())?;
        let min = u32_from_rule(majmin.next().unwrap())?;
        let min2 = majmin.next().map(u32_from_rule).transpose()?;
        Ok(Self { maj, min, min2 })
    }
}

#[derive(Clone, Debug, PartialEq)]
/// Additional actions to take on creation of the device node
pub enum OnCreation {
    /// Moves/renames the device. If the path ends with `/` then the name will be stay the same
    Move(PathBuf),
    /// Same as [`OnCreation::Move`] but also creates a symlink in `/dev/` to the
    /// renamed/moved device
    SymLink(PathBuf),
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

#[derive(Debug, PartialEq)]
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

#[derive(Debug, PartialEq)]
pub struct Command {
    /// When to run the command
    pub when: WhenToRun,
    /// Path to the executable
    pub path: String,
    /// Command line arguments
    pub args: Vec<String>,
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

fn u32_from_rule(v: Pair<'_, Rule>) -> Result<u32, ParseIntError> {
    debug_assert_eq!(v.as_rule(), Rule::number);
    v.as_str().parse()
}

fn user_group_from_rule(v: Pair<'_, Rule>) -> (String, String) {
    debug_assert_eq!(v.as_rule(), Rule::usergroup);
    let mut usergroup = v.into_inner();
    let user = name_from_rule(usergroup.next().unwrap()).into();
    let group = name_from_rule(usergroup.next().unwrap()).into();
    (user, group)
}

fn mode_from_rule(v: Pair<'_, Rule>) -> u32 {
    debug_assert_eq!(v.as_rule(), Rule::mode);
    u32::from_str_radix(v.as_str(), 8).unwrap()
}

/// Parses every line of the configuration contained in `input` excluding invalid ones.
pub fn parse(input: &str) -> Vec<Conf> {
    let filter_map = |line| {
        let mut v = ConfParser::parse(Rule::line, line)
            .map_err(|err| error!("parsing error: {}", err))
            .ok()?;
        let rule = Some(v.next().unwrap().into_inner().next().unwrap())
            .filter(|r| r.as_rule() == Rule::rule)?;
        Conf::from_rule(rule)
            .map_err(|err| error!("regex error: {}", err))
            .ok()
    };
    input
        .lines()
        .filter_map(filter_map)
        .chain(once(Conf::default()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    macro_rules! in_out_test {
        ($($in:literal <===> $out:expr),* $(,)?) => {
            const INPUT: &str = concat!($($in, "\n",)*);

            fn outs() -> Vec<Conf> {
                vec![$($out),*]
            }
        };
    }

    fn common_case(r: &str) -> Conf {
        Conf {
            stop: true,
            envmatches: vec![],
            filter: DeviceRegex {
                envvar: None,
                regex: regex(r),
            }
            .into(),
            user: "root".into(),
            group: "root".into(),
            mode: 0o660,
            on_creation: None,
            command: None,
        }
    }

    fn regex(s: &str) -> Regex {
        Regex::new(s).unwrap()
    }

    in_out_test! {
        "SYSTEM=usb;DEVTYPE=usb_device;.*\troot:root\t660  */opt/dev-bus-usb" <===> Conf {
            envmatches: vec![
                EnvMatch { envvar: "SYSTEM".into(), regex: regex("usb") },
                EnvMatch { envvar: "DEVTYPE".into(), regex: regex("usb_device") },
            ],
            command: Command {
                when: WhenToRun::Both,
                path: "/opt/dev-bus-usb".into(),
                args: vec![],
            }.into(),
            ..common_case(".*")
        },
        "$MODALIAS=.*\troot:root\t660 @modprobe -b \"$MODALIAS\" " <===> Conf {
            filter: DeviceRegex {
                envvar: Some("MODALIAS".into()),
                regex: regex(".*"),
            }.into(),
            command: Command {
                when: WhenToRun::After,
                path: "modprobe".into(),
                args: vec!["-b".into(), "\"$MODALIAS\"".into()],
            }.into(),
            ..common_case(".*")
        },
        "@42,17-125 root:root 660" <===> Conf {
            filter: MajMin { maj: 42, min: 17, min2: Some(125) }.into(),
            ..common_case(".*")
        },
        "@42,17     root:root 660" <===> Conf {
            filter: MajMin { maj: 42, min: 17, min2: None }.into(),
            ..common_case(".*")
        },
        "loop([0-9]+)\troot:disk 660\t>loop/%1" <===> Conf {
            user: "root".into(), group: "disk".into(),
            on_creation: OnCreation::SymLink("loop/%1".into()).into(),
            ..common_case("loop([0-9]+)")
        },
        "SUBSYSTEM=usb;DEVTYPE=usb_device;.* root:root 660 */opt/mdev/helpers/dev-bus-usb" <===> Conf {
            envmatches: vec![
                EnvMatch { envvar: "SUBSYSTEM".into(), regex: regex("usb"), },
                EnvMatch { envvar: "DEVTYPE".into(), regex: regex("usb_device"), },
            ],
            command: Command {
                when: WhenToRun::Both,
                path: "/opt/mdev/helpers/dev-bus-usb".into(),
                args: vec![],
            }.into(),
            ..common_case(".*")
        },
        "-SUBSYSTEM=net;DEVPATH=.*/net/.*;.*\troot:root 600 @/opt/mdev/helpers/settle-nics --write-mactab" <===> Conf {
            stop: false,
            envmatches: vec![
                EnvMatch { envvar: "SUBSYSTEM".into(), regex: regex("net"), },
                EnvMatch { envvar: "DEVPATH".into(), regex: regex(".*/net/.*"), },
            ],
            mode: 0o600,
            command: Command {
                when: WhenToRun::After,
                path: "/opt/mdev/helpers/settle-nics".into(),
                args: vec!["--write-mactab".into()],
            }.into(),
            ..common_case(".*")
        },
        "SUBSYSTEM=sound;.*  root:audio 660 @/opt/mdev/helpers/sound-control" <===> Conf {
            envmatches: vec![EnvMatch { envvar: "SUBSYSTEM".into(), regex: regex("sound"), }],
            user: "root".into(), group: "audio".into(),
            command: Command {
                when: WhenToRun::After,
                path: "/opt/mdev/helpers/sound-control".into(),
                args: vec![],
            }.into(),
            ..common_case(".*")
        },
        "cpu([0-9]+)\troot:root 600\t=cpu/%1/cpuid" <===> Conf {
            mode: 0o600,
            on_creation: OnCreation::Move("cpu/%1/cpuid".into()).into(),
            ..common_case("cpu([0-9]+)")
        },
        "SUBSYSTEM=input;.* root:input 660" <===> Conf {
            envmatches: vec![EnvMatch { envvar: "SUBSYSTEM".into(), regex: regex("input"), }],
            user: "root".into(), group: "input".into(),
            ..common_case(".*")
        },
        "[0-9]+:[0-9]+:[0-9]+:[0-9]+ root:root 660 !" <===> Conf {
            on_creation: OnCreation::Prevent.into(),
            ..common_case("[0-9]+:[0-9]+:[0-9]+:[0-9]+")
        },
    }

    #[test]
    fn test_all() {
        let conf = parse(INPUT);
        let hardcoded = outs();

        for (a, b) in conf.iter().zip(hardcoded.iter()) {
            assert_eq!(a, b);
        }

        for (source, parsed) in INPUT.lines().zip(conf.iter().map(ToString::to_string)) {
            let parts = source.split_whitespace().zip(parsed.split_whitespace());
            for (source, parsed) in parts {
                assert_eq!(source, parsed)
            }
        }
    }
}
