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
    use std::cell::RefCell;
    use std::ffi::OsStr;
    use std::fs;
    use std::io;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};

    use clap::Parser as _;
    use clap::error::ErrorKind;
    use mp_preview::ColorMode;

    use super::{Cli, Env, When, resolve_color_mode, run};

    static TEMP_FILE_COUNTER: AtomicUsize = AtomicUsize::new(0);

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

    #[test]
    fn run_reports_an_unreadable_file_on_stderr_and_fails() -> io::Result<()> {
        let missing_file = unique_temp_path();
        let cli = parse(&[], &missing_file)?;
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        let exit_code = run(&cli, &mut stdout, &mut stderr, Env::default());

        assert_eq!(exit_code, std::process::ExitCode::FAILURE);
        assert!(stdout.is_empty());
        let diagnostic = utf8(stderr)?;
        assert!(diagnostic.starts_with("mp: unable to read '"), "{diagnostic:?}");
        // The underlying I/O error follows the path, so users see why it failed.
        assert!(diagnostic.contains(&format!("'{}': ", missing_file.display())), "{diagnostic:?}");
        Ok(())
    }

    #[test]
    fn run_flushes_stdout_after_rendering() -> io::Result<()> {
        let file = write_temp_markdown("# Hello\n")?;
        let cli = parse(&["--color", "never"], &file)?;
        let log = RefCell::new(Vec::new());
        let mut stdout = RecordingWriter {
            log: &log,
            stream: OutputStream::Stdout,
            remaining_bytes: None,
            bytes: Vec::new(),
        };
        let mut stderr = Vec::new();

        let exit_code =
            run(&cli, &mut stdout, &mut stderr, Env { stdout_is_terminal: true, ..Env::default() });

        assert_eq!(exit_code, std::process::ExitCode::SUCCESS);
        assert_eq!(stdout.bytes, b"Hello\n");
        assert_eq!(log.borrow().last(), Some(&WriterEvent::Flush(OutputStream::Stdout)));
        assert!(stderr.is_empty());
        fs::remove_file(file)?;
        Ok(())
    }

    #[test]
    fn run_wraps_to_the_detected_terminal_width() -> io::Result<()> {
        let output = run_on(
            "hello world\n",
            &["--color", "never"],
            Env { stdout_is_terminal: true, stdout_width: Some(5), ..Env::default() },
        )?;

        assert_eq!(output, "hello\nworld\n");
        Ok(())
    }

    #[test]
    fn run_does_not_wrap_without_a_detected_terminal_width() -> io::Result<()> {
        let output = run_on(
            "hello world\n",
            &["--color", "never"],
            Env { stdout_is_terminal: true, stdout_width: None, ..Env::default() },
        )?;

        assert_eq!(output, "hello world\n");
        Ok(())
    }

    #[test]
    fn run_flushes_stdout_before_reporting_a_mid_stream_failure() -> io::Result<()> {
        let file = write_temp_markdown("a\n\nb\n")?;
        let cli = parse(&["--render", "always", "--color", "never"], &file)?;
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
    fn run_treats_closed_stdout_pipe_as_success_without_diagnostic() -> io::Result<()> {
        let file = write_temp_markdown("Hello\n")?;
        let cli = parse(&["--render", "always", "--color", "never"], &file)?;
        let mut stdout = BrokenPipeWriter;
        let mut stderr = Vec::new();

        let exit_code = run(&cli, &mut stdout, &mut stderr, plain_env());

        assert_eq!(exit_code, std::process::ExitCode::SUCCESS);
        assert!(stderr.is_empty());
        fs::remove_file(file)?;
        Ok(())
    }

    #[test]
    fn run_passes_the_file_through_unchanged_when_stdout_is_piped() -> io::Result<()> {
        let markdown = "# Title\n\n| a | b |\n| --- | --- |\n| 1 | 2 |";
        let file = write_temp_markdown(markdown)?;
        let cli = parse(&[], &file)?;
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        let exit_code = run(&cli, &mut stdout, &mut stderr, Env::default());

        assert_eq!(exit_code, std::process::ExitCode::SUCCESS);
        assert_eq!(utf8(stdout)?, markdown);
        assert!(stderr.is_empty());
        fs::remove_file(file)?;
        Ok(())
    }

    #[test]
    fn run_passes_non_utf8_bytes_through_unchanged() -> io::Result<()> {
        let bytes = b"caf\xe9\n";
        let file = write_temp_markdown(bytes)?;
        let cli = parse(&["--render", "never"], &file)?;
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        let exit_code = run(&cli, &mut stdout, &mut stderr, plain_env());

        assert_eq!(exit_code, std::process::ExitCode::SUCCESS);
        assert_eq!(stdout, bytes);
        assert!(stderr.is_empty(), "{}", String::from_utf8_lossy(&stderr));
        fs::remove_file(file)?;
        Ok(())
    }

    #[test]
    fn run_renders_when_stdout_is_a_terminal() -> io::Result<()> {
        let output = run_on("# Title\n", &[], Env { stdout_is_terminal: true, ..Env::default() })?;

        assert!(output.contains("Title"), "{output:?}");
        assert!(!output.contains("# Title"), "{output:?}");
        assert!(output.contains("\x1b["), "{output:?}");
        Ok(())
    }

    #[test]
    fn run_passes_the_file_through_on_a_terminal_with_render_never() -> io::Result<()> {
        let output = run_on(
            "# Title\n",
            &["--render", "never"],
            Env { stdout_is_terminal: true, ..Env::default() },
        )?;

        assert_eq!(output, "# Title\n");
        Ok(())
    }

    #[test]
    fn run_renders_plain_on_a_pipe_with_render_always() -> io::Result<()> {
        let output = run_on("# Title\n", &["--render", "always"], Env::default())?;

        assert_eq!(output, "Title\n");
        Ok(())
    }

    #[test]
    fn run_renders_with_ansi_on_a_pipe_with_render_always_and_color_always() -> io::Result<()> {
        let output =
            run_on("# Title\n", &["--render", "always", "--color", "always"], Env::default())?;

        assert!(output.contains("Title"), "{output:?}");
        assert!(!output.contains("# Title"), "{output:?}");
        assert!(output.contains("\x1b["), "{output:?}");
        Ok(())
    }

    #[test]
    fn run_ignores_color_when_not_rendering() -> io::Result<()> {
        let output =
            run_on("# Title\n", &["--render", "never", "--color", "always"], Env::default())?;

        assert_eq!(output, "# Title\n");
        Ok(())
    }

    #[test]
    fn run_force_render_renders_with_ansi_on_a_pipe_despite_no_color() -> io::Result<()> {
        let output = run_on("# Title\n", &["-f"], Env { no_color: true, ..Env::default() })?;

        assert!(output.contains("Title"), "{output:?}");
        assert!(!output.contains("# Title"), "{output:?}");
        assert!(output.contains("\x1b["), "{output:?}");
        Ok(())
    }

    #[test]
    fn run_force_render_long_flag_matches_short_flag() -> io::Result<()> {
        let env = Env { no_color: true, ..Env::default() };

        let long = run_on("# Title\n", &["--force-render"], env)?;
        let short = run_on("# Title\n", &["-f"], env)?;

        assert_eq!(long, short);
        Ok(())
    }

    #[test]
    fn force_render_rejects_an_explicit_render_flag() -> io::Result<()> {
        let file = write_temp_markdown("# Title\n")?;

        let parsed = parse(&["-f", "--render", "never"], &file);

        fs::remove_file(file)?;
        let error =
            parsed.err().ok_or_else(|| io::Error::other("`-f --render` unexpectedly parsed"))?;
        assert_eq!(clap_error_kind(&error)?, ErrorKind::ArgumentConflict);
        Ok(())
    }

    #[test]
    fn force_render_rejects_an_explicit_color_flag() -> io::Result<()> {
        let file = write_temp_markdown("# Title\n")?;

        let parsed = parse(&["-f", "--color", "never"], &file);

        fs::remove_file(file)?;
        let error =
            parsed.err().ok_or_else(|| io::Error::other("`-f --color` unexpectedly parsed"))?;
        assert_eq!(clap_error_kind(&error)?, ErrorKind::ArgumentConflict);
        Ok(())
    }

    /// Runs `mp <flags> <file>` on a temporary file holding `markdown` and returns
    /// stdout, failing unless the run succeeds without a diagnostic.
    fn run_on(markdown: &str, flags: &[&str], env: Env) -> io::Result<String> {
        let file = write_temp_markdown(markdown)?;
        let cli = parse(flags, &file)?;
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        let exit_code = run(&cli, &mut stdout, &mut stderr, env);

        fs::remove_file(file)?;
        if exit_code != std::process::ExitCode::SUCCESS || !stderr.is_empty() {
            return Err(io::Error::other(format!(
                "mp failed: {exit_code:?}, stderr: {}",
                String::from_utf8_lossy(&stderr)
            )));
        }
        utf8(stdout)
    }

    /// Recovers the kind of the `clap::Error` that `parse` wrapped into an `io::Error`.
    fn clap_error_kind(error: &io::Error) -> io::Result<ErrorKind> {
        error
            .get_ref()
            .and_then(|inner| inner.downcast_ref::<clap::Error>())
            .map(clap::Error::kind)
            .ok_or_else(|| io::Error::other(format!("not a clap error: {error}")))
    }

    /// Parses `mp <flags> <file>` through clap, as the binary does.
    fn parse(flags: &[&str], file: &Path) -> io::Result<Cli> {
        let args = std::iter::once(OsStr::new("mp"))
            .chain(flags.iter().map(OsStr::new))
            .chain(std::iter::once(file.as_os_str()));
        Cli::try_parse_from(args).map_err(io::Error::other)
    }

    /// Piped stdout: not a terminal, no known width, and `NO_COLOR` unset.
    fn plain_env() -> Env {
        Env::default()
    }

    fn write_temp_markdown(contents: impl AsRef<[u8]>) -> io::Result<PathBuf> {
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
