# ARCHITECTURE

This document is a bird's-eye view of how mp-rs is put together. It describes what each
crate is responsible for, which direction dependencies flow, and the design invariants
that cannot be read from `Cargo.toml` alone. It deliberately omits module- and
function-level detail; read the crate-root and module doc comments (`//!`) and the code
for that.

## Bird's-eye view

`mp` reads a Markdown file, hands the text to `mp-preview`, and writes the rendered result
to stdout. File-system access, terminal detection, and the color policy live in the binary;
parsing, layout, and ANSI encoding are library code that never touch the file system.

Inside `mp-preview` the text flows through four stages. Each stage is a pure function from
its input to its output, and only the last step writes anything:

```
Markdown text ──> markdown ──> model ──> layout ──> ansi ──> preview writes to `out`
                  (comrak)     (Block)   (Lines of   (String)
                                          Spans)
```

1. `markdown` parses the whole document with comrak and converts it into an owned
   `Vec<Block>`; the comrak arena is dropped before returning.
2. `layout` turns one `Block` into lines of styled spans, applying the width limit, list
   markers, quote bars, table borders, and syntax highlighting. A fenced block whose info
   string's first word, cut before its first `,`, is `mermaid` is first handed from
   `layout/code.rs` to `diagram` (`crates/mp-preview/src/diagram.rs`), a crate-level
   module of pure calculations (parse, layer, route, draw onto a character canvas); when
   the diagram cannot be drawn, the block is laid out as an ordinary code block.
3. `ansi` encodes those lines as text, emitting escape sequences only in `ColorMode::Ansi`.
4. `preview` writes each encoded block to the caller's writer before laying out the next one.

## Crates

The project is a Cargo workspace whose members live under `crates/*`.

| Crate               | Responsibility                                                                                                                 |
| ------------------- | ------------------------------------------------------------------------------------------------------------------------------ |
| `crates/mp`         | CLI binary: argument parsing, `NO_COLOR` and terminal detection, file reading, error-to-exit-code mapping.                     |
| `crates/mp-preview` | Library: Markdown parsing, terminal layout, Mermaid flowchart drawing, and ANSI encoding behind the single `preview` function. |

## Dependency graph

Dependencies flow in one direction, from the binary to the library, and never form a cycle.
Run `cargo tree --workspace` for the exact graph including external crates.

```
mp ──> mp-preview
```

`mp` is the only binary crate. It depends on `mp-preview` and on nothing else inside the
workspace, so the library is the single entry point into rendering code.

## Invariants

These are guarantees the code relies on; changing one requires updating its consumers.

- **Library code performs no file-system access.** `preview` takes the Markdown text and a
  writer, so the caller (the binary) owns reading the file and can attach path-bearing
  error context.
- **Parsing is infallible.** comrak always produces a well-formed tree, so `preview` has no
  parse error path; its only error is the first one reported by the writer.
- **Parse whole, write per block.** `markdown::parse` converts the entire document into
  `Vec<Block>` up front, then `preview` lays out and writes one top-level block at a time.
  Memory is bounded by the model plus the largest block's lines, and blocks written before
  a writer failure stay written, which lets the binary flush partial output ahead of the
  diagnostic.
- **Spacing comes from structure.** Exactly one blank line separates consecutive top-level
  blocks and the child blocks of a blockquote; list items get a blank line only when the
  list is loose. Blank-line counts in the source are never reproduced.
- **Styles are data.** `layout` produces lines of spans carrying a `Style`; `ansi` is the
  only place that emits escape sequences, and nothing reads ANSI bytes back. Styles never
  carry across lines.
- **External libraries stay at the edges.** Only `markdown` names a comrak type, only
  `highlight` names a syntect type, and only the flowchart `label` module reads the
  `entities` table of HTML named character references. The comrak arena never escapes
  `markdown::parse`.
- **Text is safe once it leaves `markdown`.** Inline text, inline code, URLs, titles, alert
  titles, and code-fence info strings contain no line breaks or tabs: LF, CR, and HT become
  one space. Code and HTML block bodies keep LF and HT, with CR and CRLF normalized to LF.
  Everywhere, the remaining C0 control characters and DEL are replaced with Unicode control
  pictures and C1 control characters with U+FFFD, including characters decoded from
  references, so later stages can print text verbatim. `diagram` decodes Mermaid entity
  codes (`#27;`) in labels and runs the result through the same replacement
  (`control::visualize_control`), so a drawing is as safe as the text it came from.
- **Non-empty output ends with exactly one trailing newline.** `preview` appends the newline
  to every encoded block, and empty input writes nothing.
- **Width handling is the library's job.** The binary only detects whether stdout is a
  terminal and how wide it is; every wrapping and table-shrinking decision is made in
  `layout` from the `Options` it receives.
- **Mermaid diagrams follow upstream Mermaid.** `diagram` accepts the syntax Mermaid
  accepts, rejects what Mermaid rejects, and applies Mermaid's limits, so a block that
  renders in a browser renders here and vice versa. It deviates only where character
  output cannot reproduce Mermaid's result (shapes, curves, label placement), and every
  such deviation is stated in `README.md` next to the feature it affects.
