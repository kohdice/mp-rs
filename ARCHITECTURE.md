# ARCHITECTURE

This document is a bird's-eye view of how mp-rs is put together. It describes what each
crate is responsible for, which direction dependencies flow, and the design invariants
that cannot be read from `Cargo.toml` alone. It deliberately omits module- and
function-level detail; read the crate-root doc comments (`//!`) and the code for that.

## Bird's-eye view

`mp` reads a Markdown file, parses it into a stream of blocks, renders each block for the
terminal, and writes the result to stdout. File-system access and terminal detection live
in the binary; parsing and rendering are library code that never touch the file system.

```
Markdown text ──> mp-parser ──> mp-ast blocks ──> mp-renderer ──> styled text ──> stdout
                  (streaming)                     (one block at a time)
```

## Crates

The project is a Cargo workspace whose members live under `crates/*`.

| Crate                | Responsibility                                                                                  |
| -------------------- | ----------------------------------------------------------------------------------------------- |
| `crates/mp`          | CLI binary: argument parsing, terminal detection, file reading, error-to-exit-code mapping.     |
| `crates/mp-preview`  | Use-case layer: composes parsing and rendering behind a single `preview` function.              |
| `crates/mp-parser`   | Markdown-to-block parsing on top of pulldown-cmark, exposed as the streaming `blocks` iterator. |
| `crates/mp-renderer` | Block-to-terminal rendering: styling, syntax highlighting, width-aware wrapping and tables.     |
| `crates/mp-ast`      | Shared AST data types produced by the parser and consumed by the renderer.                      |

## Dependency graph

Dependencies flow in one direction, from the binary down to the shared data types, and
never form a cycle. Run `cargo tree --workspace` for the exact graph including external
crates.

```
mp ──> mp-preview ──> mp-parser ──> mp-ast
                 └──> mp-renderer ──> mp-ast
                 └──> mp-ast
```

- `mp` is the only binary crate. It depends on `mp-preview` and on nothing else inside the
  workspace, so the use-case layer is the single entry point into library code.
- `mp-parser` and `mp-renderer` do not depend on each other. They share vocabulary only
  through `mp-ast`, which has no dependencies of its own.

## Invariants

These are guarantees the crates rely on; changing one requires updating its consumers.

- **Library crates perform no file-system access.** `preview` takes the Markdown text and
  a writer, so the caller (the binary) owns reading the file and can attach path-bearing
  error context.
- **Parsing is streaming.** `mp_parser::blocks` yields one block at a time so the renderer
  can write output without buffering the whole document.
- **Non-empty output ends with exactly one trailing newline.** `mp-renderer` enforces this
  in its finishing step, and `mp-preview` relies on it when composing the stream.
- **Width handling is the renderer's job.** The binary only detects whether stdout is a
  terminal and how wide it is; every wrapping and table-shrinking decision is made in
  `mp-renderer` from the options it receives.
