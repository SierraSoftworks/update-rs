//! Self-contained, in-place self-updates for Rust applications.
//!
//! `update-rs` lets a Rust application replace its own binary on disk with a
//! newer release and relaunch into it — without an installer, package manager,
//! or external updater process. It was extracted from the Sierra Softworks
//! [Git-Tool](https://github.com/SierraSoftworks/git-tool) project, where it
//! powers the `gt update` command.
//!
//! # The three-phase update
//!
//! A running executable can't reliably overwrite itself (Windows holds an
//! exclusive lock on a running image), so the update is performed across three
//! phases, each running from a *different* binary and relaunching the next:
//!
//! 1. **Prepare** — the running application downloads the new release to a
//!    temporary file next to it, marks it executable (on Unix), and launches
//!    that temporary binary to perform the next phase.
//! 2. **Replace** — the temporary binary deletes the original application file
//!    and copies itself over it (retrying while the old process exits), then
//!    launches the freshly replaced original.
//! 3. **Cleanup** — the updated original deletes the leftover temporary binary
//!    and returns control to the application.
//!
//! The phases are threaded together by relaunching the binary with
//! [`RESUME_FLAG`] followed by a serialized [`UpdateState`]. This design is
//! described in more detail in
//! [*Building self-updating applications*](https://sierrasoftworks.com/2019/10/15/app-updates/#cleanup).
//!
//! # Quick start
//!
//! Build an [`UpdateManager`] around a [`Source`] (the crate ships
//! [`GitHubSource`]), detect the [`RESUME_FLAG`] **before** any other argument
//! parsing, and otherwise offer the newest release. The asset to download is
//! chosen by a glob pattern; the [`naming`] helpers build one for the current
//! platform.
//!
//! ```no_run
//! use update_rs::{naming, GitHubSource, Release, UpdateManager, RESUME_FLAG};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), update_rs::Error> {
//!     let manager = UpdateManager::new(
//!         // e.g. matches "yourapp-linux-amd64", "yourapp-windows-amd64.exe", ...
//!         GitHubSource::new("yourorg/yourapp", naming::go("yourapp"))
//!             .with_release_tag_prefix("v"), // strips the leading v in vX.Y.Z tags
//!     );
//!
//!     // The updater relaunches your application between phases, passing the
//!     // serialized update state after `RESUME_FLAG`. Detect it first and hand
//!     // control back to the library.
//!     let args: Vec<String> = std::env::args().collect();
//!     if let Some(i) = args.iter().position(|a| a == RESUME_FLAG) {
//!         if manager.resume_from_arg(&args[i + 1]).await? {
//!             return Ok(()); // a phase was launched; exit so it can take over
//!         }
//!     }
//!
//!     // Otherwise, look for the newest release with a binary for this platform.
//!     let releases = manager.get_releases().await?;
//!     let latest = Release::get_latest(releases.iter().filter(|r| r.get_variant().is_some()));
//!     if let Some(latest) = latest {
//!         if manager.update(latest).await? {
//!             println!("Shutting down to complete the update.");
//!             return Ok(()); // exit promptly so the new binary can take over
//!         }
//!     }
//!
//!     // ... your normal application logic ...
//!     Ok(())
//! }
//! ```
//!
//! # Selecting the release asset
//!
//! [`GitHubSource::new`] takes a glob pattern (`*` and `?` wildcards) that is
//! matched against each release's asset file names, so your project can name its
//! assets however it likes. Build the pattern by hand —
//! `format!("yourapp-{OS}-{ARCH}{EXE_SUFFIX}")` using [`std::env::consts`] — or
//! with a [`naming`] helper: [`naming::go`] for Go-style names
//! (`yourapp-linux-amd64`) or [`naming::rust`] for the Rust target triple
//! (`yourapp-x86_64-unknown-linux-gnu`).
//!
//! # Windows: avoiding UAC / Error 740
//!
//! Because the temporary binary is named like `yourapp-<tag>.exe`, Windows'
//! installer-detection heuristic can decide it's an installer and demand UAC
//! elevation — which fails the relaunch with `ERROR_ELEVATION_REQUIRED`
//! (Win32 error 740). The fix is to ship your binary with an `asInvoker`
//! application manifest. This is a property of the consuming *binary* (a library
//! can't embed a manifest), so `update-rs` ships a ready-to-copy `build.rs` and
//! manifest template under `examples/windows-manifest/`; see the project README
//! for details.

mod cmd;
mod fs;
mod glob;
mod manager;
pub mod naming;
mod release;
mod source;
mod state;

pub use human_errors::Error;
pub use manager::UpdateManager;
pub use release::{Release, ReleaseVariant};
pub use source::{GitHubSource, Source};
pub use state::{UpdatePhase, UpdateState};

/// The command-line flag the library uses to relaunch the consuming binary
/// between update phases.
///
/// A consuming `main()` **must** detect this flag (before any other argument
/// parsing), pass the JSON value that follows it to
/// [`UpdateManager::resume_from_arg`], and exit immediately if it returns
/// `Ok(true)` — so that the next phase's process can take ownership of the
/// binary.
pub const RESUME_FLAG: &str = "--update-resume-internal";

/// The Rust target triple this crate was compiled for (e.g.
/// `x86_64-unknown-linux-gnu`), captured at build time. Used by
/// [`naming::rust`] to build release asset names.
pub const TARGET: &str = env!("UPDATE_RS_TARGET");
