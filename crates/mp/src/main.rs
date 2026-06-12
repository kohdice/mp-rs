//! Binary entry point: wires standard streams and terminal detection into [`mp::run`].

use clap::Parser;
use std::io::{BufWriter, IsTerminal};

fn main() -> std::process::ExitCode {
    let cli = mp::Cli::parse();

    let stdout = std::io::stdout();
    let stdout_is_terminal = stdout.is_terminal();
    let stdout_width = terminal_size::terminal_size_of(&stdout)
        .map(|(terminal_size::Width(width), _)| usize::from(width));
    let mut stdout = BufWriter::new(stdout.lock());

    let stderr = std::io::stderr();
    let mut stderr = stderr.lock();

    // `NO_COLOR` set to any non-empty value disables color (per no-color.org).
    let no_color = std::env::var_os("NO_COLOR").is_some_and(|value| !value.is_empty());

    mp::run(&cli, &mut stdout, &mut stderr, mp::Env { no_color, stdout_is_terminal, stdout_width })
}
