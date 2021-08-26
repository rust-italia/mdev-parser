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
        let input = r#"
# mdev-like-a-boss

# Syntax:
# [-]devicename_regex user:group mode [=path]|[>path]|[!] [@|$|*cmd args...]
# [-]$ENVVAR=regex    user:group mode [=path]|[>path]|[!] [@|$|*cmd args...]
# [-]@maj,min[-min2]  user:group mode [=path]|[>path]|[!] [@|$|*cmd args...]
#
# [-]: do not stop on this match, continue reading mdev.conf
# =: move, >: move and create a symlink
# !: do not create device node
# @|$|*: run cmd if $ACTION=remove, @cmd if $ACTION=add, *cmd in all cases

# support module loading on hotplug
$MODALIAS=.*    root:root 660 @modprobe -b "$MODALIAS"

# null may already exist; therefore ownership has to be changed with command
null        root:root 666 @chmod 666 $MDEV
zero        root:root 666
full        root:root 666
random      root:root 444
urandom     root:root 444
hwrandom    root:root 444
grsec       root:root 660

# Kernel-based Virtual Machine.
kvm     root:kvm 660

# vhost-net, to be used with kvm.
vhost-net   root:kvm 660

kmem        root:root 640
mem         root:root 640
port        root:root 640
# console may already exist; therefore ownership has to be changed with command
console     root:tty 600 @chmod 600 $MDEV
ptmx        root:tty 666
pty.*       root:tty 660

# Typical devices
tty         root:tty 666
tty[0-9]*   root:tty 660
vcsa*[0-9]* root:tty 660
ttyS[0-9]*  root:uucp 660

# block devices
ram([0-9]*)        root:disk 660 >rd/%1
loop([0-9]+)       root:disk 660 >loop/%1
sr[0-9]*           root:cdrom 660 @ln -sf $MDEV cdrom
fd[0-9]*           root:floppy 660
SUBSYSTEM=block;.* root:disk 660 */opt/mdev/helpers/storage-device

# Run settle-nics every time new NIC appear.
# If you don't want to auto-populate /etc/mactab with NICs, run 'settle-nis' without '--write-mactab' param.
-SUBSYSTEM=net;DEVPATH=.*/net/.*;.*     root:root 600 @/opt/mdev/helpers/settle-nics --write-mactab

net/tun[0-9]*   root:kvm 660
net/tap[0-9]*   root:root 600

# alsa sound devices and audio stuff
SUBSYSTEM=sound;.*  root:audio 660 @/opt/mdev/helpers/sound-control

adsp        root:audio 660 >sound/
audio       root:audio 660 >sound/
dsp         root:audio 660 >sound/
mixer       root:audio 660 >sound/
sequencer.* root:audio 660 >sound/


# raid controllers
cciss!(.*)  root:disk 660 =cciss/%1
ida!(.*)    root:disk 660 =ida/%1
rd!(.*)     root:disk 660 =rd/%1


fuse        root:root 666

card[0-9]   root:video 660 =dri/

agpgart     root:root 660 >misc/
psaux       root:root 660 >misc/
rtc         root:root 664 >misc/

# input stuff
SUBSYSTEM=input;.* root:input 660

# v4l stuff
vbi[0-9]    root:video 660 >v4l/
video[0-9]  root:video 660 >v4l/

# dvb stuff
dvb.*       root:video 660

# drm etc
dri/.*      root:video 660

# Don't create old usbdev* devices.
usbdev[0-9].[0-9]* root:root 660 !

# Stop creating x:x:x:x which looks like /dev/dm-*
[0-9]+\:[0-9]+\:[0-9]+\:[0-9]+ root:root 660 !

# /dev/cpu support.
microcode       root:root 600 =cpu/
cpu([0-9]+)     root:root 600 =cpu/%1/cpuid
msr([0-9]+)     root:root 600 =cpu/%1/msr

# Populate /dev/bus/usb.
SUBSYSTEM=usb;DEVTYPE=usb_device;.* root:root 660 */opt/mdev/helpers/dev-bus-usb

# Catch-all other devices, Right now useful only for debuging.
#.* root:root 660 */opt/mdev/helpers/catch-all
"#;
        for conf in super::parse(input) {
            println!("{}", conf);
        }
    }
}
