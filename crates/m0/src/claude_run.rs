//! The Claude runner lives in `bff-core`. This wrapper keeps the harness recording each child's identity.

pub use bff_core::claude_run::{ClaudeRun, RunSpec};
use std::io;

pub fn run(spec: &RunSpec) -> io::Result<ClaudeRun> {
    bff_core::claude_run::run_recorded(spec, &|id| crate::common::record_proc("claude", id))
}
