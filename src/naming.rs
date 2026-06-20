//! Helpers for building release-asset name patterns for the current platform.
//!
//! [`GitHubSource`](crate::GitHubSource) selects the asset to download by
//! matching a glob pattern against each release's asset names. These helpers
//! produce a concrete asset name for the platform the application is running on,
//! following one of two common naming schemes, so you don't have to assemble the
//! pattern by hand:
//!
//! ```
//! use update_rs::{naming, GitHubSource};
//!
//! // Go-style: "myapp-linux-amd64", "myapp-windows-amd64.exe", ...
//! let go = GitHubSource::new("yourorg/yourapp", naming::go("myapp"));
//!
//! // Rust-style: "myapp-x86_64-unknown-linux-gnu", "myapp-x86_64-pc-windows-msvc.exe", ...
//! let rust = GitHubSource::new("yourorg/yourapp", naming::rust("myapp"));
//! # let _ = (go, rust);
//! ```
//!
//! The returned names contain no wildcards (they match exactly one asset), but
//! they are still valid glob patterns. If your release assets follow a different
//! layout, pass your own pattern to [`GitHubSource::new`](crate::GitHubSource::new)
//! directly — for example `format!("yourapp-{OS}-{ARCH}{EXE_SUFFIX}")` using
//! [`std::env::consts`], or a glob like `"*-linux-amd64"`.

use std::env::consts::{ARCH, EXE_SUFFIX, OS};

/// Build a Go-style asset name for the current platform:
/// `{prefix}-{os}-{arch}{exe}`, using Go's platform naming conventions
/// (`darwin`/`amd64`/`arm64`/...) and a `.exe` suffix on Windows.
///
/// For example, on Linux x86-64 this returns `myapp-linux-amd64`; on Windows
/// x86-64, `myapp-windows-amd64.exe`; on Apple Silicon, `myapp-darwin-arm64`.
/// This matches the convention used by, for example, Git-Tool.
pub fn go(prefix: &str) -> String {
    format!("{prefix}-{}-{}{EXE_SUFFIX}", go_os(OS), go_arch(ARCH))
}

/// Build a Rust-style asset name for the current platform:
/// `{prefix}-{target}{exe}`, using the full Rust target triple this crate was
/// compiled for (see [`crate::TARGET`]).
///
/// For example: `myapp-x86_64-unknown-linux-gnu`,
/// `myapp-x86_64-pc-windows-msvc.exe`, `myapp-aarch64-apple-darwin`.
pub fn rust(prefix: &str) -> String {
    format!("{prefix}-{}{EXE_SUFFIX}", crate::TARGET)
}

/// Translate Rust's [`std::env::consts::OS`] into Go's `GOOS` naming
/// (notably `macos` -> `darwin`).
fn go_os(os: &str) -> &str {
    match os {
        "macos" => "darwin",
        other => other,
    }
}

/// Translate Rust's [`std::env::consts::ARCH`] into Go's `GOARCH` naming
/// (e.g. `x86_64` -> `amd64`, `aarch64` -> `arm64`).
fn go_arch(arch: &str) -> &str {
    match arch {
        "x86_64" => "amd64",
        "i686" => "386",
        "aarch64" => "arm64",
        "powerpc64" => "ppc64",
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn go_starts_with_prefix_and_has_no_rust_arch() {
        let name = go("myapp");
        assert!(name.starts_with("myapp-"), "got {name}");
        // Go naming never contains the Rust spelling of common architectures.
        assert!(!name.contains("x86_64"), "got {name}");
        assert!(!name.contains("aarch64"), "got {name}");

        #[cfg(target_os = "macos")]
        assert!(name.contains("-darwin-"), "got {name}");
        #[cfg(target_os = "windows")]
        assert!(name.ends_with(".exe"), "got {name}");
        #[cfg(target_os = "linux")]
        assert!(
            name.contains("-linux-") && !name.ends_with(".exe"),
            "got {name}"
        );
    }

    #[test]
    fn rust_uses_the_target_triple() {
        let name = rust("myapp");
        assert!(name.starts_with("myapp-"), "got {name}");
        assert!(name.contains(crate::TARGET), "got {name}");
        // The Rust target triple embeds the Rust architecture spelling.
        assert!(name.contains(std::env::consts::ARCH), "got {name}");

        #[cfg(target_os = "windows")]
        assert!(name.ends_with(".exe"), "got {name}");
    }

    #[test]
    fn go_translations() {
        assert_eq!(go_os("macos"), "darwin");
        assert_eq!(go_os("linux"), "linux");
        assert_eq!(go_arch("x86_64"), "amd64");
        assert_eq!(go_arch("aarch64"), "arm64");
        assert_eq!(go_arch("riscv64"), "riscv64");
    }
}
