// Template `build.rs` for an application that self-updates with `update-rs`.
//
// Copy this into the *binary* crate that you ship to users (the one that calls
// `update-rs`), NOT into a library — a library produces an `.rlib` and has no
// executable to attach a manifest to. Only the final binary can carry one, so
// embedding the manifest is the consuming application's responsibility.
//
// It embeds an application manifest declaring `requestedExecutionLevel
// level="asInvoker"`, which stops Windows' installer-detection heuristic from
// demanding UAC elevation for your updater binary (and the temporary
// `yourapp-<tag>.exe` it downloads), avoiding ERROR_ELEVATION_REQUIRED (Win32
// error 740). See `app.exe.manifest` in this directory.
//
// Requires, in your binary crate's Cargo.toml:
//
//   [target.'cfg(windows)'.build-dependencies]
//   winresource = "0.1"
//
// and `app.exe.manifest` placed in your crate (here we read it from `assets/`).

fn main() {
    #[cfg(windows)]
    embed_windows_resources();
}

#[cfg(windows)]
fn embed_windows_resources() {
    let mut res = winresource::WindowsResource::new();

    // Optional: set your application icon. Point this at an .ico in your repo.
    // res.set_icon("assets/app.ico");

    // The assembly identity requires a four-part `major.minor.build.revision`
    // version, so pad the three-part Cargo version with a trailing `.0`.
    let manifest = std::fs::read_to_string("assets/app.exe.manifest")
        .expect("failed to read Windows manifest template");
    let version = std::env::var("CARGO_PKG_VERSION").expect("CARGO_PKG_VERSION not set by Cargo");
    let manifest = manifest.replace("{VERSION}", &format!("{version}.0"));
    res.set_manifest(&manifest);

    res.set("ProductName", "Your Application");
    res.set("FileDescription", "Your application description.");
    res.set("CompanyName", "Your Company");
    res.set("LegalCopyright", "Copyright © Your Company");

    res.compile()
        .expect("failed to embed Windows executable resources");

    println!("cargo:rerun-if-changed=assets/app.exe.manifest");
    // println!("cargo:rerun-if-changed=assets/app.ico");
}
