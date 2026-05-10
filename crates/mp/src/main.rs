use clap::Parser;
use std::io::{BufWriter, IsTerminal};

fn main() -> std::process::ExitCode {
    let cli = mp::Cli::parse();

    let stdout = std::io::stdout();
    let stdout_is_terminal = stdout.is_terminal();
    let mut stdout = BufWriter::new(stdout.lock());

    let stderr = std::io::stderr();
    let mut stderr = stderr.lock();

    mp::run(&cli, &mut stdout, &mut stderr, stdout_is_terminal)
}
