fn main() -> std::io::Result<()> {
    let stdout = std::io::stdout();
    let stdout = stdout.lock();

    mp_core::write_preview(stdout)
}
