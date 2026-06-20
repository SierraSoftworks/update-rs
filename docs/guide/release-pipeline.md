# Your release pipeline

`update-rs` downloads release binaries that **your** project publishes. The
crate's own release workflow only publishes the library to crates.io — the
application that consumes it is responsible for building and uploading the
per-platform binaries that `GitHubSource` looks for.

## Naming your assets

There is **no required naming scheme**. You publish one binary per platform as a
GitHub Release asset, and configure a glob pattern (the second argument to
`GitHubSource::new`) that selects the right one on each platform. As long as the
pattern matches your asset on the running machine, any layout works.

The [`naming`](https://docs.rs/update-rs/latest/update_rs/naming/index.html)
helpers build a pattern for the two most common conventions:

| Helper | Example assets it matches |
| --- | --- |
| `naming::go("yourapp")` | `yourapp-linux-amd64`, `yourapp-windows-amd64.exe`, `yourapp-darwin-arm64`, ... (Go's `GOOS`/`GOARCH` names) |
| `naming::rust("yourapp")` | `yourapp-x86_64-unknown-linux-gnu`, `yourapp-x86_64-pc-windows-msvc.exe`, `yourapp-aarch64-apple-darwin`, ... (the Rust target triple) |

If neither fits, pass your own pattern — a name built from
[`std::env::consts`](https://doc.rust-lang.org/std/env/consts/) such as
`format!("yourapp-{OS}-{ARCH}{EXE_SUFFIX}")`, or a glob like `"*-linux-amd64"`.
The pattern supports `*` (any sequence) and `?` (single character); everything
else matches literally.

## Integrity verification

You don't need to do anything special to get verified downloads: GitHub computes
a SHA-256 digest for every release asset and returns it from the releases API.
`update-rs` captures it and checks the downloaded bytes against it before the
binary is swapped in, rejecting a corrupted or tampered artifact. Assets that
predate GitHub's digests (or sources that don't provide one) simply skip the
check.

## Tagging

Tag releases as `vX.Y.Z` (or whatever you pass to `with_release_tag_prefix`).
The prefix is stripped and the remainder is parsed as a
[SemVer](https://semver.org/) version; tags that don't parse are silently
skipped, so a stray `latest` or `nightly` tag won't break update discovery.
Pre-releases are listed too — your application can decide whether to offer them.

## Example: a GitHub Actions pipeline using the target triple

This is a complete, copy-pasteable example that builds your application for
several targets and uploads each binary as `<name>-<target-triple>[.exe]` — the
exact layout [`naming::rust`](https://docs.rs/update-rs/latest/update_rs/naming/fn.rust.html)
expects. It runs when you publish a GitHub Release, and matches the version from
the release tag.

```yaml
name: Release Binaries

# Build a binary per target and upload it to the GitHub Release as
# `<name>-<target-triple>[.exe]`, the layout `update_rs::naming::rust` expects.
on:
  release:
    types: [published]

permissions:
  contents: write # required to upload release assets

env:
  BIN_NAME: myapp # the name of your binary (and the prefix you pass to naming::rust)

jobs:
  upload:
    name: ${{ matrix.target }}
    runs-on: ${{ matrix.os }}
    strategy:
      fail-fast: false
      matrix:
        include:
          - { target: x86_64-unknown-linux-gnu, os: ubuntu-latest }
          - { target: aarch64-unknown-linux-gnu, os: ubuntu-latest, cross: true }
          - { target: x86_64-pc-windows-msvc, os: windows-latest }
          - { target: x86_64-apple-darwin, os: macos-latest }
          - { target: aarch64-apple-darwin, os: macos-latest }
    steps:
      - uses: actions/checkout@v7

      - uses: dtolnay/rust-toolchain@stable
        with:
          targets: ${{ matrix.target }}

      # `cross` (Docker-based) gives a working linker/sysroot for the Linux
      # cross target; the native targets build with plain `cargo`.
      - name: Install cross
        if: matrix.cross
        uses: taiki-e/install-action@v2
        with:
          tool: cross

      - name: Build
        shell: bash
        run: ${{ matrix.cross && 'cross' || 'cargo' }} build --release --locked --target ${{ matrix.target }}

      - name: Upload to the release
        shell: bash
        env:
          GH_TOKEN: ${{ github.token }}
          TAG: ${{ github.event.release.tag_name }}
        run: |
          ext=""
          [ "${{ runner.os }}" = "Windows" ] && ext=".exe"
          asset="${BIN_NAME}-${{ matrix.target }}${ext}"
          cp "target/${{ matrix.target }}/release/${BIN_NAME}${ext}" "$asset"
          gh release upload "$TAG" "$asset" --clobber
```

This publishes, for tag `v1.2.3`, assets like:

```text
myapp-x86_64-unknown-linux-gnu
myapp-aarch64-unknown-linux-gnu
myapp-x86_64-pc-windows-msvc.exe
myapp-x86_64-apple-darwin
myapp-aarch64-apple-darwin
```

Add a row to the matrix for each extra target you ship. GitHub computes a
SHA-256 digest for every uploaded asset automatically, so the downloads are
[verified](#integrity-verification) with no extra work.

### Configuring the update manager to match

Point `GitHubSource` at the same repository and use `naming::rust` with the same
binary name. At build time `update-rs` captures the target triple it was compiled
for (exposed as [`update_rs::TARGET`](https://docs.rs/update-rs/latest/update_rs/constant.TARGET.html)),
so the running binary asks for exactly the asset that was built for its own
target:

```rust
use update_rs::{naming, GitHubSource, UpdateManager};

let manager = UpdateManager::new(
    // On x86-64 Linux this resolves to "myapp-x86_64-unknown-linux-gnu";
    // on x86-64 Windows, "myapp-x86_64-pc-windows-msvc.exe"; and so on.
    GitHubSource::new("yourorg/myapp", naming::rust("myapp"))
        .with_release_tag_prefix("v"),
);
```

The only things that must agree are the **repository** (`yourorg/myapp`), the
**binary name** (`myapp` — `BIN_NAME` in the workflow and the prefix passed to
`naming::rust`), and the **tag prefix** (`v`).

## Another example: Go-style names

If you prefer the shorter Go-style names that [`naming::go`](https://docs.rs/update-rs/latest/update_rs/naming/fn.go.html)
matches (`myapp-linux-amd64`, `myapp-windows-amd64.exe`, ...), Git-Tool's
[release workflow](https://github.com/SierraSoftworks/git-tool/blob/main/.github/workflows/release.yml)
is a worked example to adapt — the only difference is the asset names it uploads
and using `naming::go("myapp")` instead of `naming::rust("myapp")`.
