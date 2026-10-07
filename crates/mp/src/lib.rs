//! CLI entry points for the `mp` binary: argument parsing, the render and color
//! policies based on supplied environment facts (rendering on a terminal, writing the
//! file unchanged otherwise), and error-to-exit-code mapping.

mod cli;

pub use cli::{Cli, Env, run};
