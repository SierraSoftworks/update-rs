# Usage

## A complete `main()`

```rust
use update_rs::{GitHubSource, Release, UpdateManager, RESUME_FLAG};

#[tokio::main]
async fn main() -> Result<(), update_rs::Error> {
    let manager = UpdateManager::new(
        // The second argument is a glob matched against your release asset names;
        // `naming::go` builds one for this platform (e.g. "yourapp-linux-amd64").
        GitHubSource::new("yourorg/yourapp", update_rs::naming::go("yourapp"))
            .with_release_tag_prefix("v"), // strips the leading v in vX.Y.Z tags
    );

    // 1. Hand control back to the library if we were relaunched to continue an
    //    update. This MUST come before any other argument parsing.
    let args: Vec<String> = std::env::args().collect();
    if let Some(i) = args.iter().position(|a| a == RESUME_FLAG) {
        if manager.resume_from_arg(&args[i + 1]).await? {
            return Ok(()); // a phase was launched; exit so it can take over
        }
    }

    // 2. Otherwise, look for the newest release with a binary for this platform.
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

## The contract

Two things are load-bearing and easy to get wrong:

- **Detect `RESUME_FLAG` before any other CLI parsing.** The library relaunches
  your binary with `--update-resume-internal <state-json>`. If your normal
  argument parser sees that flag first it will likely error out, breaking the
  update mid-way. Check for it up front and pass the following value straight to
  [`resume_from_arg`](https://docs.rs/update-rs/latest/update_rs/struct.UpdateManager.html#method.resume_from_arg).
- **Exit immediately when `update` or `resume_from_arg` returns `Ok(true)`.** A
  `true` result means a follow-up phase has been launched in a separate process,
  which now needs your process to release the binary so it can replace it. Don't
  keep running.

## Configuring the source

[`GitHubSource`](https://docs.rs/update-rs/latest/update_rs/struct.GitHubSource.html)
takes the `owner/name` repository and a glob pattern that selects which release
asset to download for the current platform:

```rust,ignore
use std::env::consts::{ARCH, EXE_SUFFIX, OS};
use update_rs::{naming, GitHubSource};

// Using a naming helper (recommended):
let source = GitHubSource::new("yourorg/yourapp", naming::go("yourapp"));

// ...or an explicit name built from the standard consts:
let source = GitHubSource::new("yourorg/yourapp", format!("yourapp-{OS}-{ARCH}{EXE_SUFFIX}"));

// ...or any glob:
let source = GitHubSource::new("yourorg/yourapp", "*-linux-amd64");
```

| Method | Purpose |
| --- | --- |
| `new(repo, pattern)` | The `owner/name` repo and a glob (`*`/`?` wildcards) matched against asset file names to choose the download. |
| `with_release_tag_prefix(p)` | Strip `p` from each Git tag before parsing it as a SemVer version (e.g. `"v"` for `vX.Y.Z`). |
| `with_github_endpoints(web, api)` | Point at a GitHub Enterprise instance, or a mock server in tests. |

The [`naming`](https://docs.rs/update-rs/latest/update_rs/naming/index.html)
helpers build a pattern for the current platform: `naming::go("yourapp")` for
Go-style names (`yourapp-linux-amd64`) and `naming::rust("yourapp")` for the Rust
target triple (`yourapp-x86_64-unknown-linux-gnu`).

If you need to update a binary other than the currently running executable, use
`UpdateManager::with_target_application`.

## Bring your own source

`GitHubSource` is just one implementation of the
[`Source`](https://docs.rs/update-rs/latest/update_rs/trait.Source.html) trait.
Implement the trait yourself to fetch releases from anywhere — a custom HTTP
endpoint, an object store, or a local directory:

```rust,ignore
#[async_trait::async_trait]
impl update_rs::Source for MySource {
    async fn get_releases(&self) -> Result<Vec<update_rs::Release>, update_rs::Error> {
        // ...
    }

    async fn get_binary<W: std::io::Write + Send>(
        &self,
        release: &update_rs::Release,
        variant: &update_rs::ReleaseVariant,
        into: &mut W,
    ) -> Result<(), update_rs::Error> {
        // ...
    }
}
```
