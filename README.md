# mp-rs

Command to preview Markdown in the terminal.

## Usage

```bash
mp [--render <auto|always|never>] [--color <auto|always|never>] [-f] <file.md>
```

`mp` reads a Markdown file and writes it to stdout: rendered for the terminal when stdout
is a terminal, unchanged otherwise (see [Rendering](#rendering)). It understands CommonMark plus the GitHub extensions for tables, strikethrough, task
lists, autolinks, and alerts (`> [!NOTE]`, `> [!TIP]`, `> [!IMPORTANT]`, `> [!WARNING]`,
`> [!CAUTION]`).

### Rendering

`--render` decides whether `mp` renders the file or writes it unchanged:

- `auto` (default): render only when stdout is a terminal; otherwise write the file
  unchanged, byte for byte, like `cat`. Piping `mp` into another program therefore passes
  the Markdown source through.
- `always`: render even when stdout is piped or redirected.
- `never`: write the file unchanged even on a terminal.

`-f, --force-render` is an alias for `--render always --color always` and cannot be
combined with `--render` or `--color`. Use it to keep the styled rendering when piping into
a pager:

```bash
mp -f file.md | less -R
```

### Color

`--color` decides whether the rendering carries ANSI styling and syntax-highlighted code
blocks. It has no effect when the file is passed through unchanged:

- `auto` (default): styled when stdout is a terminal and `NO_COLOR` is unset or empty
  (see [no-color.org](https://no-color.org)); plain text when the rendering goes to a pipe
  or a redirect.
- `always`: styled even when `NO_COLOR` is set; on a pipe or a redirect, combine it with
  `--render always`, or use `-f`.
- `never`: plain text.

### Width

When stdout is a terminal, text reflows and tables shrink to fit its width. Code and HTML
blocks are never wrapped. When the rendering goes to a pipe or a redirect (`--render
always` or `-f`), no width limit applies.

### Mermaid diagrams

A ` ```mermaid ` block holding a `flowchart` or `graph` diagram renders as boxes joined by
box-drawing lines, in place of the code block. The drawing is built from characters, so it
only approximates what a browser shows: shapes are made of box-drawing glyphs and lines run
horizontally and vertically only. Colors set with `style`, `classDef`, `class`, `:::` and
`linkStyle` are applied when color output is on.

The block renders as a plain code block instead when the diagram is wider than the
terminal, when it is another diagram type, or when it uses a feature that is not supported
yet. A block that is not valid Mermaid also renders as a plain code block, with one line
above it naming the problem, such as `mermaid: line 2: unclosed node label`.

### Safety

Control characters in the document, including ones written as references such as `&#27;`,
are shown as control pictures such as `␛`, so a document can never inject escape sequences
into the terminal. This protection applies to the rendering. When the file is passed through
unchanged (`--render never`, or the default on a pipe or a redirect), its bytes reach the
reader as they are, like `cat`.

### Exit status

`mp` exits with 0 on success, including when stdout is closed early as in
`mp file.md | head`. When the file cannot be read or stdout cannot be written, it prints a
diagnostic prefixed with `mp:` on stderr and exits with 1. Malformed Markdown is never an
error.

## For developers

- [ARCHITECTURE.md](./ARCHITECTURE.md)
- [CONTRIBUTING.md](./CONTRIBUTING.md)
