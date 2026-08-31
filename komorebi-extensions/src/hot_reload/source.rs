use std::io;
use std::path::Path;
use std::path::PathBuf;

use thiserror::Error;

use crate::PluginProgram;
use crate::PluginProgramError;

/// Existing Lua source file watched without repairing its native Windows path.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PluginSourceFile {
    path: PathBuf,
    chunk_name: Box<str>,
}

impl PluginSourceFile {
    /// Resolves one existing source file and validates its diagnostic chunk name.
    pub fn open(path: impl AsRef<Path>, chunk_name: &str) -> Result<Self, PluginSourceOpenError> {
        let requested = path.as_ref();
        let path = dunce::canonicalize(requested).map_err(|source| {
            PluginSourceOpenError::Canonicalize {
                path: requested.to_path_buf(),
                source,
            }
        })?;
        if !path.is_file() {
            return Err(PluginSourceOpenError::NotFile(path));
        }
        PluginProgram::new(chunk_name, []).map_err(PluginSourceOpenError::ChunkName)?;
        Ok(Self {
            path,
            chunk_name: chunk_name.into(),
        })
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Reads and validates the current source through Tokio's blocking-file owner.
    pub async fn load(&self) -> Result<PluginProgram, PluginSourceLoadError> {
        let source =
            tokio::fs::read(&self.path)
                .await
                .map_err(|source| PluginSourceLoadError::Read {
                    path: self.path.clone(),
                    source,
                })?;
        PluginProgram::new(&self.chunk_name, source).map_err(PluginSourceLoadError::Program)
    }
}

#[derive(Debug, Error)]
pub enum PluginSourceOpenError {
    #[error("failed to resolve extension source {path:?}: {source}")]
    Canonicalize {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("extension source is not a regular file: {0:?}")]
    NotFile(PathBuf),
    #[error("invalid extension chunk name: {0}")]
    ChunkName(PluginProgramError),
}

#[derive(Debug, Error)]
pub enum PluginSourceLoadError {
    #[error("failed to read extension source {path:?}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("extension source was rejected: {0}")]
    Program(PluginProgramError),
}
