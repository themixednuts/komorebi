use std::collections::HashMap;
use std::fmt;
use std::fs::File;
use std::io::Write;
use std::path::Component;
use std::path::Path;
use std::path::PathBuf;
use std::str::FromStr;

use serde::Deserialize;
use serde::Serialize;
use uds_windows::UnixStream;

use crate::core::SubscribeOptions;

/// Single-component subscriber leaf accepted at the untrusted command edge.
#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "schemars", schemars(transparent))]
#[serde(transparent)]
pub struct SubscriberName(String);

impl SubscriberName {
    pub const MAX_LEN: usize = 128;

    pub fn parse(raw: &str) -> Result<Self, SubscriberNameError> {
        if raw.is_empty() {
            return Err(SubscriberNameError::Empty);
        }
        if raw.len() > Self::MAX_LEN {
            return Err(SubscriberNameError::TooLong { len: raw.len() });
        }
        let mut chars = raw.chars();
        let Some(first) = chars.next() else {
            return Err(SubscriberNameError::Empty);
        };
        if !first.is_ascii_alphanumeric() {
            return Err(SubscriberNameError::ForbiddenCharacter);
        }
        if !chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-')) {
            return Err(SubscriberNameError::ForbiddenCharacter);
        }
        if raw.ends_with('.') {
            return Err(SubscriberNameError::ForbiddenCharacter);
        }
        if is_reserved_device_name(raw) {
            return Err(SubscriberNameError::ReservedDeviceName);
        }

        let path = Path::new(raw);
        if path.is_absolute() {
            return Err(SubscriberNameError::NotASingleLeaf);
        }
        let mut components = path.components();
        match (components.next(), components.next()) {
            (Some(Component::Normal(os)), None) if os == raw => Ok(Self(raw.to_owned())),
            _ => Err(SubscriberNameError::NotASingleLeaf),
        }
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    #[must_use]
    pub fn named_pipe_path(&self) -> String {
        format!(r"\\.\pipe\{}", self.0)
    }
}

impl fmt::Display for SubscriberName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for SubscriberName {
    type Err = SubscriberNameError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

impl<'de> Deserialize<'de> for SubscriberName {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        Self::parse(&raw).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum SubscriberNameError {
    Empty,
    TooLong { len: usize },
    ForbiddenCharacter,
    ReservedDeviceName,
    NotASingleLeaf,
}

impl fmt::Display for SubscriberNameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "subscriber name is empty"),
            Self::TooLong { len } => write!(
                f,
                "subscriber name is {len} bytes; maximum is {}",
                SubscriberName::MAX_LEN
            ),
            Self::ForbiddenCharacter => write!(
                f,
                "subscriber name must be a single leaf of ASCII letters, digits, '.', '_' or '-'"
            ),
            Self::ReservedDeviceName => {
                write!(f, "subscriber name is a reserved Windows device name")
            }
            Self::NotASingleLeaf => {
                write!(
                    f,
                    "subscriber name must be a single relative path component"
                )
            }
        }
    }
}

impl std::error::Error for SubscriberNameError {}

fn is_reserved_device_name(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or(name);
    matches!(
        stem.to_ascii_uppercase().as_str(),
        "CON"
            | "PRN"
            | "AUX"
            | "NUL"
            | "COM1"
            | "COM2"
            | "COM3"
            | "COM4"
            | "COM5"
            | "COM6"
            | "COM7"
            | "COM8"
            | "COM9"
            | "LPT1"
            | "LPT2"
            | "LPT3"
            | "LPT4"
            | "LPT5"
            | "LPT6"
            | "LPT7"
            | "LPT8"
            | "LPT9"
    )
}

/// Manager-created socket identity. The path is computed from the data directory
/// and a parsed name, never from a raw caller string at cleanup time.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SubscriberSocketPath {
    name: SubscriberName,
    path: PathBuf,
}

impl SubscriberSocketPath {
    pub fn admit(data_dir: &Path, name: SubscriberName) -> Result<Self, SubscriberAdmitError> {
        let path = data_dir.join(name.as_str());
        inspect_existing(data_dir, &path)?;
        Ok(Self { name, path })
    }

    #[must_use]
    pub fn name(&self) -> &SubscriberName {
        &self.name
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn remove_file(&self, data_dir: &Path) -> Result<RemoveOutcome, SubscriberCleanupError> {
        if self.path != data_dir.join(self.name.as_str()) {
            return Err(SubscriberCleanupError::NotManagerRecorded);
        }
        remove_owned_leaf(data_dir, &self.path)
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum SubscriberAdmitError {
    ReparseEscape,
    NotAFile,
}

impl fmt::Display for SubscriberAdmitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ReparseEscape => write!(
                f,
                "subscriber path is a reparse point that escapes the manager data directory"
            ),
            Self::NotAFile => write!(f, "subscriber path exists and is not a file"),
        }
    }
}

impl std::error::Error for SubscriberAdmitError {}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum SubscriberCleanupError {
    NotManagerRecorded,
    EscapesDataDir,
    ReparseEscape,
    NotAFile,
    Io(String),
}

impl fmt::Display for SubscriberCleanupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotManagerRecorded => {
                write!(f, "subscriber path is not the manager-recorded identity")
            }
            Self::EscapesDataDir => {
                write!(
                    f,
                    "subscriber path is not a child of the manager data directory"
                )
            }
            Self::ReparseEscape => write!(
                f,
                "refusing to delete a reparse point whose target escapes the manager data directory"
            ),
            Self::NotAFile => write!(f, "subscriber path is not a file"),
            Self::Io(message) => write!(f, "subscriber cleanup failed: {message}"),
        }
    }
}

impl std::error::Error for SubscriberCleanupError {}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum RemoveOutcome {
    Removed,
    AlreadyGone,
}

#[derive(Debug, Default)]
pub struct SubscriberRegistry {
    sockets: HashMap<SubscriberName, SubscriberSocketPath>,
    options: HashMap<SubscriberName, SubscribeOptions>,
    pipes: HashMap<SubscriberName, File>,
}

impl SubscriberRegistry {
    pub fn add_socket(
        &mut self,
        data_dir: &Path,
        name: SubscriberName,
        options: Option<SubscribeOptions>,
    ) -> Result<(), SubscriberAdmitError> {
        let endpoint = SubscriberSocketPath::admit(data_dir, name.clone())?;
        match options {
            Some(options) => {
                self.options.insert(name.clone(), options);
            }
            None => {
                self.options.remove(&name);
            }
        }
        self.sockets.insert(name, endpoint);
        Ok(())
    }

    pub fn remove_socket(&mut self, name: &SubscriberName) {
        self.sockets.remove(name);
        self.options.remove(name);
    }

    pub fn add_pipe(&mut self, name: SubscriberName, pipe: File) {
        self.pipes.insert(name, pipe);
    }

    pub fn remove_pipe(&mut self, name: &SubscriberName) {
        self.pipes.remove(name);
    }

    #[must_use]
    pub fn contains_socket(&self, name: &SubscriberName) -> bool {
        self.sockets.contains_key(name)
    }

    pub fn socket_paths(&self) -> impl Iterator<Item = &Path> {
        self.sockets.values().map(SubscriberSocketPath::path)
    }

    pub fn deliver(
        &mut self,
        data_dir: &Path,
        notification: &str,
        state_has_been_modified: bool,
        is_override_event: bool,
    ) -> Result<(), SubscriberCleanupError> {
        let mut stale_sockets = Vec::new();
        for (name, endpoint) in &self.sockets {
            let apply_state_filter = self
                .options
                .get(name)
                .copied()
                .unwrap_or_default()
                .filter_state_changes;
            if !apply_state_filter || state_has_been_modified || is_override_event {
                match UnixStream::connect(endpoint.path()) {
                    Ok(mut stream) => {
                        tracing::debug!("pushed notification to subscriber: {name}");
                        if let Err(error) = stream.write_all(notification.as_bytes()) {
                            tracing::error!("could not write to subscriber {name}: {error}");
                            stale_sockets.push(name.clone());
                        }
                    }
                    Err(_) => stale_sockets.push(name.clone()),
                }
            }
        }

        for name in stale_sockets {
            let _ = self.cleanup_stale_socket(data_dir, &name);
        }

        let mut stale_pipes = Vec::new();
        for (name, pipe) in &mut self.pipes {
            match writeln!(pipe, "{notification}") {
                Ok(()) => {
                    tracing::debug!("pushed notification to subscriber: {name}");
                }
                Err(error) => {
                    if let Some(2 | 232) = error.raw_os_error() {
                        stale_pipes.push(name.clone());
                    }
                }
            }
        }

        for name in stale_pipes {
            tracing::warn!("removing stale subscription: {name}");
            self.pipes.remove(&name);
        }

        Ok(())
    }

    fn cleanup_stale_socket(
        &mut self,
        data_dir: &Path,
        name: &SubscriberName,
    ) -> Result<(), SubscriberCleanupError> {
        tracing::warn!("removing stale subscription: {name}");
        self.options.remove(name);
        let Some(endpoint) = self.sockets.remove(name) else {
            return Ok(());
        };
        match endpoint.remove_file(data_dir) {
            Ok(RemoveOutcome::Removed | RemoveOutcome::AlreadyGone) => Ok(()),
            Err(error) => {
                tracing::error!(
                    "could not remove stale subscriber socket file at {}: {error}",
                    endpoint.path().display()
                );
                Err(error)
            }
        }
    }
}

fn inspect_existing(data_dir: &Path, path: &Path) -> Result<(), SubscriberAdmitError> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(_) => return Ok(()),
    };

    if metadata.is_dir() {
        return Err(SubscriberAdmitError::NotAFile);
    }

    // AF_UNIX sockets are reparse points on Windows. Only follow symlink
    // reparse points when checking for an escape.
    if metadata.file_type().is_symlink() && resolved_escapes(data_dir, path) {
        return Err(SubscriberAdmitError::ReparseEscape);
    }

    Ok(())
}

fn remove_owned_leaf(
    data_dir: &Path,
    recorded: &Path,
) -> Result<RemoveOutcome, SubscriberCleanupError> {
    if !is_lexical_child(data_dir, recorded) {
        return Err(SubscriberCleanupError::EscapesDataDir);
    }

    let metadata = match std::fs::symlink_metadata(recorded) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(RemoveOutcome::AlreadyGone);
        }
        Err(error) => return Err(SubscriberCleanupError::Io(error.to_string())),
    };

    if metadata.is_dir() {
        return Err(SubscriberCleanupError::NotAFile);
    }

    if metadata.file_type().is_symlink() && resolved_escapes(data_dir, recorded) {
        return Err(SubscriberCleanupError::ReparseEscape);
    }

    match std::fs::remove_file(recorded) {
        Ok(()) => Ok(RemoveOutcome::Removed),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(RemoveOutcome::AlreadyGone)
        }
        Err(error) => Err(SubscriberCleanupError::Io(error.to_string())),
    }
}

fn is_lexical_child(parent: &Path, child: &Path) -> bool {
    child.parent() == Some(parent)
}

fn resolved_escapes(data_dir: &Path, path: &Path) -> bool {
    let Ok(resolved_dir) = std::fs::canonicalize(data_dir) else {
        return true;
    };
    let Ok(resolved_path) = std::fs::canonicalize(path) else {
        return true;
    };
    !is_canonical_child(&resolved_dir, &resolved_path)
}

fn is_canonical_child(parent: &Path, child: &Path) -> bool {
    use std::os::windows::ffi::OsStrExt;

    let parent: Vec<u16> = parent.as_os_str().encode_wide().collect();
    let child: Vec<u16> = child.as_os_str().encode_wide().collect();
    if child.len() <= parent.len() {
        return false;
    }
    if !eq_ignore_ascii_case_wide(&parent, &child[..parent.len()]) {
        return false;
    }
    matches!(child[parent.len()], 0x5C | 0x2F)
}

fn eq_ignore_ascii_case_wide(left: &[u16], right: &[u16]) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .all(|(a, b)| wide_eq_ignore_ascii_case(*a, *b))
}

fn wide_eq_ignore_ascii_case(left: u16, right: u16) -> bool {
    if left == right {
        return true;
    }
    ascii_wide_lower(left) == ascii_wide_lower(right)
}

fn ascii_wide_lower(unit: u16) -> u16 {
    if (u16::from(b'A')..=u16::from(b'Z')).contains(&unit) {
        unit + 32
    } else {
        unit
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use uds_windows::UnixListener;

    fn parse_err(raw: &str) -> SubscriberNameError {
        SubscriberName::parse(raw).expect_err("name should be rejected")
    }

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "komorebi-subscriber-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    fn write_file(path: &Path, contents: &[u8]) {
        std::fs::write(path, contents).expect("write");
    }

    #[test]
    fn accepts_legacy_bar_and_test_socket_names() {
        for name in [
            "komorebi-bar-forest",
            "komorebi-test-4d3c2b1a-0e9f-4a67-8c15-abcdef012345.sock",
            "status_bar",
        ] {
            assert_eq!(SubscriberName::parse(name).unwrap().as_str(), name);
        }
    }

    #[test]
    fn rejects_absolute_rooted_parent_device_ads_and_mixed_separators() {
        assert_eq!(
            parse_err(r"C:\Windows\win.ini"),
            SubscriberNameError::ForbiddenCharacter
        );
        assert_eq!(
            parse_err(r"\Windows\win.ini"),
            SubscriberNameError::ForbiddenCharacter
        );
        assert_eq!(
            parse_err("../secrets"),
            SubscriberNameError::ForbiddenCharacter
        );
        assert_eq!(
            parse_err(r"..\secrets"),
            SubscriberNameError::ForbiddenCharacter
        );
        assert_eq!(
            parse_err("foo/bar"),
            SubscriberNameError::ForbiddenCharacter
        );
        assert_eq!(
            parse_err(r"foo\bar"),
            SubscriberNameError::ForbiddenCharacter
        );
        assert_eq!(
            parse_err(r"\\.\pipe\komorebi"),
            SubscriberNameError::ForbiddenCharacter
        );
        assert_eq!(
            parse_err(r"\\?\C:\Windows\win.ini"),
            SubscriberNameError::ForbiddenCharacter
        );
        assert_eq!(
            parse_err("socket:stream"),
            SubscriberNameError::ForbiddenCharacter
        );
        assert_eq!(
            parse_err("socket:stream:$DATA"),
            SubscriberNameError::ForbiddenCharacter
        );
        assert_eq!(parse_err(""), SubscriberNameError::Empty);
        assert_eq!(parse_err("."), SubscriberNameError::ForbiddenCharacter);
        assert_eq!(parse_err(".."), SubscriberNameError::ForbiddenCharacter);
        assert_eq!(parse_err("NUL"), SubscriberNameError::ReservedDeviceName);
        assert_eq!(
            parse_err("con.txt"),
            SubscriberNameError::ReservedDeviceName
        );
        assert_eq!(
            parse_err(&"a".repeat(SubscriberName::MAX_LEN + 1)),
            SubscriberNameError::TooLong {
                len: SubscriberName::MAX_LEN + 1
            }
        );
    }

    #[test]
    fn named_pipe_path_is_constructed_from_the_parsed_leaf() {
        let name = SubscriberName::parse("komorebi-bar").unwrap();
        assert_eq!(name.named_pipe_path(), r"\\.\pipe\komorebi-bar");
    }

    #[test]
    fn socket_message_rejects_escaping_subscriber_names() {
        let message = r#"{"type":"AddSubscriberSocket","content":"..\\secrets"}"#;
        let error = crate::core::SocketMessage::from_str(message).expect_err("must reject");
        assert!(error.to_string().contains("subscriber name"));
    }

    #[test]
    fn socket_message_keeps_legacy_subscriber_names() {
        let message = r#"{"type":"AddSubscriberSocket","content":"komorebi-bar-forest"}"#;
        let parsed = crate::core::SocketMessage::from_str(message).unwrap();
        assert!(matches!(
            parsed,
            crate::core::SocketMessage::AddSubscriberSocket(name) if name.as_str() == "komorebi-bar-forest"
        ));
    }

    #[test]
    fn admit_records_the_manager_joined_path() {
        let dir = temp_dir();
        let name = SubscriberName::parse("legacy-bar").unwrap();
        let endpoint = SubscriberSocketPath::admit(&dir, name.clone()).unwrap();
        assert_eq!(endpoint.path(), dir.join("legacy-bar"));
        assert_eq!(endpoint.name(), &name);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn cleanup_uses_recorded_identity_and_is_idempotent() {
        let dir = temp_dir();
        let name = SubscriberName::parse("legacy-bar").unwrap();
        write_file(&dir.join("legacy-bar"), b"socket");
        let endpoint = SubscriberSocketPath::admit(&dir, name).unwrap();
        assert_eq!(endpoint.remove_file(&dir).unwrap(), RemoveOutcome::Removed);
        assert!(!dir.join("legacy-bar").exists());
        assert_eq!(
            endpoint.remove_file(&dir).unwrap(),
            RemoveOutcome::AlreadyGone
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn cleanup_refuses_a_path_that_is_not_the_recorded_identity() {
        let dir = temp_dir();
        let name = SubscriberName::parse("legacy-bar").unwrap();
        let mut endpoint = SubscriberSocketPath::admit(&dir, name).unwrap();
        endpoint.path = dir.join("..").join("escape.txt");
        assert_eq!(
            endpoint.remove_file(&dir).unwrap_err(),
            SubscriberCleanupError::NotManagerRecorded
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn cleanup_refuses_to_delete_an_escaped_leaf() {
        let dir = temp_dir();
        let outside = dir.join("..").join(format!(
            "komorebi-subscriber-outside-{}",
            std::process::id()
        ));
        write_file(&outside, b"secret");
        let error = remove_owned_leaf(&dir, &outside).unwrap_err();
        assert_eq!(error, SubscriberCleanupError::EscapesDataDir);
        assert!(outside.exists());
        std::fs::remove_file(&outside).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn admit_and_cleanup_refuse_reparse_points_that_escape() {
        let dir = temp_dir();
        let outside =
            std::env::temp_dir().join(format!("komorebi-subscriber-secret-{}", std::process::id()));
        write_file(&outside, b"secret");
        let link = dir.join("legacy-bar");
        match std::os::windows::fs::symlink_file(&outside, &link) {
            Ok(()) => {
                let name = SubscriberName::parse("legacy-bar").unwrap();
                assert_eq!(
                    SubscriberSocketPath::admit(&dir, name.clone()).unwrap_err(),
                    SubscriberAdmitError::ReparseEscape
                );
                assert_eq!(
                    remove_owned_leaf(&dir, &link).unwrap_err(),
                    SubscriberCleanupError::ReparseEscape
                );
                assert!(outside.exists());
                std::fs::remove_file(&link).unwrap();
            }
            Err(error) => {
                eprintln!("skipping reparse test without symlink privilege: {error}");
            }
        }
        std::fs::remove_file(&outside).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn failed_delivery_cleans_the_recorded_file_and_options_once() {
        let dir = temp_dir();
        let name = SubscriberName::parse("legacy-bar").unwrap();
        write_file(&dir.join("legacy-bar"), b"not-a-listener");
        let mut registry = SubscriberRegistry::default();
        registry
            .add_socket(
                &dir,
                name.clone(),
                Some(SubscribeOptions {
                    filter_state_changes: false,
                }),
            )
            .unwrap();
        assert!(registry.contains_socket(&name));
        registry.deliver(&dir, "{}", true, true).unwrap();
        assert!(!registry.contains_socket(&name));
        assert!(!dir.join("legacy-bar").exists());
        registry.deliver(&dir, "{}", true, true).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn legitimate_legacy_socket_stays_registered_after_delivery() {
        let dir = temp_dir();
        let name = SubscriberName::parse("komorebi-bar-forest").unwrap();
        let listener = UnixListener::bind(dir.join(name.as_str())).unwrap();
        let mut registry = SubscriberRegistry::default();
        registry.add_socket(&dir, name.clone(), None).unwrap();

        let incoming = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buffer = String::new();
            stream.read_to_string(&mut buffer).unwrap();
            buffer
        });

        registry.deliver(&dir, "{\"ok\":true}", true, true).unwrap();
        assert_eq!(incoming.join().unwrap(), "{\"ok\":true}");
        assert!(registry.contains_socket(&name));
        assert!(dir.join(name.as_str()).exists());

        std::fs::remove_file(dir.join(name.as_str())).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
