#![windows_subsystem = "windows"]

use std::num::NonZeroU32;

use clap::Parser;
use native_appbar_lifecycle_prototype::child::{self, ChildOptions};
use native_appbar_lifecycle_prototype::protocol::EdgeArg;

#[derive(Parser)]
struct Args {
    #[arg(long, value_enum, default_value_t = EdgeArg::Right)]
    edge: EdgeArg,
    #[arg(long, default_value = "13")]
    thickness: NonZeroU32,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    child::run(ChildOptions {
        edge: args.edge.into(),
        thickness_dip: args.thickness,
    })
}
