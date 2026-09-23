//! CLI entry points for the `mp` binary: argument parsing, rendering policy based
//! on supplied environment facts, and error-to-exit-code mapping.

mod cli;

pub use cli::{Cli, Env, run};
