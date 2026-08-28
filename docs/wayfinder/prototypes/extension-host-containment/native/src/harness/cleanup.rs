use std::ffi::OsStr;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

use anyhow::{Context, Result};
use uuid::Uuid;

use crate::windows::windows_string_evidence;

const PRIVATE_FILE_PREFIX: &str = "host-private-";
const PRIVATE_FILE_SUFFIX: &str = ".txt";
const PRIVATE_FILE_CONTENT: &[u8] = b"must not be readable from LPAC";

pub(super) fn remove_orphan_private_files(results_dir: &Path) -> Result<usize> {
    let mut removed = 0_usize;
    for entry in fs::read_dir(results_dir).context("enumerate containment result artifacts")? {
        let entry = entry.context("read containment result artifact")?;
        if !is_private_probe_name(&entry.file_name()) {
            continue;
        }
        let path = entry.path();
        fs::remove_file(&path)
            .with_context(|| format!("remove orphan private probe {}", path_label(&path)))?;
        removed = removed
            .checked_add(1)
            .context("orphan private probe count overflow")?;
    }
    Ok(removed)
}

pub(super) fn create_private_file(results_dir: &Path) -> Result<std::path::PathBuf> {
    let path = results_dir.join(format!(
        "{PRIVATE_FILE_PREFIX}{}{PRIVATE_FILE_SUFFIX}",
        Uuid::new_v4()
    ));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .with_context(|| format!("create private probe {}", path_label(&path)))?;
    file.write_all(PRIVATE_FILE_CONTENT)
        .with_context(|| format!("write private probe {}", path_label(&path)))?;
    Ok(path)
}

fn is_private_probe_name(name: &OsStr) -> bool {
    name.to_str()
        .and_then(|name| name.strip_prefix(PRIVATE_FILE_PREFIX))
        .and_then(|name| name.strip_suffix(PRIVATE_FILE_SUFFIX))
        .is_some_and(|id| Uuid::parse_str(id).is_ok())
}

fn path_label(path: &Path) -> String {
    let evidence = windows_string_evidence(path.as_os_str());
    evidence
        .utf8
        .unwrap_or_else(|| format!("<UTF-16 code units {}>", evidence.utf16_code_units_hex))
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;

    use super::is_private_probe_name;

    #[test]
    fn cleanup_matches_only_exact_generated_private_probe_names() {
        assert!(is_private_probe_name(OsStr::new(
            "host-private-950e8400-e29b-41d4-a716-446655440000.txt"
        )));
        assert!(!is_private_probe_name(OsStr::new("host-private-note.txt")));
        assert!(!is_private_probe_name(OsStr::new(
            "host-private-950e8400-e29b-41d4-a716-446655440000.txt.bak"
        )));
    }
}
