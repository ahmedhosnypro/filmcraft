//! Regression and hostile-parameter tests for the target-dir commands (`target-size`,
//! `clean-target`), per AGENTS.md §0.8. Each test drives the real `xtask` binary against a
//! scratch target dir, so no real `target/` is ever read or removed.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// A unique scratch dir removed on drop, so a failing test cannot leak state into the next one.
struct Scratch(PathBuf);

impl Scratch {
    fn new(tag: &str) -> Scratch {
        let dir = std::env::temp_dir().join(format!("filmcraft-xtask-tests-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch dir");
        // Canonicalize: the child resolves its cwd with getcwd, which resolves symlinks
        // (macOS `/tmp` -> `/private/tmp`), and the assertions compare literal path text.
        Scratch(dir.canonicalize().expect("canonical scratch dir"))
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn text(&self) -> String {
        self.0.to_string_lossy().into_owned()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Run the xtask binary with `env` applied (`TARGET_CAP_GB` cleared first for determinism) and an
/// optional cwd; returns the captured output.
fn xtask(args: &[&str], env: &[(&str, &str)], cwd: Option<&Path>) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_xtask"));
    cmd.args(args).env_remove("TARGET_CAP_GB");
    for (k, v) in env {
        cmd.env(k, v);
    }
    if let Some(cwd) = cwd {
        cmd.current_dir(cwd);
    }
    cmd.output().expect("spawn the xtask binary")
}

fn out_text(out: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr))
}

/// A relative `CARGO_TARGET_DIR` is resolved against the current directory, the way cargo
/// resolves it — not against the workspace root (regression: `cargo xtask` from a member
/// directory used to report and clean the wrong tree).
#[test]
fn relative_target_dir_follows_the_current_directory() {
    let scratch = Scratch::new("relative");
    let out = xtask(&["target-size"], &[("CARGO_TARGET_DIR", "rel-target")], Some(scratch.path()));
    let text = out_text(&out);
    assert!(
        out.status.success() && text.starts_with(&format!("{}: ", scratch.path().join("rel-target").display())),
        "a relative CARGO_TARGET_DIR must resolve against the cwd like cargo; got: {text}"
    );
}

/// An absolute `CARGO_TARGET_DIR` is used as-is.
#[test]
fn absolute_target_dir_is_used_as_is() {
    let scratch = Scratch::new("absolute");
    let dir = scratch.text();
    let out = xtask(&["target-size"], &[("CARGO_TARGET_DIR", &dir)], None);
    let text = out_text(&out);
    assert!(out.status.success() && text.starts_with(&format!("{}: ", dir)), "an absolute CARGO_TARGET_DIR must be used as-is; got: {text}");
}

/// `clean-target --incremental` reclaims every profile's session cache, not just dev's
/// (regression: the `dbg` cache invited by `CARGO_INCREMENTAL=1` + `cargo test --profile dbg`
/// used to survive the clean).
#[test]
fn clean_incremental_sweeps_every_profile() {
    let scratch = Scratch::new("incremental");
    let t = scratch.path();
    for p in ["debug/incremental", "dbg/incremental", "release/incremental"] {
        std::fs::create_dir_all(t.join(p)).expect("profile incremental dir");
        std::fs::write(t.join(p).join("cache.bin"), [0u8; 4096]).expect("cache file");
    }
    std::fs::write(t.join("release/keep.bin"), [0u8; 16]).expect("release artifact");
    let dir = scratch.text();
    let out = xtask(&["clean-target", "--incremental"], &[("CARGO_TARGET_DIR", &dir)], None);
    let text = out_text(&out);
    assert!(out.status.success(), "clean-target --incremental failed: {text}");
    for p in ["debug/incremental", "dbg/incremental", "release/incremental"] {
        assert!(!t.join(p).exists(), "{p} must be reclaimed");
    }
    assert!(t.join("release/keep.bin").exists(), "only incremental dirs may be removed");
}

/// `clean-target <mode>` on a missing tree is a no-op, not an error.
#[test]
fn clean_target_missing_tree_is_a_no_op() {
    let scratch = Scratch::new("missing");
    let dir = scratch.text();
    let out = xtask(&["clean-target", "--dbg"], &[("CARGO_TARGET_DIR", &dir)], None);
    let text = out_text(&out);
    assert!(out.status.success() && text.contains("nothing to do"), "a missing tree must be a no-op, got: {text}");
}

/// `target-size --check` exits non-zero over the cap and zero under it.
#[test]
fn check_enforces_the_cap() {
    let scratch = Scratch::new("cap");
    std::fs::write(scratch.path().join("blob.bin"), [0u8; 64 * 1024]).expect("blob");
    let dir = scratch.text();
    let over = xtask(&["target-size", "--check"], &[("CARGO_TARGET_DIR", &dir), ("TARGET_CAP_GB", "0.000001")], None);
    let text = out_text(&over);
    assert!(!over.status.success() && text.contains("over the"), "over the cap --check must fail, got: {text}");
    let under = xtask(&["target-size", "--check"], &[("CARGO_TARGET_DIR", &dir), ("TARGET_CAP_GB", "1000")], None);
    assert!(under.status.success(), "under the cap --check must pass: {}", out_text(&under));
}

/// Hostile parameters are rejected with an error, never a panic (AGENTS.md §0.8).
#[test]
fn hostile_parameters_error_cleanly() {
    let scratch = Scratch::new("hostile");
    let dir = scratch.text();
    let cases: &[(&[&str], &[(&str, &str)], &str)] = &[
        (&["target-size", "--bogus"], &[("CARGO_TARGET_DIR", dir.as_str())], "unknown argument"),
        (&["clean-target", "--nope"], &[("CARGO_TARGET_DIR", dir.as_str())], "unknown mode"),
        (&["clean-target", "--debug", "--release"], &[("CARGO_TARGET_DIR", dir.as_str())], "at most one mode"),
        (&["target-size"], &[("CARGO_TARGET_DIR", dir.as_str()), ("TARGET_CAP_GB", "abc")], "not a number"),
        (&["target-size"], &[("CARGO_TARGET_DIR", dir.as_str()), ("TARGET_CAP_GB", "0")], "must be a positive number"),
        (&["target-size"], &[("CARGO_TARGET_DIR", dir.as_str()), ("TARGET_CAP_GB", "inf")], "must be a positive number"),
    ];
    for &(args, env, needle) in cases {
        let out = xtask(args, env, None);
        let text = out_text(&out);
        assert!(!out.status.success() && text.contains(needle), "{args:?} must fail with {needle:?}, got: {text}");
    }
}
