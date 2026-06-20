# Working on `update-rs`

Guidance for humans and coding agents contributing to this crate. `CLAUDE.md`
is a symlink to this file, so both names point at the same instructions.

`update-rs` is a small, self-contained library that lets a Rust application
replace its own binary on disk with a newer release and relaunch into it. The
update is performed across three phases — **prepare → replace → cleanup** — each
running from a *different* binary and relaunching the next, because a running
executable cannot reliably overwrite itself (especially on Windows). The phases
are threaded together by relaunching the binary with
[`RESUME_FLAG`](src/lib.rs) followed by a serialized `UpdateState`.

## Design goals — keep these in mind for every change

1. **Standalone and engine-agnostic.** This crate was lifted out of Git-Tool;
   it must not depend on any host application's types. Releases are fetched
   through the `Source` trait (the crate ships `GitHubSource`, which owns its own
   `reqwest::Client` and exposes configurable endpoints for testing). Don't
   reintroduce coupling to a shared HTTP client, telemetry stack, or config
   object.
2. **Friendly errors.** Every fallible path returns a `human_errors::Error`
   (re-exported as `update_rs::Error`) carrying a description and concrete
   advice. `advice` is `&'static [&'static str]`; only the description may be a
   runtime `format!`. There is no blanket `From<io::Error>`, so wrap each
   fallible call explicitly with `ResultExt::{wrap_user_err, wrap_system_err}`
   or `OptionExt::{ok_or_user_err, ok_or_system_err}`.
3. **Testable state machine.** The filesystem (`fs::FileSystem`) and process
   launcher (`cmd::Launcher`) sit behind `#[cfg_attr(test, automock)]` traits so
   the `prepare/replace/cleanup` flow can be unit-tested with `mockall` mocks and
   `tempfile`, and the HTTP layer can be tested against a `wiremock` server via
   `GitHubSource::with_github_endpoints`. Keep these seams.
4. **Cross-platform, Windows-first.** The whole point of the crate is to make
   self-update work everywhere, including the awkward Windows cases. Preserve the
   `#[cfg(windows)]` detached-process spawn flags, the `#[cfg(unix)]` `chmod`
   of the downloaded binary, and the read-only pre-check.

## Before you push — reproduce CI locally

CI (`.github/workflows/ci.yml`) has three jobs (lint, test on
Linux/Windows/macOS, docs). Run the full set below and make sure it's clean:

```sh
cargo fmt --all                                            # then: git add the result
cargo fmt --all --check                                    # Lint job, step 1
cargo clippy --all-targets --all-features -- -D warnings   # Lint job, step 2 (warnings are errors)
cargo test --no-fail-fast                                  # Test job, default features
cargo test --all-features --no-fail-fast                   # Test job, all features
RUSTDOCFLAGS="-D warnings" cargo doc --all-features --no-deps   # Docs job (doc warnings are errors)
```

Notes:
- **`cargo fmt`** is non-negotiable — CI runs `--check` and fails on any diff.
- **Clippy runs with `-D warnings`** — a warning fails the build.
- **Docs run with `RUSTDOCFLAGS=-D warnings`** — broken intra-doc links and other
  rustdoc warnings fail the build. Doc examples are part of the test suite
  (`cargo test --doc`); keep the crate-level example compiling.
- **Windows / Error 740:** the test binaries are named `update_rs-<hash>.exe`,
  which trips Windows' installer-detection heuristic and fails to launch with
  `ERROR_ELEVATION_REQUIRED` (740). `.cargo/config.toml` sets
  `__COMPAT_LAYER=RunAsInvoker` so `cargo test` runs them with the caller's
  token — don't remove it. This is the same problem the crate helps consumers
  solve with an `asInvoker` manifest (see `examples/windows-manifest/`).

## Repository layout

| Path | Purpose |
| --- | --- |
| `src/lib.rs` | Crate docs, module wiring, public re-exports, `RESUME_FLAG`, `TARGET` |
| `src/manager.rs` | `UpdateManager<S>` and the `prepare/replace/cleanup` state machine |
| `src/source/mod.rs` | The `Source` trait |
| `src/source/github.rs` | `GitHubSource` (own `reqwest::Client`, glob asset selection) |
| `src/glob.rs` | Tiny dependency-free `*`/`?` glob matcher used to select assets |
| `src/naming.rs` | `naming::go` / `naming::rust` asset-name helpers |
| `src/fs.rs` | `FileSystem` trait + retrying `DefaultFileSystem` (`#[automock]`) |
| `src/cmd.rs` | `Launcher` trait + `DefaultLauncher` (relaunch with `RESUME_FLAG`) |
| `src/release.rs` | `Release` and `ReleaseVariant` (a selected release asset) |
| `src/state.rs` | `UpdateState`, `UpdatePhase` (serde) |
| `build.rs` | Captures the target triple into `UPDATE_RS_TARGET` for `naming::rust` |
| `examples/windows-manifest/` | Copy-paste `build.rs` + manifest for consumers (Error 740) |
| `assets/` | Logo and icon SVGs |
| `docs/` | VuePress documentation website (deployed to GitHub Pages) |

## Conventions

- **Edition 2024, MSRV `1.88`** (`Cargo.toml` `rust-version`). Don't use APIs
  newer than the MSRV.
- **A library owns no runtime.** Keep `tokio` to the minimal feature set in
  `[dependencies]`; runtime features (`macros`, `rt-multi-thread`) belong in
  `[dev-dependencies]` only.
- **Match the surrounding style** — comment density, naming, and idioms. Favour
  explanatory comments on non-obvious decisions.
- **Commits** end with the trailer used across this repo:
  `Co-Authored-By: <author>` when pair-authored with an agent.
