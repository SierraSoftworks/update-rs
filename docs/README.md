---
home: true
title: Home
heroImage: /icon.svg
heroText: update-rs
tagline: Self-contained, in-place self-updates for Rust applications.

actions:
    - text: Get Started
      link: /guide/
      type: primary
    - text: View on Docs.rs
      link: https://docs.rs/update-rs
      type: secondary

features:
    - title: Three-phase updates
      details: |
        Replace a running binary safely with a prepare → replace → cleanup
        handover, even when the OS won't let a process overwrite its own image.

    - title: Pluggable sources
      details: |
        Ships a configurable GitHub releases source, or implement the single
        Source trait to fetch updates from anywhere you like.

    - title: Windows-ready
      details: |
        First-class handling of the Windows installer-detection heuristic, so
        self-update never trips an unexpected UAC prompt or Error 740.
---

`update-rs` gives any Rust application a built-in "update yourself" command — no
installer, package manager, or external updater process required. It downloads
the newest release for the current platform, replaces the running binary on
disk, and relaunches into the new version.

```rust
use update_rs::{naming, GitHubSource, Release, UpdateManager, RESUME_FLAG};

#[tokio::main]
async fn main() -> Result<(), update_rs::Error> {
    let manager = UpdateManager::new(
        // The second argument is a glob matched against your release asset
        // names; `naming::go` builds one for this platform.
        GitHubSource::new("yourorg/yourapp", naming::go("yourapp"))
            .with_release_tag_prefix("v"),
    );

    let args: Vec<String> = std::env::args().collect();
    if let Some(i) = args.iter().position(|a| a == RESUME_FLAG) {
        if manager.resume_from_arg(&args[i + 1]).await? {
            return Ok(());
        }
    }

    let releases = manager.get_releases().await?;
    let latest = Release::get_latest(releases.iter().filter(|r| r.get_variant().is_some()));
    if let Some(latest) = latest {
        if manager.update(latest).await? {
            return Ok(());
        }
    }

    Ok(())
}
```

This library was extracted from the Sierra Softworks
[Git-Tool](https://github.com/SierraSoftworks/git-tool) project, where it powers
the `gt update` command. The design is described in
[*Building self-updating applications*](https://sierrasoftworks.com/2019/10/15/app-updates/#cleanup).
