# mp-rs

Command to preview Markdown in the terminal.

## Usage

```bash
cargo run -p mp -- <file.md>
```

`mp` reads a Markdown file, renders it for the terminal (ANSI colors when stdout is a
terminal, plain text otherwise), and writes the result to stdout. Non-empty output always
ends with exactly one trailing newline.

## Workspace layout

The project is a Cargo workspace whose responsibilities are split across crates so that
each layer depends only on the layer below it:

| Crate | Responsibility |
| --- | --- |
| `crates/mp` | CLI binary: argument parsing, terminal detection, error-to-exit-code mapping. Depends only on `mp-preview`. |
| `crates/mp-preview` | Use-case layer: composes parsing and rendering (`preview` / `preview_file`). |
| `crates/mp-parser` | Markdown-to-block parsing (streaming `blocks` iterator). |
| `crates/mp-renderer` | Block-to-terminal rendering, including the trailing-newline guarantee. |
| `crates/mp-ast` | Shared AST data types. |
