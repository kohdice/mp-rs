//! Binary entry point: wires standard streams and terminal detection into [`mp::run`].

use clap::Parser;
use std::io::{BufWriter, IsTerminal};

fn main() -> std::process::ExitCode {
    let cli = mp::Cli::parse();

    let stdout = std::io::stdout();
    let stdout_is_terminal = stdout.is_terminal();
    let stdout_width = mp::usable_width(
        terminal_size::terminal_size_of(&stdout).map(|(terminal_size::Width(width), _)| width),
    );
    let mut stdout = BufWriter::new(stdout.lock());

    let stderr = std::io::stderr();
    let mut stderr = stderr.lock();

    let no_color = mp::no_color_requested(std::env::var_os("NO_COLOR").as_deref());

    mp::run(&cli, &mut stdout, &mut stderr, mp::Env { no_color, stdout_is_terminal, stdout_width })
}
