use std::ffi::OsStr;
use std::fmt;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser as ClapParser, ValueEnum};
use mp_preview::{ColorMode, Options};

/// Facts about the execution environment, detected once at the binary edge
/// (`main.rs`) and passed down so the rest of the CLI stays free of
/// process-global reads.
#[derive(Debug, Clone, Copy, Default)]
pub struct Env {
    /// `NO_COLOR` is set to a non-empty value (per no-color.org).
    pub no_color: bool,
    /// Whether stdout is attached to a terminal.
    pub stdout_is_terminal: bool,
    /// Display width of the stdout terminal in columns; `None` when stdout is
    /// not a terminal or its size cannot be determined, which renders without
    /// a width limit.
    pub stdout_width: Option<usize>,
}

/// Whether `value`, the value of the `NO_COLOR` environment variable, asks for
/// uncolored output: an empty value counts as unset (per no-color.org).
pub fn no_color_requested(value: Option<&OsStr>) -> bool {
    value.is_some_and(|value| !value.is_empty())
}

/// Turns the reported stdout terminal width into a wrapping limit. Some ptys
/// report zero when the size is unknown, so zero means no limit, like an
/// undeterminable size.
pub fn usable_width(width: Option<u16>) -> Option<usize> {
    width.filter(|width| *width > 0).map(usize::from)
}

/// Runs the CLI: writes the requested file to `stdout`, rendered or unchanged according to
/// the flags and `env`, and maps failures to exit codes.
///
/// Errors are reported on `stderr` with an `mp:` prefix; a broken pipe on `stdout` is
/// treated as success so piping into `head` and friends stays quiet.
pub fn run<W, E>(cli: &Cli, mut stdout: W, mut stderr: E, env: Env) -> ExitCode
where
    W: Write,
    E: Write,
{
    let render = render_file(&cli.file, resolve_output(cli, env), env.stdout_width, &mut stdout);
    // Flush before reporting so a partial render reaches the terminal ahead of any
    // diagnostic. `and_then` would skip the flush after a render error; `and` runs it
    // and still reports the render error as the root cause when both fail.
    let flush = stdout.flush().map_err(CliError::WriteStdout);
    match render.and(flush) {
        Ok(()) => ExitCode::SUCCESS,
        Err(CliError::WriteStdout(error)) if error.kind() == io::ErrorKind::BrokenPipe => {
            ExitCode::SUCCESS
        }
        Err(error) => {
            let _ = writeln!(stderr, "mp: {}", ErrorChain(&error));
            ExitCode::FAILURE
        }
    }
}

/// Command-line arguments of the `mp` binary.
#[derive(Debug, ClapParser)]
#[command(name = "mp", version, about = "Preview Markdown in the terminal")]
pub struct Cli {
    /// When to render for the terminal: auto (stdout is a terminal), always, or
    /// never. When not rendering, the file is written unchanged, like `cat`.
    #[arg(long, value_enum, default_value_t = When::Auto)]
    render: When,
    /// When to colorize the rendering: auto (a terminal, unless NO_COLOR), always,
    /// or never. Has no effect when not rendering.
    #[arg(long, value_enum, default_value_t = When::Auto)]
    color: When,
    /// Alias for `--render always --color always`; keeps the rendering when piping
    /// into a program such as `less -R`. Cannot be combined with `--render` or
    /// `--color`.
    #[arg(short = 'f', long, conflicts_with_all = ["render", "color"])]
    force_render: bool,
    /// Path to the Markdown file to preview.
    file: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum When {
    Auto,
    Always,
    Never,
}

/// What `mp` writes to stdout, decided once from the flags and the environment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Output {
    /// The file contents, byte for byte.
    PassThrough,
    /// The terminal rendering, with or without ANSI styling.
    Render(ColorMode),
}

#[derive(Debug, thiserror::Error)]
enum CliError {
    #[error("unable to read '{}'", path.display())]
    Read {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("unable to write stdout")]
    WriteStdout(#[source] io::Error),
}

/// Displays `error` followed by each of its sources, separated by `: `.
struct ErrorChain<'a>(&'a dyn std::error::Error);

impl fmt::Display for ErrorChain<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.0)?;
        let mut source = self.0.source();
        while let Some(error) = source {
            write!(formatter, ": {error}")?;
            source = error.source();
        }
        Ok(())
    }
}

fn render_file<W>(
    path: &Path,
    output: Output,
    width: Option<usize>,
    mut stdout: W,
) -> Result<(), CliError>
where
    W: Write,
{
    let read_error = |source| CliError::Read { path: path.to_path_buf(), source };
    match output {
        // Read raw bytes so passing through works like `cat` even for files that
        // are not valid UTF-8; only rendering needs the contents as text.
        Output::PassThrough => {
            let bytes = std::fs::read(path).map_err(read_error)?;
            stdout.write_all(&bytes)
        }
        Output::Render(color) => {
            let markdown = std::fs::read_to_string(path).map_err(read_error)?;
            mp_preview::preview(&markdown, &Options { width, color }, stdout)
        }
    }
    .map_err(CliError::WriteStdout)
}

const fn resolve_output(cli: &Cli, env: Env) -> Output {
    let (render_policy, color_policy) =
        if cli.force_render { (When::Always, When::Always) } else { (cli.render, cli.color) };
    let render = match render_policy {
        When::Always => true,
        When::Never => false,
        When::Auto => env.stdout_is_terminal,
    };
    if render { Output::Render(resolve_color_mode(color_policy, env)) } else { Output::PassThrough }
}

const fn resolve_color_mode(color_policy: When, env: Env) -> ColorMode {
    match color_policy {
        // An explicit `--color always` wins over `NO_COLOR` (per no-color.org).
        When::Always => ColorMode::Ansi,
        When::Never => ColorMode::Plain,
        When::Auto if env.stdout_is_terminal && !env.no_color => ColorMode::Ansi,
        When::Auto => ColorMode::Plain,
    }
}

#[cfg(test)]
mod tests {
    use mp_preview::ColorMode;

    use super::{Env, When, resolve_color_mode};

    #[test]
    fn auto_with_no_color_set_downgrades_to_plain() {
        assert_eq!(
            resolve_color_mode(
                When::Auto,
                Env { no_color: true, stdout_is_terminal: true, ..Env::default() }
            ),
            ColorMode::Plain
        );
    }

    #[test]
    fn explicit_always_outranks_no_color() {
        assert_eq!(
            resolve_color_mode(
                When::Always,
                Env { no_color: true, stdout_is_terminal: true, ..Env::default() }
            ),
            ColorMode::Ansi
        );
    }
}
