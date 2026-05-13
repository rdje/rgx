//! Build script: invoke cbindgen to regenerate `rgx.h` from the
//! Rust source on every build.
//!
//! The header is written to `${OUT_DIR}/include/rgx.h` (visible to
//! cdylib consumers via `cargo build` outputs) and ALSO copied to
//! `${CARGO_MANIFEST_DIR}/include/rgx.h` so it's committed alongside
//! the source for callers who want to inspect the API without
//! building.
//!
//! cbindgen is idempotent: re-running it on unchanged source produces
//! identical output. The CI ABI-diff gate (see design doc §6.2)
//! relies on this — a `git diff` after `cargo build` is a red flag.

use std::env;
use std::path::PathBuf;

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let crate_name = "rgx-capi";
    let config_path = manifest_dir.join("cbindgen.toml");

    // Re-run only when the inputs change.
    println!("cargo:rerun-if-changed=cbindgen.toml");
    println!("cargo:rerun-if-changed=src/lib.rs");
    println!("cargo:rerun-if-changed=build.rs");

    let config = match cbindgen::Config::from_file(&config_path) {
        Ok(c) => c,
        Err(e) => {
            // Build script errors print but don't fail the build by
            // default; print prominently. cbindgen failures are
            // recoverable — Rust users don't need the C header.
            eprintln!("cargo:warning=rgx-capi: failed to read cbindgen.toml: {e}");
            return;
        }
    };

    let builder = cbindgen::Builder::new()
        .with_crate(&manifest_dir)
        .with_config(config);

    let bindings = match builder.generate() {
        Ok(b) => b,
        Err(e) => {
            eprintln!("cargo:warning=rgx-capi: cbindgen generation failed: {e}");
            return;
        }
    };

    // Write to the committed location so callers can inspect the
    // header at the source tree without running a build.
    let committed_dir = manifest_dir.join("include");
    if let Err(e) = std::fs::create_dir_all(&committed_dir) {
        eprintln!(
            "cargo:warning=rgx-capi: failed to create include dir {}: {}",
            committed_dir.display(),
            e
        );
        return;
    }
    let committed_path = committed_dir.join("rgx.h");
    if !bindings.write_to_file(&committed_path) {
        eprintln!(
            "cargo:warning=rgx-capi: failed to write {}",
            committed_path.display()
        );
    }

    // Also copy to OUT_DIR for downstream Cargo consumers.
    if let Ok(out_dir) = env::var("OUT_DIR") {
        let out_path = PathBuf::from(out_dir).join("include").join("rgx.h");
        if let Some(parent) = out_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::copy(&committed_path, &out_path);
    }

    let _ = crate_name; // suppress unused warning if removed in future
}
