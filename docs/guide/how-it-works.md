# How it works: the three-phase update

A running executable can't reliably overwrite itself. On Windows the running
image is locked for writing; on every platform, swapping a binary out from under
a live process is a recipe for half-applied updates. `update-rs` sidesteps this
by performing the update across three phases, **each running from a different
binary** and relaunching the next. The approach is described in detail in
[*Building self-updating applications*](https://sierrasoftworks.com/2019/10/15/app-updates/#cleanup).

```text
running app ──prepare──▶ temp binary ──replace──▶ updated app ──cleanup──▶ done
   (download)              (overwrite original)        (remove temp file)
```

## Phase 1 — Prepare

Runs inside your **original** application when you call `update`.

1. The new release is downloaded to a temporary file next to your binary
   (`yourapp-<tag>` in the system temp directory).
2. If GitHub reported a SHA-256 digest for the asset, the downloaded bytes are
   verified against it; a mismatch aborts the update (and removes the temporary
   file) so a corrupted or tampered binary is never run.
3. On Unix the file is marked executable.
4. The temporary binary is launched with the resume flag, carrying a serialized
   state that says "you are now in the replace phase".

`update` returns `Ok(true)`, and your application exits so it no longer holds the
binary open.

## Phase 2 — Replace

Runs inside the freshly downloaded **temporary** binary.

1. It deletes the original application file, retrying with a short backoff in
   case the original process hasn't fully exited yet.
2. It copies itself over the original's path.
3. It launches the now-updated original binary with the resume flag, carrying a
   state that says "you are now in the cleanup phase", then exits.

## Phase 3 — Cleanup

Runs inside the **updated** application.

1. It deletes the leftover temporary binary.
2. It returns control — the update is complete.

## Why a flag and not a daemon?

Each phase is just your own binary, relaunched with
`--update-resume-internal <state-json>`. There is no background service, no
helper executable, and nothing left installed on the user's machine. The only
requirement is that your `main()` detects the resume flag and calls
`resume_from_arg` before doing anything else — see [Usage](./usage.md).

## Errors are made for humans

Every failure is a [`human_errors::Error`](https://docs.rs/human-errors)
(re-exported as `update_rs::Error`) carrying a plain-language description and
concrete advice — for example, telling the user to close other running copies of
the application if the binary is still locked during the replace phase.
