// PyO3 extension modules leave the Python C-API symbols (`_Py_*`) to be resolved
// at import time by the host interpreter, not linked at build time. maturin sets
// the macOS linker flag for this; for a plain `cargo build`/`check` to link the
// cdylib we set it ourselves. (`rustc-cdylib-link-arg` applies only to the cdylib
// crate-type, leaving the rlib untouched.)
fn main() {
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os == "macos" {
        println!("cargo:rustc-cdylib-link-arg=-undefined");
        println!("cargo:rustc-cdylib-link-arg=dynamic_lookup");
    }
}
