# Contributing to mp-rs

Thank you for considering contributing to **mp-rs**!

Your contributions are what make this project great, whether it's through:

- reporting issues
- suggesting features
- submitting pull requests

Together, we can make **mp-rs** even better!

## Table of Contents

1. [Developer Guide](#developer-guide)
   - [Setup](#setup)
   - [Project Layout](#project-layout)
   - [Build and Test](#build-and-test)
   - [Benchmarking](#benchmarking)
2. [How to Contribute](#how-to-contribute)
   - [Issues](#issues)
   - [Pull Requests](#pull-requests)
   - [Documentation](#documentation)
3. [Git Commit Guidelines](#git-commit-guidelines)
   - [Type](#type)

## Developer Guide

### Setup

```bash
# With direnv
direnv allow

# Without direnv
nix develop
```

### Project Layout

The workspace has two crates: `crates/mp` (the CLI binary) and `crates/mp-preview` (the
rendering library). Crate responsibilities, the dependency graph, and the invariants the
code relies on are documented in [ARCHITECTURE.md](./ARCHITECTURE.md).

### Build and Test

Recipes live in the `justfile`; run `just --list` to see them all.

```bash
just check                   # Format and lint
just test                    # Run all unit and integration tests
```

Mermaid tests in `crates/mp-preview/tests/preview/mermaid.rs` assert the exact drawing when a chart
has at most four nodes and a dozen rows: a small drawing is stable and the strongest check.
Larger charts use the structural helpers in that file instead, since a layout change may
legitimately move their coordinates.

CI runs `just check-ci` and `just test` for pushes to `main` and for non-draft pull requests
that touch code, build, or CI files (the `paths` filters in `.github/workflows/ci.yml`);
documentation-only changes do not start it.
Make sure `just check` and `just test` pass locally before pushing.

### Benchmarking

Build the release binary used for benchmarking:

```bash
cargo build --release
```

The compiled binary lives at `./target/release/mp`. Benchmark `mp` directly with `hyperfine` against any sample Markdown file you provide:

```bash
hyperfine \
  --warmup 3 \
  --min-runs 10 \
  --command-name mp "./target/release/mp 'path/to/sample.md' > /dev/null"
```

To compare `mp` against `cat`, run:

```bash
hyperfine \
  --warmup 3 \
  --min-runs 10 \
  --command-name cat "cat 'path/to/sample.md' > /dev/null" \
  --command-name mp "./target/release/mp 'path/to/sample.md' > /dev/null"
```

`hyperfine` is provided by the Nix dev shell (`flake.nix`); run `nix develop` (or `direnv allow`) first if it is not on your `PATH`.

## How to Contribute

### Issues

Feel free to create issues for any bugs, feature requests, or questions.
Please provide as much detail as possible to help others understand the context and the problem.

It is recommended to include the following:

- A clear title and description.
- Steps to reproduce the issue (if applicable).
- Relevant logs, screenshots, or error messages.

### Pull Requests

Pull requests for improvements, bug fixes, or new features are always welcome.
Please follow these steps:

1. Fork the repository and create a new branch.
2. Make your changes and write clear, descriptive commit messages.
3. Run `just check` and `just test` locally to ensure nothing is broken.
4. Submit a pull request and include:
   - A detailed description of your changes.
   - A reference to any related issues (e.g., "Fixes #443").

### Documentation

Contributions to documentation are highly valued.
If you find anything unclear or outdated, please consider improving it or adding a new section if necessary.

## Git Commit Guidelines

### Type

Commit messages must follow one of the following types:

- **feat**: A new feature
- **fix**: A bug fix
- **refactor**: A code change that neither fixes a bug nor adds a feature
- **test**: Adding missing or correcting existing tests
- **style**: Changes that do not affect the meaning of the code (e.g., white-space, formatting, missing semi-colons)
- **chore**: Changes to the build process or auxiliary tools and libraries such as documentation generation
- **docs**: Documentation only changes
- **ci**: Changes to CI configuration files and scripts
- **perf**: A code change that improves performance

---

Thank you for your contributions, big or small, in making **mp-rs** better!
