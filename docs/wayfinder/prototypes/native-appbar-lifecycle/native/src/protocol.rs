use std::io::{BufRead, Write};
use std::num::NonZeroU32;

use anyhow::Context;
use serde::{Deserialize, Serialize};

use crate::model::{Edge, Rect, ShellIdentity};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum ChildEvent {
    CreatedHidden {
        process_id: u32,
    },
    Registered {
        shell: ShellIdentity,
    },
    Positioned {
        reason: PositionReason,
        rect: Rect,
        work_area: Rect,
    },
    Shown {
        rect: Rect,
        work_area: Rect,
        visible_before_position: bool,
    },
    Notification {
        notification: NotificationKind,
    },
    RegistrationSuppressed {
        shell: ShellIdentity,
    },
    Released,
    Failure {
        operation: String,
        message: String,
    },
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PositionReason {
    Initial,
    ShellPositionChanged,
    GeometryChanged,
    ShellRecreated,
}

impl PositionReason {
    #[must_use]
    pub const fn merge(self, newer: Self) -> Self {
        if newer.priority() > self.priority() {
            newer
        } else {
            self
        }
    }

    const fn priority(self) -> u8 {
        match self {
            Self::Initial => 0,
            Self::ShellPositionChanged => 1,
            Self::GeometryChanged => 2,
            Self::ShellRecreated => 3,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationKind {
    PositionChanged,
    FullscreenOpened,
    FullscreenClosed,
    StateChanged,
    WindowArrangeStarted,
    WindowArrangeFinished,
    TaskbarCreated,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChildCommand {
    SetThickness(NonZeroU32),
    SimulateDpi(NonZeroU32),
    RegisterAgain,
    Shutdown,
}

#[derive(Clone, Copy, Debug, thiserror::Error, Eq, PartialEq)]
pub enum CommandError {
    #[error("empty command")]
    Empty,
    #[error("unknown command")]
    Unknown,
    #[error("missing command value")]
    MissingValue,
    #[error("invalid command value")]
    InvalidValue,
    #[error("unexpected command value")]
    UnexpectedValue,
}

impl ChildCommand {
    /// Parses one trusted probe command.
    ///
    /// # Errors
    ///
    /// Returns a typed syntax or value error when the command is not exact.
    pub fn parse(line: &str) -> Result<Self, CommandError> {
        let mut fields = line.split_ascii_whitespace();
        let name = fields.next().ok_or(CommandError::Empty)?;
        let value = fields.next();
        if fields.next().is_some() {
            return Err(CommandError::UnexpectedValue);
        }

        match (name, value) {
            ("set-thickness", Some(value)) => value
                .parse()
                .map(Self::SetThickness)
                .map_err(|_| CommandError::InvalidValue),
            ("simulate-dpi", Some(value)) => value
                .parse()
                .map(Self::SimulateDpi)
                .map_err(|_| CommandError::InvalidValue),
            ("register-again", None) => Ok(Self::RegisterAgain),
            ("shutdown", None) => Ok(Self::Shutdown),
            ("set-thickness" | "simulate-dpi", None) => Err(CommandError::MissingValue),
            ("register-again" | "shutdown", Some(_)) => Err(CommandError::UnexpectedValue),
            _ => Err(CommandError::Unknown),
        }
    }
}

/// Writes one newline-delimited child event.
///
/// # Errors
///
/// Returns an error when serialization or the underlying write fails.
pub fn write_event(mut output: impl Write, event: &ChildEvent) -> anyhow::Result<()> {
    serde_json::to_writer(&mut output, event).context("encode child event")?;
    output.write_all(b"\n").context("terminate child event")?;
    output.flush().context("flush child event")
}

/// Reads one newline-delimited child event.
///
/// # Errors
///
/// Returns an error when the stream cannot be read or the event is invalid JSON.
pub fn read_event(mut input: impl BufRead) -> anyhow::Result<Option<ChildEvent>> {
    let mut line = String::new();
    if input.read_line(&mut line).context("read child event")? == 0 {
        return Ok(None);
    }
    serde_json::from_str(&line)
        .context("decode child event")
        .map(Some)
}

#[derive(Clone, Debug, Serialize)]
pub struct ProbeReport {
    pub schema: u16,
    pub monitor: Rect,
    pub baseline_work_area: Rect,
    pub child_pe_subsystem: u16,
    pub cases: Vec<ProbeCase>,
    pub limitations: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ProbeCase {
    pub name: String,
    pub passed: bool,
    pub evidence: serde_json::Value,
}

#[derive(Clone, Copy, Debug, clap::ValueEnum)]
pub enum EdgeArg {
    Left,
    Top,
    Right,
    Bottom,
}

impl From<EdgeArg> for Edge {
    fn from(value: EdgeArg) -> Self {
        match value {
            EdgeArg::Left => Self::Left,
            EdgeArg::Top => Self::Top,
            EdgeArg::Right => Self::Right,
            EdgeArg::Bottom => Self::Bottom,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::PositionReason;

    #[test]
    fn concurrent_shell_callback_cannot_erase_stronger_position_cause() {
        assert_eq!(
            PositionReason::GeometryChanged.merge(PositionReason::ShellPositionChanged),
            PositionReason::GeometryChanged
        );
        assert_eq!(
            PositionReason::ShellPositionChanged.merge(PositionReason::ShellRecreated),
            PositionReason::ShellRecreated
        );
    }
}
