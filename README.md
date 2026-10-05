# mp-rs

Command to preview Markdown in the terminal.

## Usage

```bash
cargo run -p mp -- [--color <auto|always|never>] <file.md>
```

`mp` reads a Markdown file, renders it for the terminal, and writes the result to stdout.
It understands CommonMark plus the GitHub extensions for tables, strikethrough, task
lists, autolinks, and alerts (`> [!NOTE]`, `> [!TIP]`, `> [!IMPORTANT]`, `> [!WARNING]`,
`> [!CAUTION]`).

### Color

`--color` decides whether the output carries ANSI styling and syntax-highlighted code
blocks:

- `auto` (default): styled when stdout is a terminal and `NO_COLOR` is unset or empty
  (see [no-color.org](https://no-color.org)); plain text otherwise.
- `always`: styled even when stdout is piped or `NO_COLOR` is set.
- `never`: plain text.

### Width

When stdout is a terminal, text reflows and tables shrink to fit its width. Code and HTML
blocks are never wrapped. When stdout is piped or redirected, no width limit applies.

### Mermaid diagrams

A ` ```mermaid ` block holding a `flowchart` or `graph` diagram renders as boxes joined by
box-drawing lines, in place of the code block. The drawing is built from characters, so it
only approximates what a browser shows: shapes are made of box-drawing glyphs, lines run
horizontally and vertically only, and a diagram has no colors.

The block renders as a plain code block instead when the diagram is wider than the
terminal, when it is another diagram type, or when it uses a feature that is not supported
yet. A block that is not valid Mermaid also renders as a plain code block, with one line
above it naming the problem, such as `mermaid: line 2: unclosed node label`.

### Safety

Control characters in the document, including ones written as references such as `&#27;`,
are shown as control pictures such as `␛`, so a document can never inject escape sequences
into the terminal.

### Exit status

`mp` exits with 0 on success, including when stdout is closed early as in
`mp file.md | head`. When the file cannot be read or stdout cannot be written, it prints a
diagnostic prefixed with `mp:` on stderr and exits with 1. Malformed Markdown is never an
error.

## For developers

- [ARCHITECTURE.md](./ARCHITECTURE.md)
- [CONTRIBUTING.md](./CONTRIBUTING.md)
