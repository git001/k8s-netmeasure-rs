//! Build metadata for `build_info` (design §5, §13): git revision, rustc version,
//! target triple. The container build passes `BUILD_GIT_SHA`, because the build
//! context has no `.git` directory.
#![forbid(unsafe_code)]
#![forbid(clippy::too_many_arguments)]

use std::path::Path;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-env-changed=BUILD_GIT_SHA");
    let head = Path::new("../../.git/HEAD");
    if head.exists() {
        println!("cargo:rerun-if-changed=../../.git/HEAD");
        println!("cargo:rerun-if-changed=../../.git/refs");
    }

    let git_sha = std::env::var("BUILD_GIT_SHA")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| command_output("git", &["rev-parse", "--short=12", "HEAD"]))
        .unwrap_or_else(|| "unknown".to_owned());
    let rustc = std::env::var("RUSTC").unwrap_or_else(|_| "rustc".to_owned());
    let rustc_version = command_output(&rustc, &["--version"])
        .and_then(|v| v.split_whitespace().nth(1).map(str::to_owned))
        .unwrap_or_else(|| "unknown".to_owned());
    let target = std::env::var("TARGET").unwrap_or_else(|_| "unknown".to_owned());

    println!("cargo:rustc-env=NETMEASURE_BUILD_GIT_SHA={git_sha}");
    println!("cargo:rustc-env=NETMEASURE_BUILD_RUSTC={rustc_version}");
    println!("cargo:rustc-env=NETMEASURE_BUILD_TARGET={target}");
}

fn command_output(program: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(program).args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8(out.stdout).ok()?.trim().to_owned();
    (!s.is_empty()).then_some(s)
}
