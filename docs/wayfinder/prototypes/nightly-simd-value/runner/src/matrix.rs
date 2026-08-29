use serde::Serialize;

pub(crate) const PINNED_NIGHTLY: &str = "nightly-2026-08-27";
pub(crate) type Operation = (&'static str, Vec<&'static str>);
type StaticOperation = (&'static str, &'static [&'static str]);

const REPOSITORY_OPERATIONS: [StaticOperation; 6] = [
    ("check-clean", &["check", "--workspace", "--locked"]),
    ("check-incremental", &["check", "--workspace", "--locked"]),
    ("build-debug", &["build", "--workspace", "--locked"]),
    (
        "build-release",
        &["build", "--workspace", "--release", "--locked"],
    ),
    ("test", &["test", "--workspace", "--locked"]),
    (
        "clippy",
        &[
            "clippy",
            "--workspace",
            "--all-targets",
            "--locked",
            "--",
            "-D",
            "warnings",
        ],
    ),
];

const FIXTURE_PACKAGES: &[&str] = &[
    "-p",
    "toolchain-state-compatibility",
    "-p",
    "toolchain-extension-compatibility",
    "-p",
    "toolchain-shell-compatibility",
];

fn fixture_operation(operation: &'static str, command: &'static str) -> Operation {
    let mut arguments = Vec::with_capacity(FIXTURE_PACKAGES.len() + 4);
    arguments.push(command);
    arguments.extend_from_slice(FIXTURE_PACKAGES);
    match operation {
        "build-release" => arguments.push("--release"),
        "clippy" => arguments.push("--all-targets"),
        _ => {}
    }
    arguments.push("--locked");
    if operation == "clippy" {
        arguments.extend_from_slice(&["--", "-D", "warnings"]);
    }
    (operation, arguments)
}

pub(crate) fn operations(scope: Scope) -> Vec<Operation> {
    match scope {
        Scope::Repository => REPOSITORY_OPERATIONS
            .iter()
            .map(|(operation, arguments)| (*operation, arguments.to_vec()))
            .collect(),
        Scope::PlannedStackFixture => [
            ("check-clean", "check"),
            ("check-incremental", "check"),
            ("build-debug", "build"),
            ("build-release", "build"),
            ("test", "test"),
            ("clippy", "clippy"),
        ]
        .map(|(operation, command)| fixture_operation(operation, command))
        .into(),
    }
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum CompilerArm {
    Stable,
    NightlyDefault,
    NightlyNextSolver,
}

impl CompilerArm {
    pub(crate) const ALL: [Self; 3] = [Self::Stable, Self::NightlyDefault, Self::NightlyNextSolver];

    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Stable => "stable",
            Self::NightlyDefault => "nightly-default",
            Self::NightlyNextSolver => "nightly-next-solver",
        }
    }

    pub(crate) fn toolchain(self) -> &'static str {
        match self {
            Self::Stable => "stable",
            Self::NightlyDefault | Self::NightlyNextSolver => PINNED_NIGHTLY,
        }
    }

    pub(crate) fn rustflags(self) -> Option<&'static str> {
        matches!(self, Self::NightlyNextSolver).then_some("-Znext-solver")
    }
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Scope {
    Repository,
    PlannedStackFixture,
}

impl Scope {
    pub(crate) const ALL: [Self; 2] = [Self::Repository, Self::PlannedStackFixture];

    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Repository => "repository",
            Self::PlannedStackFixture => "planned-stack-fixture",
        }
    }
}
