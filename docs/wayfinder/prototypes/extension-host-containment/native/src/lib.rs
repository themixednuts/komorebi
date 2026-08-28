pub mod child;
mod child_pipe;
pub mod fault_child;
pub mod harness;
pub mod protocol;

#[cfg(windows)]
pub mod windows;
#[cfg(all(test, windows))]
mod windows_tests;
