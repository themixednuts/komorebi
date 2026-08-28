use std::ffi::{OsStr, OsString};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::windows::ffi::OsStringExt;
use std::path::{Path, PathBuf};

use tempfile::TempDir;
use thiserror::Error;

pub struct SyntheticFixture {
    directory: TempDir,
    pub visible: PathBuf,
    pub ignored: PathBuf,
    pub invalid_wtf16: Option<PathBuf>,
    pub reparse: Option<PathBuf>,
}

impl SyntheticFixture {
    pub fn build() -> Result<Self, FixtureError> {
        let directory = tempfile::Builder::new()
            .prefix("wayfinder-palette-")
            .tempdir()?;
        let root = directory.path();
        git2::Repository::init(root)?;
        fs::create_dir(root.join("src"))?;
        fs::create_dir(root.join("ignored"))?;
        fs::write(root.join(".gitignore"), b"ignored/\n*.secret\n")?;
        fs::write(root.join(".ignore"), b"ignored/\n*.secret\n")?;
        let visible = root.join("src").join("manager_epoch.rs");
        fs::write(
            &visible,
            b"pub struct ManagerEpoch(u64);\nconst SEARCH_NEEDLE: &str = \"revisioned-manager\";\n",
        )?;
        fs::write(
            root.join("src").join("palette.rs"),
            b"fn command_palette() {}\n",
        )?;
        let ignored = root.join("ignored").join("private.secret");
        fs::write(&ignored, b"revisioned-manager should not be indexed\n")?;

        let invalid_wtf16 = create_invalid_wtf16_file(root).ok();
        let reparse = create_reparse_fixture(root).ok();
        Ok(Self {
            directory,
            visible,
            ignored,
            invalid_wtf16,
            reparse,
        })
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        self.directory.path()
    }

    pub fn add_replacement_file(root: &Path) -> Result<PathBuf, FixtureError> {
        let path = root.join("src").join("replacement_generation.rs");
        fs::write(&path, b"struct ReplacementGeneration(u64);\n")?;
        Ok(path)
    }
}

fn create_invalid_wtf16_file(root: &Path) -> Result<PathBuf, FixtureError> {
    let mut name = "invalid-".encode_utf16().collect::<Vec<_>>();
    name.push(0xD800);
    name.extend(".txt".encode_utf16());
    let path = root.join(OsString::from_wide(&name));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)?;
    file.write_all(b"lossless native path fixture\n")?;
    file.sync_all()?;
    Ok(path)
}

fn create_reparse_fixture(root: &Path) -> Result<PathBuf, FixtureError> {
    let target = root.join("src");
    let link = root.join("src-link");
    std::os::windows::fs::symlink_dir(target, &link)?;
    Ok(link)
}

#[derive(Debug, Error)]
pub enum FixtureError {
    #[error("fixture filesystem operation failed")]
    Io(#[from] std::io::Error),
    #[error("fixture Git repository initialization failed")]
    Git(#[from] git2::Error),
}

#[allow(dead_code)]
fn _native_string_marker(_: &OsStr) {}
