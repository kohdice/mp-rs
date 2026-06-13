# mp-rs

Command to preview Markdown in the terminal.

## Usage

```bash
cargo run -p mp -- <file.md>
```

`mp` reads a Markdown file, renders it for the terminal
(ANSI colors when stdout is a terminal, plain text otherwise),
and writes the result to stdout.

When stdout is a terminal, the output adapts to its width: paragraphs, headings, list
items, and blockquotes reflow at word boundaries, and tables shrink their widest columns
(wrapping cell content onto multiple lines) to fit, down to a small per-column minimum
below which a very wide table can still overflow. Code blocks are never wrapped. When
stdout is piped or redirected, no width limit applies and the content keeps its natural
width.

## Workspace layout

The project is a Cargo workspace whose responsibilities are split across crates so that
each layer depends only on the layer below it:

| Crate                | Responsibility                                                                                              |
| -------------------- | ----------------------------------------------------------------------------------------------------------- |
| `crates/mp`          | CLI binary: argument parsing, terminal detection, error-to-exit-code mapping. Depends only on `mp-preview`. |
| `crates/mp-preview`  | Use-case layer: composes parsing and rendering (`preview`).                                                 |
| `crates/mp-parser`   | Markdown-to-block parsing (streaming `blocks` iterator).                                                    |
| `crates/mp-renderer` | Block-to-terminal rendering, including the trailing-newline guarantee.                                      |
| `crates/mp-ast`      | Shared AST data types.                                                                                      |
