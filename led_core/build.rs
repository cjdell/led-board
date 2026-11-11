//! Stamps the source version of the running image into the crate.
//!
//! The `GIT_*` env vars feed `led_core::build_info`, which surfaces them on
//! the live status feed, `GET /api/version`, and the UI — so a board can say
//! exactly which commit is on flash. (Identifying a running image by bundle
//! size was how the Nov 2026 OTA forensics started; never again.)
//!
//! The script re-runs on *every* build — `rerun-if-changed` points at a path
//! that never exists. `git status`/`HEAD` reflect the whole tree, which has
//! no single file whose mtime could be watched, and the script is trivial.

use std::process::Command;

fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn main() {
    println!("cargo:rerun-if-changed=.always-rerun");

    let sha = git(&["rev-parse", "--short=8", "HEAD"]).unwrap_or_else(|| "unknown".into());

    // An uncommitted working tree must not masquerade as a reproducible
    // commit — bench builds carry the dirty flag.
    let dirty = git(&["status", "--porcelain"]).is_some_and(|s| !s.is_empty());

    // Commit date, not build date: orders two builds of the same sha.
    let commit_time =
        git(&["log", "-1", "--format=%cd", "--date=format:%Y-%m-%dT%H:%MZ"]).unwrap_or_else(|| "unknown".into());

    // Wall-clock build time. The commit date cannot distinguish two dirty
    // builds of the same sha, which made OTA slot-selection forensics
    // needlessly hard; `/api/version` exposes this so "did the OTA actually
    // boot?" is a single HTTP request.
    let build_time = Command::new("date")
        .args(["-u", "+%Y-%m-%dT%H:%M:%SZ"])
        .output()
        .ok()
        .filter(|out| out.status.success())
        .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_string())
        .unwrap_or_else(|| "unknown".into());

    println!("cargo:rustc-env=GIT_SHA={sha}");
    println!("cargo:rustc-env=GIT_DIRTY={dirty}");
    println!("cargo:rustc-env=GIT_COMMIT_TIME={commit_time}");
    println!("cargo:rustc-env=BUILD_TIME={build_time}");
}
