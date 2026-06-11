use std::fmt;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser as ClapParser, ValueEnum};
use mp_preview::{ParseError, PreviewError, RenderOptions};

/// Runs the CLI: renders the requested file to `stdout` and maps failures to exit codes.
///
/// Errors are reported on `stderr` with an `mp:` prefix; a broken pipe on `stdout` is
/// treated as success so piping into `head` and friends stays quiet.
pub fn run<W, E>(cli: &Cli, stdout: &mut W, stderr: &mut E, stdout_is_terminal: bool) -> ExitCode
where
    W: Write,
    E: Write,
{
    match render_file(&cli.file, cli.color, stdout_is_terminal, stdout)
        .and_then(|()| stdout.flush().map_err(CliError::WriteStdout))
    {
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
    #[arg(long, value_enum, default_value_t = ColorPolicy::Auto)]
    color: ColorPolicy,
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
    stdout_is_terminal: bool,
    stdout: &mut W,
) -> Result<(), CliError>
where
    W: Write,
{
    let options = RenderOptions { ansi: resolve_color_policy(color_policy, stdout_is_terminal) };
    mp_preview::preview_file(path, options, stdout).map_err(|error| match error {
        PreviewError::Read(source) => CliError::Read { path: path.to_path_buf(), source },
        PreviewError::Parse(source) => CliError::Parse { path: path.to_path_buf(), source },
        PreviewError::Write(source) => CliError::WriteStdout(source),
    })
}

const fn resolve_color_policy(color_policy: ColorPolicy, stdout_is_terminal: bool) -> bool {
    match color_policy {
        ColorPolicy::Auto => stdout_is_terminal,
        ColorPolicy::Always => true,
        ColorPolicy::Never => false,
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::{Cli, ColorPolicy, render_file, run};

    static TEMP_FILE_COUNTER: AtomicUsize = AtomicUsize::new(0);

    #[test]
    fn render_file_terminates_output_with_a_newline_when_the_source_lacks_one() -> io::Result<()> {
        let file = write_temp_markdown("Hello")?;
        let mut output = Vec::new();
        render_file(&file, ColorPolicy::Never, plain_terminal(), &mut output)
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

        let error = render_file(&missing_file, ColorPolicy::Never, plain_terminal(), &mut output)
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

        let exit_code = run(&cli, &mut stdout, &mut stderr, plain_terminal());

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

        let exit_code = run(&cli, &mut stdout, &mut stderr, plain_terminal());

        assert_eq!(exit_code, std::process::ExitCode::FAILURE);
        assert!(utf8(stderr)?.contains("unable to write stdout"));
        fs::remove_file(file)?;
        Ok(())
    }

    #[test]
    fn run_treats_closed_stdout_pipe_as_success_without_diagnostic() -> io::Result<()> {
        let file = write_temp_markdown("Hello\n")?;
        let cli = Cli { color: ColorPolicy::Never, file: file.clone() };
        let mut stdout = BrokenPipeWriter;
        let mut stderr = Vec::new();

        let exit_code = run(&cli, &mut stdout, &mut stderr, plain_terminal());

        assert_eq!(exit_code, std::process::ExitCode::SUCCESS);
        assert!(stderr.is_empty());
        fs::remove_file(file)?;
        Ok(())
    }

    fn plain_terminal() -> bool {
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
