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

/// Runs the CLI: renders the requested file to `stdout` and maps failures to exit codes.
///
/// Errors are reported on `stderr` with an `mp:` prefix; a broken pipe on `stdout` is
/// treated as success so piping into `head` and friends stays quiet.
pub fn run<W, E>(cli: &Cli, stdout: &mut W, stderr: &mut E, env: Env) -> ExitCode
where
    W: Write,
    E: Write,
{
    let render = render_file(&cli.file, cli.color, env, stdout);
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
            let _ = writeln!(stderr, "mp: {error}");
            ExitCode::FAILURE
        }
    }
}

/// Command-line arguments of the `mp` binary.
#[derive(Debug, ClapParser)]
#[command(name = "mp", version, about = "Preview Markdown in the terminal")]
pub struct Cli {
    /// When to colorize output: auto (a terminal, unless NO_COLOR), always, or never.
    #[arg(long, value_enum, default_value_t = ColorPolicy::Auto)]
    color: ColorPolicy,
    /// Path to the Markdown file to preview.
    file: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum ColorPolicy {
    Auto,
    Always,
    Never,
}

#[derive(Debug)]
enum CliError {
    Read { path: PathBuf, source: io::Error },
    WriteStdout(io::Error),
}

impl fmt::Display for CliError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read { path, source } => {
                write!(formatter, "unable to read '{}': {source}", path.display())
            }
            Self::WriteStdout(source) => write!(formatter, "unable to write stdout: {source}"),
        }
    }
}

impl std::error::Error for CliError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Read { source, .. } => Some(source),
            Self::WriteStdout(source) => Some(source),
        }
    }
}

fn render_file<W>(
    path: &Path,
    color_policy: ColorPolicy,
    env: Env,
    stdout: &mut W,
) -> Result<(), CliError>
where
    W: Write,
{
    let markdown = std::fs::read_to_string(path)
        .map_err(|source| CliError::Read { path: path.to_path_buf(), source })?;
    let options = Options { width: env.stdout_width, color: resolve_color_mode(color_policy, env) };
    mp_preview::preview(&markdown, &options, stdout).map_err(CliError::WriteStdout)
}

const fn resolve_color_mode(color_policy: ColorPolicy, env: Env) -> ColorMode {
    match color_policy {
        // An explicit `--color always` wins over `NO_COLOR` (per no-color.org).
        ColorPolicy::Always => ColorMode::Ansi,
        ColorPolicy::Never => ColorMode::Plain,
        ColorPolicy::Auto if env.stdout_is_terminal && !env.no_color => ColorMode::Ansi,
        ColorPolicy::Auto => ColorMode::Plain,
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::fs;
    use std::io;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use clap::Parser as _;
    use mp_preview::ColorMode;

    use super::{Cli, ColorPolicy, Env, render_file, resolve_color_mode, run};

    static TEMP_FILE_COUNTER: AtomicUsize = AtomicUsize::new(0);

    #[test]
    fn auto_with_no_color_set_downgrades_to_plain() {
        assert_eq!(
            resolve_color_mode(
                ColorPolicy::Auto,
                Env { no_color: true, stdout_is_terminal: true, ..Env::default() }
            ),
            ColorMode::Plain
        );
    }

    #[test]
    fn explicit_always_outranks_no_color() {
        assert_eq!(
            resolve_color_mode(
                ColorPolicy::Always,
                Env { no_color: true, stdout_is_terminal: true, ..Env::default() }
            ),
            ColorMode::Ansi
        );
    }

    #[test]
    fn render_file_terminates_output_with_a_newline_when_the_source_lacks_one() -> io::Result<()> {
        let file = write_temp_markdown("Hello")?;
        let mut output = Vec::new();
        render_file(&file, ColorPolicy::Never, plain_env(), &mut output)
            .map_err(|error| io::Error::other(error.to_string()))?;
        let output = utf8(output)?;

        assert!(output.ends_with('\n'), "expected trailing newline, got {output:?}");
        fs::remove_file(file)?;
        Ok(())
    }

    #[test]
    fn prints_a_clear_diagnostic_when_the_file_cannot_be_read() -> io::Result<()> {
        let missing_file = unique_temp_path();
        let mut output = Vec::new();

        let error = render_file(&missing_file, ColorPolicy::Never, plain_env(), &mut output)
            .err()
            .ok_or_else(|| io::Error::other("missing file unexpectedly rendered"))?;

        let diagnostic = error.to_string();
        assert!(diagnostic.contains("unable to read '"));
        assert!(diagnostic.contains(&missing_file.display().to_string()));
        Ok(())
    }

    #[test]
    fn run_flushes_stdout_after_rendering() -> io::Result<()> {
        let file = write_temp_markdown("Hello\n")?;
        let cli = Cli { color: ColorPolicy::Never, file: file.clone() };
        let log = RefCell::new(Vec::new());
        let mut stdout = RecordingWriter {
            log: &log,
            stream: OutputStream::Stdout,
            remaining_bytes: None,
            bytes: Vec::new(),
        };
        let mut stderr = Vec::new();

        let exit_code = run(&cli, &mut stdout, &mut stderr, plain_env());

        assert_eq!(exit_code, std::process::ExitCode::SUCCESS);
        assert_eq!(stdout.bytes, b"Hello\n");
        assert_eq!(log.borrow().last(), Some(&WriterEvent::Flush(OutputStream::Stdout)));
        assert!(stderr.is_empty());
        fs::remove_file(file)?;
        Ok(())
    }

    #[test]
    fn run_wraps_wide_tables_to_the_detected_terminal_width() -> io::Result<()> {
        let file = write_temp_markdown(WIDE_TABLE_MARKDOWN)?;
        let cli = Cli { color: ColorPolicy::Never, file: file.clone() };
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        let exit_code =
            run(&cli, &mut stdout, &mut stderr, Env { stdout_width: Some(40), ..Env::default() });

        assert_eq!(exit_code, std::process::ExitCode::SUCCESS);
        let output = utf8(stdout)?;
        assert!(
            output.lines().all(|line| line.chars().count() <= 40),
            "every line must fit in 40 columns: {output}"
        );
        fs::remove_file(file)?;
        Ok(())
    }

    #[test]
    fn run_leaves_piped_output_unwrapped_without_a_terminal_width() -> io::Result<()> {
        let file = write_temp_markdown(WIDE_TABLE_MARKDOWN)?;
        let cli = Cli { color: ColorPolicy::Never, file: file.clone() };
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        let exit_code = run(&cli, &mut stdout, &mut stderr, plain_env());

        assert_eq!(exit_code, std::process::ExitCode::SUCCESS);
        let output = utf8(stdout)?;
        assert!(
            output.lines().any(|line| line.chars().count() > 40),
            "piped output must keep the table's natural width: {output}"
        );
        fs::remove_file(file)?;
        Ok(())
    }

    const WIDE_TABLE_MARKDOWN: &str = "| Crate | Responsibility |\n\
         | --- | --- |\n\
         | mp-renderer | Block-to-terminal rendering with a trailing-newline guarantee |\n";

    #[test]
    fn run_reports_stdout_write_errors() -> io::Result<()> {
        let file = write_temp_markdown("Hello\n")?;
        let cli = Cli { color: ColorPolicy::Never, file: file.clone() };
        let mut stdout = FailingWriter;
        let mut stderr = Vec::new();

        let exit_code = run(&cli, &mut stdout, &mut stderr, plain_env());

        assert_eq!(exit_code, std::process::ExitCode::FAILURE);
        assert!(utf8(stderr)?.contains("unable to write stdout"));
        fs::remove_file(file)?;
        Ok(())
    }

    #[test]
    fn run_flushes_stdout_before_reporting_a_mid_stream_failure() -> io::Result<()> {
        let file = write_temp_markdown("a\n\nb\n")?;
        let cli = Cli { color: ColorPolicy::Never, file: file.clone() };
        let log = RefCell::new(Vec::new());
        let mut stdout = RecordingWriter {
            log: &log,
            stream: OutputStream::Stdout,
            remaining_bytes: Some(1),
            bytes: Vec::new(),
        };
        let mut stderr = RecordingWriter {
            log: &log,
            stream: OutputStream::Stderr,
            remaining_bytes: None,
            bytes: Vec::new(),
        };

        let exit_code = run(&cli, &mut stdout, &mut stderr, plain_env());

        assert_eq!(exit_code, std::process::ExitCode::FAILURE);
        assert_eq!(stdout.bytes, b"a");
        assert!(utf8(stderr.bytes)?.contains("unable to write stdout"));
        let events = log.borrow();
        let diagnostic = events
            .iter()
            .position(|event| *event == WriterEvent::Write(OutputStream::Stderr))
            .ok_or_else(|| io::Error::other("missing stderr diagnostic"))?;
        assert_eq!(
            events[..diagnostic].last(),
            Some(&WriterEvent::Flush(OutputStream::Stdout)),
            "partial stdout must be flushed before the stderr diagnostic",
        );
        fs::remove_file(file)?;
        Ok(())
    }

    #[test]
    fn long_help_describes_the_color_flag_and_file_argument() {
        use clap::CommandFactory;

        let help = Cli::command().render_long_help().to_string();

        assert!(help.contains("When to colorize output"), "missing --color help: {help}");
        assert!(help.contains("Path to the Markdown file to preview"), "missing FILE help: {help}");
    }

    #[test]
    fn misspelled_color_flag_suggests_the_correct_name() -> io::Result<()> {
        let error = match Cli::try_parse_from(["mp", "--colr", "always", "file.md"]) {
            Ok(_) => return Err(io::Error::other("expected a parse error for --colr")),
            Err(error) => error,
        };

        assert!(
            error.to_string().contains("--color"),
            "expected a suggestion for --color, got {error}",
        );
        Ok(())
    }

    #[test]
    fn run_treats_closed_stdout_pipe_as_success_without_diagnostic() -> io::Result<()> {
        let file = write_temp_markdown("Hello\n")?;
        let cli = Cli { color: ColorPolicy::Never, file: file.clone() };
        let mut stdout = BrokenPipeWriter;
        let mut stderr = Vec::new();

        let exit_code = run(&cli, &mut stdout, &mut stderr, plain_env());

        assert_eq!(exit_code, std::process::ExitCode::SUCCESS);
        assert!(stderr.is_empty());
        fs::remove_file(file)?;
        Ok(())
    }

    /// Piped stdout: not a terminal, no known width, and `NO_COLOR` unset.
    fn plain_env() -> Env {
        Env::default()
    }

    fn write_temp_markdown(contents: &str) -> io::Result<PathBuf> {
        let path = unique_temp_path();
        fs::write(&path, contents)?;
        Ok(path)
    }

    fn unique_temp_path() -> PathBuf {
        let id = TEMP_FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!("mp-rs-test-{}-{id}.md", std::process::id()))
    }

    fn utf8(bytes: Vec<u8>) -> io::Result<String> {
        String::from_utf8(bytes).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
    }

    struct FailingWriter;

    impl io::Write for FailingWriter {
        fn write(&mut self, _buffer: &[u8]) -> io::Result<usize> {
            Err(io::Error::other("closed stdout"))
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum OutputStream {
        Stdout,
        Stderr,
    }

    #[derive(Debug, PartialEq, Eq)]
    enum WriterEvent {
        Write(OutputStream),
        Flush(OutputStream),
    }

    /// Shares an event log across stdout and stderr to expose their ordering.
    struct RecordingWriter<'a> {
        log: &'a RefCell<Vec<WriterEvent>>,
        stream: OutputStream,
        remaining_bytes: Option<usize>,
        bytes: Vec<u8>,
    }

    impl io::Write for RecordingWriter<'_> {
        fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
            if buffer.is_empty() {
                return Ok(0);
            }
            let accepted = if let Some(remaining) = &mut self.remaining_bytes {
                if *remaining == 0 {
                    return Err(io::Error::other("stdout exhausted"));
                }
                let accepted = buffer.len().min(*remaining);
                *remaining -= accepted;
                accepted
            } else {
                buffer.len()
            };
            self.log.borrow_mut().push(WriterEvent::Write(self.stream));
            self.bytes.write(&buffer[..accepted])
        }

        fn flush(&mut self) -> io::Result<()> {
            self.log.borrow_mut().push(WriterEvent::Flush(self.stream));
            Ok(())
        }
    }

    struct BrokenPipeWriter;

    impl io::Write for BrokenPipeWriter {
        fn write(&mut self, _buffer: &[u8]) -> io::Result<usize> {
            Err(io::Error::new(io::ErrorKind::BrokenPipe, "closed stdout"))
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
}
