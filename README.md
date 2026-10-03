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

When stdout is a terminal, the output adapts to its width: paragraphs, headings, list
items, and blockquotes reflow at word boundaries, and tables shrink their widest columns
(wrapping cell content onto multiple lines) to fit, down to a small per-column minimum
below which a very wide table can still overflow. A word wider than the available width
is split between characters, never inside one. Code and HTML blocks are never wrapped.
Prefixes, indivisible wide characters, and thematic breaks (at least one column) can also
exceed a narrow width. Reflow drops separator spaces at line edges. When stdout is piped
or redirected, no width limit applies and the content keeps its natural width.

### Rendering

- Blank lines follow the document structure, not the source: consecutive blocks are
  separated by one blank line however many the source has, and list items are separated
  by a blank line only when the list is loose.
- Links show their URL in parentheses after the text, unless the text already is the URL.
- Alerts render as blockquotes whose first line is the alert label or its custom title.
- Tabs in code and HTML blocks expand to 4-column tab stops; elsewhere a tab, or a line
  break written as a reference such as `&#10;`, renders as one space.
- Other control characters, including ones written as references such as `&#27;`, are
  shown as control pictures such as `␛`, and C1 control characters such as `&#155;` as
  `�`, so a document can never inject escape sequences into the terminal.

### Exit status

`mp` exits with 0 on success, including when stdout is closed early as in
`mp file.md | head`. When the file cannot be read or stdout cannot be written, it prints a
diagnostic prefixed with `mp:` on stderr and exits with 1. Malformed Markdown is never an
error.

## For developers

- [ARCHITECTURE.md](./ARCHITECTURE.md)
- [CONTRIBUTING.md](./CONTRIBUTING.md)
