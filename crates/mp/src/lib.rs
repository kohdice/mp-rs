//! CLI entry points for the `mp` binary: argument parsing, terminal detection, and
//! error-to-exit-code mapping.

mod cli;

pub use cli::{Cli, Env, run};
