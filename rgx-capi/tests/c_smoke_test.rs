//! Integration test that compiles the C-side smoke test in
//! `tests/c/smoke_test.c`, links it against the `rgx-capi`
//! staticlib, runs the resulting binary, and asserts the exit
//! status is 0.
//!
//! Per `docs/A9_LANGUAGE_BINDINGS_DESIGN.md` §6.1: every Phase
//! commit must pass the C-side smoke test on Linux + macOS. The
//! Rust-side test infrastructure is `cargo test` so the smoke
//! test integrates naturally with CI.
//!
//! On Windows the linker/compiler invocation differs; this test
//! is gated to Linux/macOS for now. Windows support is on the
//! roadmap (see design doc §5 Phase 1 deliverables).

#![cfg(any(target_os = "linux", target_os = "macos"))]

use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

#[test]
fn c_side_smoke_test_runs_and_exits_zero() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let c_source = manifest_dir.join("tests/c/smoke_test.c");
    let header_dir = manifest_dir.join("include");
    let header_path = header_dir.join("rgx.h");

    assert!(
        c_source.exists(),
        "C source missing: {}",
        c_source.display()
    );
    assert!(
        header_path.exists(),
        "rgx.h missing — build.rs should have generated it: {}",
        header_path.display()
    );

    // Find the staticlib produced by the `cdylib`+`staticlib`
    // crate-type. `cargo test` builds the test binary, which
    // implicitly builds rgx-capi's rlib; the staticlib is built
    // explicitly by Cargo when crate-type = ["staticlib"] is set,
    // and lands in `target/{profile}/deps/` or `target/{profile}/`.
    let target_dir = locate_target_dir(&manifest_dir);
    let profile = if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    };
    // Same staticlib name on Linux and macOS; Windows would use
    // `rgx_capi.lib` but the whole test is cfg-gated off there.
    let lib_path = locate_lib(&target_dir, profile, "librgx_capi.a");
    assert!(
        lib_path.exists(),
        "staticlib not found at {}; run `cargo build -p rgx-capi` first",
        lib_path.display()
    );

    // Compile and link the C smoke test against the staticlib.
    let tmp_dir = env::temp_dir();
    let exe_path = tmp_dir.join(format!("rgx_capi_smoke_{}", std::process::id()));

    let cc = env::var("CC").unwrap_or_else(|_| "cc".to_string());
    let mut cmd = Command::new(&cc);
    cmd.arg(c_source.as_os_str())
        .arg("-I")
        .arg(&header_dir)
        .arg("-o")
        .arg(&exe_path)
        .arg(&lib_path);

    // Platform-specific system libraries needed by the staticlib.
    // CoreServices supplies the FSEventStream* symbols used by the
    // `notify` crate (transitive dep via rgx-core's tail_file).
    if cfg!(target_os = "macos") {
        cmd.args([
            "-framework",
            "CoreFoundation",
            "-framework",
            "CoreServices",
            "-framework",
            "Security",
            "-framework",
            "SystemConfiguration",
        ]);
    } else if cfg!(target_os = "linux") {
        cmd.args(["-lpthread", "-ldl", "-lm"]);
    }

    let output = cmd
        .output()
        .expect("failed to invoke C compiler — is `cc` on PATH?");

    if !output.status.success() {
        panic!(
            "C compile failed (cc={cc}):\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    // Run the smoke test binary.
    let run_output = Command::new(&exe_path)
        .output()
        .expect("failed to execute compiled C smoke test");

    let _ = std::fs::remove_file(&exe_path);

    if !run_output.status.success() {
        panic!(
            "C smoke test failed (exit code {:?}):\nstdout:\n{}\nstderr:\n{}",
            run_output.status.code(),
            String::from_utf8_lossy(&run_output.stdout),
            String::from_utf8_lossy(&run_output.stderr)
        );
    }
}

/// Walk up from the manifest dir looking for a `target/` sibling.
/// Cargo workspaces have a single `target/` at the workspace root,
/// not under each crate.
fn locate_target_dir(manifest_dir: &Path) -> PathBuf {
    let mut cursor = manifest_dir.to_path_buf();
    loop {
        let candidate = cursor.join("target");
        if candidate.is_dir() {
            return candidate;
        }
        if !cursor.pop() {
            // No target/ found anywhere. Default to a sibling path
            // and let the caller surface the missing-file error.
            return manifest_dir.join("..").join("target");
        }
    }
}

fn locate_lib(target_dir: &Path, profile: &str, lib_name: &str) -> PathBuf {
    // staticlib usually lands in target/{profile}/ directly.
    let primary = target_dir.join(profile).join(lib_name);
    if primary.exists() {
        return primary;
    }
    // Fall back to target/{profile}/deps/.
    target_dir.join(profile).join("deps").join(lib_name)
}
