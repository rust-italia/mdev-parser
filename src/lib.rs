use std::{
    convert::{TryFrom, TryInto},
    error::Error as StdError,
    fmt::Display,
};

use regex::Regex;

use tracing::error;

#[derive(Debug)]
/// Enumeration of all the errors that can happen while parsing a configuration line
pub enum ParsingError {
    /// The line is empty
    Empty,
    /// The line is a comment
    Comment,
    /// The device name regex is invalid
    DeviceRegex(String),
    /// The version is invalid
    MajMin(String),
    /// The mode is invalid
    Mode(String),
    /// The env var regex is invalid
    EnvRegex(String),
    /// The user group is invalid
    UserGroup(String),
    /// The on creation instruction is invalid
    OnCreation(String),
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
        }
    }
}

impl StdError for ParsingError {}

type Error = ParsingError;

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
}

impl TryFrom<&str> for Conf {
    type Error = Error;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        if s.starts_with('#') {
            return Err(Error::Comment);
        }

        let mut parts = s.split_whitespace();

        let mut first_part = parts.next().ok_or(Error::Empty)?;
        let stop = first_part.bytes().next() == Some(b'-');
        if stop {
            first_part = &first_part[1..];
        }

        let filter = match first_part.bytes().next() {
            Some(b'@') => Filter::MajMin(first_part[1..].try_into()?),
            Some(b'$') => Filter::EnvRegex(first_part[1..].try_into()?),
            _ => Filter::DeviceRegex(first_part.try_into()?),
        };

        let user_group = parts
            .next()
            .ok_or_else(|| Error::UserGroup("missing".into()))?
            .try_into()?;

        let mode = parts
            .next()
            .ok_or_else(|| Error::Mode("missing".into()))?
            .try_into()?;

        let on_creation = match parts.next() {
            None => None,
            Some(s) => Some(s.try_into()?),
        };

        //TODO: optional parts

        Ok(Conf {
            stop,
            filter,
            user_group,
            mode,
            on_creation,
        })
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

impl TryFrom<&str> for DeviceRegex {
    type Error = Error;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        Ok(Self {
            regex: Regex::new(s)
                .map_err(|err| Error::DeviceRegex(format!("invalid regex: {}", err)))?,
        })
    }
}

#[derive(Debug)]
/// TODO: add docs
pub struct EnvRegex {
    var: String,
    regex: Regex,
}

impl TryFrom<&str> for EnvRegex {
    type Error = Error;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        let (var, regex) = s
            .split_once('=')
            .ok_or_else(|| Error::EnvRegex("missing value".into()))?;
        if contains_whitespaces(var) {
            return Err(Error::EnvRegex("env var contains white spaces".into()));
        }
        Ok(Self {
            var: var.into(),
            regex: Regex::new(regex)
                .map_err(|err| Error::EnvRegex(format!("invalid regex: {}", err)))?,
        })
    }
}

#[derive(Debug)]
/// TODO: add docs
pub struct MajMin {
    maj: u8,
    min: u8,
    min2: Option<u8>,
}

impl TryFrom<&str> for MajMin {
    type Error = Error;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        let (maj, mut min) = s
            .split_once(',')
            .ok_or_else(|| Error::MajMin("missing min".into()))?;
        let mut min2 = None;
        if let Some((m, m2)) = min.split_once('-') {
            min = m;
            min2 = Some(
                m2.parse()
                    .map_err(|_| Error::MajMin("invalid min2".into()))?,
            );
        }
        Ok(Self {
            maj: maj
                .parse()
                .map_err(|_| Error::MajMin("invalid maj".into()))?,
            min: min
                .parse()
                .map_err(|_| Error::MajMin("invalid min".into()))?,
            min2,
        })
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

impl TryFrom<&str> for UserGroup {
    type Error = Error;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        let (user, group) = s
            .split_once(":")
            .ok_or_else(|| Error::UserGroup("missing group".into()))?;
        if contains_whitespaces(user) {
            return Err(Error::UserGroup("user name contains white spaces".into()));
        }
        if contains_whitespaces(group) {
            return Err(Error::UserGroup("group name contains white spaces".into()));
        }
        Ok(Self {
            user: user.into(),
            group: group.into(),
        })
    }
}

#[derive(Debug)]
/// Contains the access mode or permissions
pub struct Mode {
    /// Permissions, each value is between `b'0'` and `b'7'`
    mode: [u8; 3],
}

impl TryFrom<&str> for Mode {
    type Error = Error;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        match *s.as_bytes() {
            [a @ b'0'..=b'7', b @ b'0'..=b'7', c @ b'0'..=b'7'] => Ok(Self { mode: [a, b, c] }),
            _ => Err(Error::Mode("invalid value".into())),
        }
    }
}

#[derive(Debug)]
/// Additional actions to take on creation of the device node
pub enum OnCreation {
    /// Moves/renames the device
    Move(String),
    /// Same as [`OnCreation::Move`] but also creates a symlink in `/dev/` to the
    /// renamed/moved device
    SymLink(String),
    /// Prevents the creation of the device node
    Prevent,
}

impl TryFrom<&str> for OnCreation {
    type Error = Error;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        match s.bytes().next() {
            Some(b'=') | Some(b'>') if contains_whitespaces(&s[1..]) => {
                Err(Error::OnCreation("path contains whitespaces".into()))
            }
            Some(b'=') => Ok(Self::Move(s[1..].into())),
            Some(b'>') => Ok(Self::SymLink(s[1..].into())),
            Some(b'!') if s.len() == 1 => Ok(Self::Prevent),
            _ => Err(Error::OnCreation("invalid symbol".into())),
        }
    }
}

/// Parses every line of the configuration contained in `input` excluding invalid ones.
pub fn parse(input: &str) -> Vec<Conf> {
    input
        .lines()
        .filter_map(|s| match s.try_into() {
            Ok(v) => Some(v),
            Err(Error::Comment) | Err(Error::Empty) => None,
            Err(err) => {
                error!("parsing error: {}", err);
                None
            }
        })
        .collect()
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
        println!("{:#?}", super::parse(input));
    }
}
