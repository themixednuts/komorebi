use std::mem::size_of;
use std::ptr::null_mut;

use serde::{Deserialize, Serialize};
use thiserror::Error;
use windows_sys::Win32::System::JobObjects::{
    IsProcessInJob, JOB_OBJECT_LIMIT_BREAKAWAY_OK, JOB_OBJECT_LIMIT_SILENT_BREAKAWAY_OK,
    JOBOBJECT_BASIC_UI_RESTRICTIONS, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JobObjectBasicUIRestrictions, JobObjectExtendedLimitInformation, QueryInformationJobObject,
};
use windows_sys::Win32::System::Threading::{CREATE_BREAKAWAY_FROM_JOB, GetCurrentProcess};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum JobUiMode {
    Enforced,
    OmittedForNesting,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum HostJobMode {
    Standalone,
    ExplicitBreakaway,
    SilentBreakaway,
    Nested,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum JobContextRejection {
    UiRestrictionsWithoutBreakaway,
}

#[derive(Debug, Error)]
pub(super) enum JobContextError {
    #[error("query host Job membership")]
    Membership(#[source] std::io::Error),
    #[error("query immediate Job limits")]
    Limits(#[source] std::io::Error),
    #[error("query immediate Job UI restrictions")]
    UiRestrictions(#[source] std::io::Error),
    #[error(
        "host Job sets UI restrictions and denies breakaway; Windows cannot form the required inner Job"
    )]
    UiRestrictionsWithoutBreakaway,
}

impl JobContextError {
    pub(super) const fn rejection(&self) -> Option<JobContextRejection> {
        match self {
            Self::UiRestrictionsWithoutBreakaway => {
                Some(JobContextRejection::UiRestrictionsWithoutBreakaway)
            }
            Self::Membership(_) | Self::Limits(_) | Self::UiRestrictions(_) => None,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct LaunchJobContext(HostJobMode);

impl LaunchJobContext {
    pub(super) fn detect() -> Result<Self, JobContextError> {
        let mut in_job = 0;
        // SAFETY: the pseudo-handle is valid and in_job is writable.
        if unsafe { IsProcessInJob(GetCurrentProcess(), null_mut(), &raw mut in_job) } == 0 {
            return Err(JobContextError::Membership(std::io::Error::last_os_error()));
        }
        if in_job == 0 {
            return Ok(Self::standalone());
        }

        let limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION =
            query_immediate_job(JobObjectExtendedLimitInformation)
                .map_err(JobContextError::Limits)?;
        let flags = limits.BasicLimitInformation.LimitFlags;
        if flags & JOB_OBJECT_LIMIT_SILENT_BREAKAWAY_OK != 0 {
            return Ok(Self(HostJobMode::SilentBreakaway));
        }
        if flags & JOB_OBJECT_LIMIT_BREAKAWAY_OK != 0 {
            return Ok(Self(HostJobMode::ExplicitBreakaway));
        }

        let ui: JOBOBJECT_BASIC_UI_RESTRICTIONS = query_immediate_job(JobObjectBasicUIRestrictions)
            .map_err(JobContextError::UiRestrictions)?;
        if ui.UIRestrictionsClass != 0 {
            return Err(JobContextError::UiRestrictionsWithoutBreakaway);
        }
        Ok(Self(HostJobMode::Nested))
    }

    const fn standalone() -> Self {
        Self(HostJobMode::Standalone)
    }

    pub(super) const fn mode(self) -> HostJobMode {
        self.0
    }

    pub(super) const fn process_creation_flags(self) -> u32 {
        match self.0 {
            HostJobMode::ExplicitBreakaway => CREATE_BREAKAWAY_FROM_JOB,
            HostJobMode::Standalone | HostJobMode::SilentBreakaway | HostJobMode::Nested => 0,
        }
    }

    pub(super) const fn ui_mode(self) -> JobUiMode {
        match self.0 {
            HostJobMode::Nested => JobUiMode::OmittedForNesting,
            HostJobMode::Standalone
            | HostJobMode::ExplicitBreakaway
            | HostJobMode::SilentBreakaway => JobUiMode::Enforced,
        }
    }
}

fn query_immediate_job<T: Default>(class: i32) -> std::io::Result<T> {
    let mut value = T::default();
    let size = u32::try_from(size_of::<T>())
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidInput, error))?;
    // SAFETY: a null Job handle selects the calling process's immediate Job, and value is writable
    // for the size matching class at each call site.
    if unsafe {
        QueryInformationJobObject(null_mut(), class, (&raw mut value).cast(), size, null_mut())
    } == 0
    {
        return Err(std::io::Error::last_os_error());
    }
    Ok(value)
}
