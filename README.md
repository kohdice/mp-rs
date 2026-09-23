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
below which a very wide table can still overflow. Code and HTML blocks are never
wrapped. Prefixes, indivisible wide characters, and thematic breaks (at least one
column) can also exceed a narrow width. Reflow converts tabs to spaces and drops
separator spaces at line edges. When stdout is piped or redirected, no width limit
applies and the content keeps its natural width.
