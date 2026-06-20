# Windows & "Error 740"

On Windows, self-updating has one extra wrinkle worth understanding up front.

## The problem

Windows has a legacy **installer-detection heuristic**: it automatically
requests UAC elevation for any executable whose file name contains tokens like
`update`, `setup`, `install`, or `patch`. `update-rs` downloads the new release
to a temporary file named like `yourapp-<tag>.exe`, and updater binaries are
often named similarly — so the relaunched process can trigger an elevation
prompt.

If elevation is unavailable or the user declines it, `CreateProcess` fails with
`ERROR_ELEVATION_REQUIRED` — **Win32 error 740** — and the update breaks.

## The fix: an `asInvoker` manifest

Ship your binary with an application manifest that declares:

```xml
<requestedExecutionLevel level="asInvoker" uiAccess="false"/>
```

This opts your binary out of the heuristic: it runs with the caller's token and
never prompts. The same manifest is a good place to enable long-path support and
a UTF-8 active code page.

A library crate compiles to an `.rlib` and can't embed a manifest into your
executable, so this is your **binary** crate's responsibility. `update-rs` ships
copy-paste templates to make it quick:

- [`app.exe.manifest`](https://github.com/SierraSoftworks/update-rs/blob/main/examples/windows-manifest/app.exe.manifest)
- [`build.rs`](https://github.com/SierraSoftworks/update-rs/blob/main/examples/windows-manifest/build.rs)
- [step-by-step instructions](https://github.com/SierraSoftworks/update-rs/blob/main/examples/windows-manifest/README.md)

In short:

1. Copy `app.exe.manifest` into your binary crate (e.g. `assets/`) and edit the
   identity strings. Leave the `{VERSION}` placeholder.
2. Copy `build.rs` to your crate root.
3. Add the build dependency:

   ```toml
   [target.'cfg(windows)'.build-dependencies]
   winresource = "0.1"
   ```

4. Build on Windows. The manifest is embedded into your `.exe`, and the
   installer-detection heuristic no longer fires.

## A note for contributors and CI

The same heuristic affects Cargo's own test binaries (`yourapp-<hash>.exe`),
which means `cargo test` can fail to launch them with error 740. The fix is the
`__COMPAT_LAYER=RunAsInvoker` environment variable, which `update-rs` sets in its
[`.cargo/config.toml`](https://github.com/SierraSoftworks/update-rs/blob/main/.cargo/config.toml)
so that `cargo test` works on Windows out of the box.
