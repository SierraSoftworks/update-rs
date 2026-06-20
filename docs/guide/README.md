# Getting started

`update-rs` lets a Rust application replace its own binary on disk with a newer
release and relaunch into it — without an installer, package manager, or
external updater process. It was extracted from
[Git-Tool](https://github.com/SierraSoftworks/git-tool), where it powers the
`gt update` command.

## Install

```shell
cargo add update-rs
```

`update-rs` is async and uses [Tokio](https://tokio.rs/); add it too if you
haven't already:

```shell
cargo add tokio --features macros,rt-multi-thread
```

## The shape of an integration

There are three things your application does:

1. **Construct an [`UpdateManager`](https://docs.rs/update-rs/latest/update_rs/struct.UpdateManager.html)**
   around a [`Source`](https://docs.rs/update-rs/latest/update_rs/trait.Source.html)
   — the crate ships [`GitHubSource`](https://docs.rs/update-rs/latest/update_rs/struct.GitHubSource.html).
2. **Detect the resume flag** (`update_rs::RESUME_FLAG`) at the very start of
   `main()` and hand the value that follows it to `resume_from_arg`. This is how
   the library drives the multi-phase handover.
3. **Trigger an update** when you want to, by listing releases and calling
   `update` on the newest one.

The next pages walk through each of these:

- [Usage](./usage.md) — wiring up the manager and the resume flag.
- [How it works](./how-it-works.md) — the three-phase update process in detail.
- [Windows & Error 740](./windows.md) — shipping a manifest so updates never
  trip a UAC prompt.
- [Your release pipeline](./release-pipeline.md) — publishing the binaries that
  `GitHubSource` downloads.
