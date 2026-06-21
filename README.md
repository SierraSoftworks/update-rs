<div align="center">
  <img src="assets/logo.svg" alt="update-rs" width="440">

  <p><strong>Self-contained, in-place self-updates for Rust applications — download the new release, relaunch, and replace the running binary.</strong></p>

  <p>
    <a href="https://github.com/SierraSoftworks/update-rs/actions/workflows/ci.yml"><img src="https://github.com/SierraSoftworks/update-rs/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
    <a href="https://crates.io/crates/update-rs"><img src="https://img.shields.io/crates/v/update-rs.svg" alt="crates.io"></a>
    <a href="https://docs.rs/update-rs"><img src="https://img.shields.io/docsrs/update-rs" alt="docs.rs"></a>
    <a href="LICENSE"><img src="https://img.shields.io/github/license/SierraSoftworks/update-rs" alt="MIT License"></a>
  </p>
</div>

---

`update-rs` gives any Rust application a built-in "update yourself" command —
no installer, package manager, or external updater process required. It
downloads the newest release for the current platform, replaces the running
binary on disk, and relaunches into the new version.

This crate was extracted from the Sierra Softworks
[Git-Tool](https://github.com/SierraSoftworks/git-tool) project, where it powers
the `gt update` command. The design is described in
[*Building self-updating applications*](https://sierrasoftworks.com/2019/10/15/app-updates/#cleanup).

## How it works: the three-phase update

A running executable can't reliably overwrite itself — on Windows the running
image is locked, and on every platform you don't want to pull the binary out
from under a process that's mid-flight. So the update runs across three phases,
each executing from a **different** binary and relaunching the next:

1. **Prepare** — the running application downloads the new release to a
   temporary file next to it (`yourapp-<tag>` in the temp directory), verifies it
   against the SHA-256 digest GitHub reports for the asset, marks it executable
   on Unix, then launches that temporary binary.
2. **Replace** — the temporary binary deletes the original application file and
   copies itself over it, retrying with a short backoff while the old process
   exits, then launches the freshly replaced original.
3. **Cleanup** — the updated original deletes the leftover temporary binary and
   returns control to your application.

The phases are threaded together by relaunching the binary with a flag
(`update_rs::RESUME_FLAG`) followed by a serialized `UpdateState`. Your `main()`
detects the flag and hands control back to the library.

```text
running app ──prepare──▶ temp binary ──replace──▶ updated app ──cleanup──▶ done
   (download)              (overwrite original)        (remove temp file)
```

## Features

- **Self-relaunching three-phase updater** that works even when the OS won't let
  a process overwrite its own image.
- **Pluggable release sources** — implement the single `Source` trait, or use
  the built-in [`GitHubSource`](https://docs.rs/update-rs/latest/update_rs/struct.GitHubSource.html).
- **Glob-based asset selection** — point a pattern at your release assets and
  name them however you like; there's no required naming scheme, with `naming`
  helpers for the common Go and Rust conventions.
- **SemVer-aware** release listing, with a configurable tag prefix (`v` → `1.2.3`).
- **Verified downloads** — when GitHub reports a SHA-256 digest for an asset, the
  download is checked against it before the binary is swapped in, so a corrupted
  or tampered artifact is rejected.
- **Customisable relaunch** — swap in a custom `Launcher` to control exactly how
  the relaunch command is built: change how the update state is encoded (e.g. a
  sub-command instead of the default resume flag), or thread your own arguments
  and environment variables through to the next process.
- **Friendly errors** — every failure carries a description and actionable advice,
  powered by [`human-errors`](https://crates.io/crates/human-errors).
- **Observability** — diagnostics via the [`log`](https://crates.io/crates/log)
  facade by default, or opt into [`tracing`](https://crates.io/crates/tracing)
  spans and propagate the OpenTelemetry trace context *through the update state*,
  so the three phases form a single distributed trace (see
  [Observability](#observability-log-tracing--opentelemetry)).
- **Async** (Tokio) and **cross-platform** (Windows, Linux, macOS), with
  first-class handling of the awkward Windows cases.

## Quick start

```shell
cargo add update-rs
```

Build an `UpdateManager` around a `Source`, detect the resume flag **before** any
other argument parsing, and otherwise offer the newest release:

```rust
use update_rs::{GitHubSource, Release, UpdateManager, RESUME_FLAG};

#[tokio::main]
async fn main() -> Result<(), update_rs::Error> {
    let manager = UpdateManager::new(
        // The second argument is a glob matched against your release asset names.
        // `naming::go` builds one for this platform, e.g. "yourapp-linux-amd64".
        GitHubSource::new("yourorg/yourapp", update_rs::naming::go("yourapp"))
            .with_release_tag_prefix("v"), // strips the leading v in vX.Y.Z tags
    );

    // The updater relaunches your application between phases, passing the
    // serialized update state after RESUME_FLAG. Detect it first and hand
    // control back to the library.
    let args: Vec<String> = std::env::args().collect();
    if let Some(i) = args.iter().position(|a| a == RESUME_FLAG) {
        if manager.resume_from_arg(&args[i + 1]).await? {
            return Ok(()); // a phase was launched; exit so it can take over
        }
    }

    // Otherwise, look for the newest release with a binary for this platform.
    let releases = manager.get_releases().await?;
    let latest = Release::get_latest(releases.iter().filter(|r| r.get_variant().is_some()));
    if let Some(latest) = latest {
        if manager.update(latest).await? {
            println!("Shutting down to complete the update.");
            return Ok(()); // exit promptly so the new binary can take over
        }
    }

    // ... your normal application logic ...
    Ok(())
}
```

Two parts of the contract are load-bearing:

- **Detect `RESUME_FLAG` before any other CLI parsing.** The library relaunches
  your binary with this flag, and the JSON value that follows it must be passed
  straight to `resume_from_arg`.
- **Exit immediately when `update` or `resume_from_arg` returns `Ok(true)`.** A
  follow-up phase has been launched in a separate process, and it needs your
  process to release the binary so it can replace it.

### Customising the relaunch

The updater relaunches your binary between phases through a `Launcher`. Install
your own with `with_launcher` to control exactly how the relaunch command is
built. Every trait method has a default, so you change just the part you need —
most commonly `resume_args`, which decides *how* the serialized state reaches the
relaunched process. For example, to hand it to an `update --state <json>`
sub-command (the convention Git-Tool uses) instead of the default resume flag:

```rust
use std::ffi::OsString;
use update_rs::{Launcher, UpdateManager};

struct SubcommandLauncher;
impl Launcher for SubcommandLauncher {
    fn resume_args(&self, state_json: &str) -> Vec<OsString> {
        vec!["update".into(), "--state".into(), state_json.into()]
    }
}

let manager = UpdateManager::new(source).with_launcher(Box::new(SubcommandLauncher));
```

Override `launch` instead for complete control over the relaunch command — for
instance to thread your own arguments or environment variables (a `--trace-context`
value, an `APP_UPDATING=1` flag, ...) through to the next process — reusing the
provided `resume_args`, `detach` and `spawn` helpers as needed. The default
launcher (`DefaultLauncher`) is unchanged: the resume flag plus a detached child.

(With the `opentelemetry` feature the trace context is already propagated for you
inside the update state — see [Observability](#observability-log-tracing--opentelemetry)
— so that case needs no wiring.)

## Observability (log, tracing & OpenTelemetry)

By default the crate emits its diagnostic events through the lightweight
[`log`](https://crates.io/crates/log) facade, which does nothing until your
application installs a logger. Two opt-in features build on that:

```toml
[dependencies]
update-rs = { version = "0.3", features = ["opentelemetry"] }
```

- **`tracing`** routes diagnostics through
  [`tracing`](https://crates.io/crates/tracing) instead of `log`, adding
  `#[instrument]` spans and structured events for each step of the update.
- **`opentelemetry`** (which implies `tracing`) carries the active OpenTelemetry
  trace context **inside the serialized `UpdateState`** — *not* as an extra
  command-line argument — so the three phases, which each run in a separate
  process, stitch together into one distributed trace.

There's nothing extra to wire up: detect `RESUME_FLAG` and call `resume_from_arg`
as usual, and the trace context rides along with the update state automatically.
The feature reads and writes only the **global** propagator
(`opentelemetry::global::get_text_map_propagator`), so your application stays in
full control of how — and whether — traces are exported; with no propagator
installed it is a no-op. The OpenTelemetry crates are pinned to the `0.32`/`0.33`
series so the global propagator is shared with a host on that series.

## Windows: avoiding UAC and "Error 740"

Windows has a legacy *installer-detection heuristic* that auto-requests UAC
elevation for any executable whose file name contains tokens like `update`,
`setup`, `install`, or `patch`. Because `update-rs` downloads the new release to
a temporary file named like `yourapp-<tag>.exe` — and because updater binaries
are often named similarly — the relaunched process can trigger an elevation
prompt. If elevation is unavailable or declined, `CreateProcess` fails with
`ERROR_ELEVATION_REQUIRED` (Win32 error **740**) and the update breaks.

The fix is to ship your binary with an application manifest declaring
`requestedExecutionLevel level="asInvoker"`, which opts the binary out of the
heuristic so it runs with the caller's token and never prompts. Since a *library*
crate can't embed a manifest into your executable, this is your binary's
responsibility — but `update-rs` ships ready-to-copy templates to make it a
two-minute job:

- [`examples/windows-manifest/app.exe.manifest`](examples/windows-manifest/app.exe.manifest) — the manifest (also enables long-path support and a UTF-8 active code page).
- [`examples/windows-manifest/build.rs`](examples/windows-manifest/build.rs) — a `build.rs` that embeds it with [`winresource`](https://crates.io/crates/winresource).
- [`examples/windows-manifest/README.md`](examples/windows-manifest/README.md) — step-by-step instructions.

> The same heuristic even affects Cargo's own test binaries (`yourapp-<hash>.exe`).
> This repository sets `__COMPAT_LAYER=RunAsInvoker` in
> [`.cargo/config.toml`](.cargo/config.toml) so `cargo test` can launch them.

## Setting up your release pipeline (consumer side)

`update-rs`'s own [`release.yml`](.github/workflows/release.yml) only publishes
the *library* to crates.io. The application that consumes it is responsible for
publishing the platform binaries that `GitHubSource` downloads. To make that work:

- Publish one binary per platform as a GitHub Release asset. You choose the
  naming scheme — just make sure the glob pattern you pass to `GitHubSource::new`
  matches the right asset on each platform. The `naming` helpers cover two common
  conventions out of the box:
  - `naming::go("yourapp")` → `yourapp-linux-amd64`, `yourapp-windows-amd64.exe`,
    `yourapp-darwin-arm64`, ... (Go's `GOOS`/`GOARCH` names);
  - `naming::rust("yourapp")` → `yourapp-x86_64-unknown-linux-gnu`,
    `yourapp-x86_64-pc-windows-msvc.exe`, ... (the Rust target triple).

  Or pass any glob yourself, e.g. `format!("yourapp-{OS}-{ARCH}{EXE_SUFFIX}")`
  with [`std::env::consts`], or `"*-linux-amd64"`.
- Tag releases as `vX.Y.Z` (or whatever you pass to `with_release_tag_prefix`).
  Tags that don't parse as a SemVer version after the prefix is stripped are
  silently ignored.

The [release pipeline guide](https://sierrasoftworks.github.io/update-rs/guide/release-pipeline.html)
has a complete, copy-pasteable GitHub Actions workflow that builds a binary per
target and uploads it as `<name>-<target-triple>[.exe]` (matched by
`naming::rust`), along with the matching `GitHubSource` configuration. Git-Tool's
[release workflow](https://github.com/SierraSoftworks/git-tool/blob/main/.github/workflows/release.yml)
is a worked example of the Go-style naming `naming::go` matches.

## Documentation

- API reference on [docs.rs](https://docs.rs/update-rs).
- Guides and walkthroughs on the [documentation website](https://sierrasoftworks.github.io/update-rs/).

## License

Licensed under the [MIT License](LICENSE).

Copyright © Sierra Softworks.
