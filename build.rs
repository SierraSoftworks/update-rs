// Capture the target triple this crate is being compiled for (e.g.
// `x86_64-unknown-linux-gnu`) and expose it to the crate as the
// `UPDATE_RS_TARGET` compile-time environment variable, so that
// `update_rs::TARGET` / `update_rs::naming::rust` can build "Rust style" release
// asset names. `TARGET` is always set by Cargo for build scripts.
// See https://stackoverflow.com/a/48970885.
fn main() {
    let target = std::env::var("TARGET").unwrap_or_default();
    println!("cargo:rustc-env=UPDATE_RS_TARGET={target}");
}
