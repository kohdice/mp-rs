use std::fmt;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser as ClapParser, ValueEnum};
use mp_preview::{ColorMode, ParseError, PreviewError, RenderOptions};

/// Runs the CLI: renders the requested file to `stdout` and maps failures to exit codes.
///
/// Errors are reported on `stderr` with an `mp:` prefix; a broken pipe on `stdout` is
/// treated as success so piping into `head` and friends stays quiet.
pub fn run<W, E>(
    cli: &Cli,
    stdout: &mut W,
    stderr: &mut E,
    no_color: bool,
    stdout_is_terminal: bool,
) -> ExitCode
where
    W: Write,
    E: Write,
{
    let render = render_file(&cli.file, cli.color, no_color, stdout_is_terminal, stdout);
    // Flush buffered output before reporting, so a partial render reaches the terminal
    // ahead of any diagnostic even when rendering failed mid-stream. The flush runs
    // eagerly here; `and` then keeps the render error as the root cause if both fail.
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
#[command(
    name = "mp",
    version = env!("CARGO_PKG_VERSION"),
    about = "Preview Markdown in the terminal"
)]
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
    Parse { path: PathBuf, source: ParseError },
    WriteStdout(io::Error),
}

impl fmt::Display for CliError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read { path, source } => {
                write!(formatter, "unable to read '{}': {source}", path.display())
            }
            Self::Parse { path, source } => {
                write!(formatter, "unable to parse '{}': {source}", path.display())
            }
            Self::WriteStdout(source) => write!(formatter, "unable to write stdout: {source}"),
        }
    }
}

fn render_file<W>(
    path: &Path,
    color_policy: ColorPolicy,
    no_color: bool,
    stdout_is_terminal: bool,
    stdout: &mut W,
) -> Result<(), CliError>
where
    W: Write,
{
    let markdown = std::fs::read_to_string(path)
        .map_err(|source| CliError::Read { path: path.to_path_buf(), source })?;
    let options = RenderOptions {
        color: resolve_color_mode(color_policy, no_color, stdout_is_terminal),
        ..RenderOptions::default()
    };
    mp_preview::preview(&markdown, options, stdout).map_err(|error| match error {
        PreviewError::Parse(source) => CliError::Parse { path: path.to_path_buf(), source },
        PreviewError::Write(source) => CliError::WriteStdout(source),
    })
}

const fn resolve_color_mode(
    color_policy: ColorPolicy,
    no_color: bool,
    stdout_is_terminal: bool,
) -> ColorMode {
    match color_policy {
        // An explicit `--color always` wins over `NO_COLOR` (per no-color.org).
        ColorPolicy::Always => ColorMode::Ansi,
        ColorPolicy::Never => ColorMode::Plain,
        ColorPolicy::Auto if stdout_is_terminal && !no_color => ColorMode::Ansi,
        ColorPolicy::Auto => ColorMode::Plain,
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use clap::Parser as _;
    use mp_preview::ColorMode;

    use super::{Cli, ColorPolicy, render_file, resolve_color_mode, run};

    static TEMP_FILE_COUNTER: AtomicUsize = AtomicUsize::new(0);

    #[test]
    fn auto_with_no_color_set_downgrades_to_plain() {
        assert_eq!(resolve_color_mode(ColorPolicy::Auto, true, true), ColorMode::Plain);
    }

    #[test]
    fn explicit_always_outranks_no_color() {
        assert_eq!(resolve_color_mode(ColorPolicy::Always, true, true), ColorMode::Ansi);
    }

    #[test]
    fn render_file_terminates_output_with_a_newline_when_the_source_lacks_one() -> io::Result<()> {
        let file = write_temp_markdown("Hello")?;
        let mut output = Vec::new();
        render_file(&file, ColorPolicy::Never, no_color(), plain_terminal(), &mut output)
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

        let error = render_file(
            &missing_file,
            ColorPolicy::Never,
            no_color(),
            plain_terminal(),
            &mut output,
        )
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
        let mut stdout = CountingWriter::default();
        let mut stderr = Vec::new();

        let exit_code = run(&cli, &mut stdout, &mut stderr, no_color(), plain_terminal());

        assert_eq!(exit_code, std::process::ExitCode::SUCCESS);
        assert_eq!(stdout.flush_count, 1);
        assert!(stderr.is_empty());
        fs::remove_file(file)?;
        Ok(())
    }

    #[test]
    fn run_reports_stdout_write_errors() -> io::Result<()> {
        let file = write_temp_markdown("Hello\n")?;
        let cli = Cli { color: ColorPolicy::Never, file: file.clone() };
        let mut stdout = FailingWriter;
        let mut stderr = Vec::new();

        let exit_code = run(&cli, &mut stdout, &mut stderr, no_color(), plain_terminal());

        assert_eq!(exit_code, std::process::ExitCode::FAILURE);
        assert!(utf8(stderr)?.contains("unable to write stdout"));
        fs::remove_file(file)?;
        Ok(())
    }

    #[test]
    fn run_flushes_stdout_before_reporting_a_mid_stream_failure() -> io::Result<()> {
        let file = write_temp_markdown("a\n\nb\n")?;
        let cli = Cli { color: ColorPolicy::Never, file: file.clone() };
        let mut stdout = RecordingWriter { writes_before_failure: Some(1), ..Default::default() };
        let mut stderr = Vec::new();

        let exit_code = run(&cli, &mut stdout, &mut stderr, no_color(), plain_terminal());

        assert_eq!(exit_code, std::process::ExitCode::FAILURE);
        assert_eq!(
            stdout.log.last(),
            Some(&WriterEvent::Flush),
            "stdout must be flushed on the error path",
        );
        assert!(
            stdout.log.contains(&WriterEvent::Write),
            "the partial render must reach stdout before the flush",
        );
        assert!(utf8(stderr)?.contains("unable to write stdout"));
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

        let exit_code = run(&cli, &mut stdout, &mut stderr, no_color(), plain_terminal());

        assert_eq!(exit_code, std::process::ExitCode::SUCCESS);
        assert!(stderr.is_empty());
        fs::remove_file(file)?;
        Ok(())
    }

    fn plain_terminal() -> bool {
        false
    }

    fn no_color() -> bool {
        false
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

    #[derive(Default)]
    struct CountingWriter {
        bytes: Vec<u8>,
        flush_count: usize,
    }

    impl io::Write for CountingWriter {
        fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
            self.bytes.write(buffer)
        }

        fn flush(&mut self) -> io::Result<()> {
            self.flush_count += 1;
            self.bytes.flush()
        }
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

    #[derive(Debug, PartialEq, Eq)]
    enum WriterEvent {
        Write,
        Flush,
    }

    /// Records the order of write and flush calls, optionally failing once a write budget
    /// is exhausted, so tests can assert that stdout is flushed on the error path.
    #[derive(Default)]
    struct RecordingWriter {
        log: Vec<WriterEvent>,
        writes_before_failure: Option<usize>,
        writes: usize,
    }

    impl io::Write for RecordingWriter {
        fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
            self.log.push(WriterEvent::Write);
            self.writes += 1;
            if self.writes_before_failure.is_some_and(|limit| self.writes > limit) {
                return Err(io::Error::other("stdout exhausted"));
            }
            Ok(buffer.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            self.log.push(WriterEvent::Flush);
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
