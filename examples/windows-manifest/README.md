# Windows manifest template (avoiding UAC / Error 740)

These files are **copy-paste templates for the application that self-updates** —
they don't do anything in `update-rs` itself. A library crate compiles to an
`.rlib` and has no executable to attach a manifest to, so embedding the manifest
must happen in your *binary* crate.

## Why you need this

Windows has a legacy *installer-detection heuristic*: it auto-requests UAC
elevation for any executable whose file name contains tokens like `update`,
`setup`, `install`, or `patch`. `update-rs` downloads the new release to a
temporary file named like `yourapp-<tag>.exe`, and your updater binary is often
named similarly — so the relaunch between update phases can trigger an elevation
prompt. If elevation is unavailable or declined, `CreateProcess` fails with
`ERROR_ELEVATION_REQUIRED` (Win32 error **740**) and the update breaks.

Shipping an application manifest that declares
`requestedExecutionLevel level="asInvoker"` opts your binary out of the
heuristic: it runs with the caller's token and never prompts. The same manifest
also turns on long-path support and a UTF-8 active code page as a bonus.

## How to use it

1. Copy `app.exe.manifest` into your binary crate (this template reads it from
   `assets/app.exe.manifest`) and edit the identity, description, and
   version-info strings. Leave the `{VERSION}` placeholder — `build.rs` fills it
   in from `CARGO_PKG_VERSION`.

2. Copy `build.rs` to your binary crate root and adjust the `ProductName` /
   `FileDescription` / `CompanyName` / `LegalCopyright` strings (and the icon
   path, if you want one).

3. Add the build dependency to your binary crate's `Cargo.toml`:

   ```toml
   [target.'cfg(windows)'.build-dependencies]
   winresource = "0.1"
   ```

4. Build on Windows (`cargo build --release`). To confirm the manifest was
   embedded, you can extract it with the Windows SDK's `mt.exe`:

   ```text
   mt.exe -inputresource:target\release\yourapp.exe;#1 -out:check.manifest
   ```

   or simply verify that launching the binary no longer prompts for elevation.

> **Testing tip:** Cargo's own test binaries (`yourapp-<hash>.exe`) hit the same
> heuristic and can't be launched by `cargo test` without elevation. Set the
> environment variable `__COMPAT_LAYER=RunAsInvoker` (e.g. via a `[env]` block in
> `.cargo/config.toml`, as `update-rs` itself does) to run them with the caller's
> token during development and CI.
