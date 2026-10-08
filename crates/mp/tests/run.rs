//! The CLI, checked through [`mp::run`] on parsed arguments and supplied environment facts.
//! The two calculations `main` builds those facts with, [`mp::no_color_requested`] and
//! [`mp::usable_width`], are checked here too.

use std::cell::RefCell;
use std::error::Error;
use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::{AtomicUsize, Ordering};

use clap::Parser as _;
use clap::error::ErrorKind;
use mp::{Cli, Env, no_color_requested, run, usable_width};

type TestResult<T = ()> = Result<T, Box<dyn Error>>;

static TEMP_FILE_COUNTER: AtomicUsize = AtomicUsize::new(0);

#[test]
fn run_reports_an_unreadable_file_on_stderr_and_fails() -> TestResult {
    let missing_file = unique_temp_path();
    let cli = parse(&[], &missing_file)?;
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();

    let exit_code = run(&cli, &mut stdout, &mut stderr, Env::default());

    assert_eq!(exit_code, ExitCode::FAILURE);
    assert!(stdout.is_empty());
    let diagnostic = String::from_utf8(stderr)?;
    assert!(diagnostic.starts_with("mp: unable to read '"), "{diagnostic:?}");
    // The underlying I/O error follows the path, so users see why it failed.
    assert!(diagnostic.contains(&format!("'{}': ", missing_file.display())), "{diagnostic:?}");
    Ok(())
}

#[test]
fn run_flushes_stdout_after_rendering() -> TestResult {
    let file = TempMarkdown::new("# Hello\n")?;
    let cli = parse(&["--color", "never"], file.path())?;
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

    assert_eq!(exit_code, ExitCode::SUCCESS);
    assert_eq!(stdout.bytes, b"Hello\n");
    assert_eq!(log.borrow().last(), Some(&WriterEvent::Flush(OutputStream::Stdout)));
    assert!(stderr.is_empty());
    Ok(())
}

#[test]
fn run_wraps_to_the_detected_terminal_width() -> TestResult {
    let output = run_on(
        "hello world\n",
        &["--color", "never"],
        Env { stdout_is_terminal: true, stdout_width: Some(5), ..Env::default() },
    )?;

    assert_eq!(output, "hello\nworld\n");
    Ok(())
}

#[test]
fn run_does_not_wrap_without_a_detected_terminal_width() -> TestResult {
    let output = run_on(
        "hello world\n",
        &["--color", "never"],
        Env { stdout_is_terminal: true, stdout_width: None, ..Env::default() },
    )?;

    assert_eq!(output, "hello world\n");
    Ok(())
}

#[test]
fn run_flushes_stdout_before_reporting_a_mid_stream_failure() -> TestResult {
    let file = TempMarkdown::new("a\n\nb\n")?;
    let cli = parse(&["--render", "always", "--color", "never"], file.path())?;
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

    let exit_code = run(&cli, &mut stdout, &mut stderr, Env::default());

    assert_eq!(exit_code, ExitCode::FAILURE);
    assert_eq!(stdout.bytes, b"a");
    assert!(String::from_utf8(stderr.bytes)?.contains("unable to write stdout"));
    let events = log.borrow();
    let diagnostic = events
        .iter()
        .position(|event| *event == WriterEvent::Write(OutputStream::Stderr))
        .ok_or("missing stderr diagnostic")?;
    assert_eq!(
        events[..diagnostic].last(),
        Some(&WriterEvent::Flush(OutputStream::Stdout)),
        "partial stdout must be flushed before the stderr diagnostic",
    );
    Ok(())
}

#[test]
fn run_treats_closed_stdout_pipe_as_success_without_diagnostic() -> TestResult {
    let file = TempMarkdown::new("Hello\n")?;
    let cli = parse(&["--render", "always", "--color", "never"], file.path())?;
    let mut stdout = BrokenPipeWriter;
    let mut stderr = Vec::new();

    let exit_code = run(&cli, &mut stdout, &mut stderr, Env::default());

    assert_eq!(exit_code, ExitCode::SUCCESS);
    assert!(stderr.is_empty());
    Ok(())
}

#[test]
fn run_passes_the_file_through_unchanged_when_stdout_is_piped() -> TestResult {
    let markdown = "# Title\n\n| a | b |\n| --- | --- |\n| 1 | 2 |";
    let file = TempMarkdown::new(markdown)?;
    let cli = parse(&[], file.path())?;
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();

    let exit_code = run(&cli, &mut stdout, &mut stderr, Env::default());

    assert_eq!(exit_code, ExitCode::SUCCESS);
    assert_eq!(String::from_utf8(stdout)?, markdown);
    assert!(stderr.is_empty());
    Ok(())
}

#[test]
fn run_passes_non_utf8_bytes_through_unchanged() -> TestResult {
    let bytes = b"caf\xe9\n";
    let file = TempMarkdown::new(bytes)?;
    let cli = parse(&["--render", "never"], file.path())?;
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();

    let exit_code = run(&cli, &mut stdout, &mut stderr, Env::default());

    assert_eq!(exit_code, ExitCode::SUCCESS);
    assert_eq!(stdout, bytes);
    assert!(stderr.is_empty(), "{}", String::from_utf8_lossy(&stderr));
    Ok(())
}

#[test]
fn run_renders_when_stdout_is_a_terminal() -> TestResult {
    let output = run_on("# Title\n", &[], Env { stdout_is_terminal: true, ..Env::default() })?;

    assert!(output.contains("Title"), "{output:?}");
    assert!(!output.contains("# Title"), "{output:?}");
    assert!(output.contains("\x1b["), "{output:?}");
    Ok(())
}

#[test]
fn run_passes_the_file_through_on_a_terminal_with_render_never() -> TestResult {
    let output = run_on(
        "# Title\n",
        &["--render", "never"],
        Env { stdout_is_terminal: true, ..Env::default() },
    )?;

    assert_eq!(output, "# Title\n");
    Ok(())
}

#[test]
fn run_renders_plain_on_a_pipe_with_render_always() -> TestResult {
    let output = run_on("# Title\n", &["--render", "always"], Env::default())?;

    assert_eq!(output, "Title\n");
    Ok(())
}

#[test]
fn run_renders_with_ansi_on_a_pipe_with_render_always_and_color_always() -> TestResult {
    let output = run_on("# Title\n", &["--render", "always", "--color", "always"], Env::default())?;

    assert!(output.contains("Title"), "{output:?}");
    assert!(!output.contains("# Title"), "{output:?}");
    assert!(output.contains("\x1b["), "{output:?}");
    Ok(())
}

#[test]
fn run_ignores_color_when_not_rendering() -> TestResult {
    let output = run_on("# Title\n", &["--render", "never", "--color", "always"], Env::default())?;

    assert_eq!(output, "# Title\n");
    Ok(())
}

#[test]
fn run_force_render_renders_with_ansi_on_a_pipe_despite_no_color() -> TestResult {
    for flag in ["-f", "--force-render"] {
        let output = run_on("# Title\n", &[flag], Env { no_color: true, ..Env::default() })?;

        assert!(output.contains("Title"), "{flag}: {output:?}");
        assert!(!output.contains("# Title"), "{flag}: {output:?}");
        assert!(output.contains("\x1b["), "{flag}: {output:?}");
    }
    Ok(())
}

#[test]
fn no_color_is_requested_only_by_a_non_empty_value() {
    assert!(!no_color_requested(None));
    assert!(!no_color_requested(Some(OsStr::new(""))));
    assert!(no_color_requested(Some(OsStr::new("1"))));
}

#[test]
fn usable_width_treats_zero_and_unknown_as_no_width() {
    assert_eq!(usable_width(Some(0)), None);
    assert_eq!(usable_width(Some(80)), Some(80));
    assert_eq!(usable_width(None), None);
}

#[test]
fn force_render_rejects_an_explicit_render_or_color_flag() {
    for flag in ["--render", "--color"] {
        let Err(error) = parse(&["-f", flag, "never"], Path::new("ignored.md")) else {
            panic!("`-f {flag}` unexpectedly parsed");
        };
        assert_eq!(error.kind(), ErrorKind::ArgumentConflict, "-f {flag}");
    }
}

/// Runs `mp <flags> <file>` on a temporary file holding `markdown` and returns
/// stdout, failing unless the run succeeds without a diagnostic.
fn run_on(markdown: &str, flags: &[&str], env: Env) -> TestResult<String> {
    let file = TempMarkdown::new(markdown)?;
    let cli = parse(flags, file.path())?;
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();

    let exit_code = run(&cli, &mut stdout, &mut stderr, env);

    if exit_code != ExitCode::SUCCESS || !stderr.is_empty() {
        return Err(format!(
            "mp failed: {exit_code:?}, stderr: {}",
            String::from_utf8_lossy(&stderr)
        )
        .into());
    }
    Ok(String::from_utf8(stdout)?)
}

/// Parses `mp <flags> <file>` through clap, as the binary does.
fn parse(flags: &[&str], file: &Path) -> clap::error::Result<Cli> {
    let args = std::iter::once(OsStr::new("mp"))
        .chain(flags.iter().map(OsStr::new))
        .chain(std::iter::once(file.as_os_str()));
    Cli::try_parse_from(args)
}

fn unique_temp_path() -> PathBuf {
    let id = TEMP_FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("mp-rs-test-{}-{id}.md", std::process::id()))
}

/// A Markdown file in the temporary directory, removed when dropped so a failing
/// assertion leaves nothing behind.
struct TempMarkdown(PathBuf);

impl TempMarkdown {
    fn new(contents: impl AsRef<[u8]>) -> io::Result<Self> {
        let path = unique_temp_path();
        fs::write(&path, contents)?;
        Ok(Self(path))
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempMarkdown {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
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
