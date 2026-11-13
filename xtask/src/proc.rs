//! Running external commands with inherited output.

use std::path::Path;
use std::process::Command;

/// Runs `cargo <args>` in the workspace root; any non-zero exit is an error.
pub fn cargo(root: &Path, args: &[&str]) -> Result<(), String> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
    run(root, &cargo, args)
}

/// Runs `program <args>` in the workspace root; any non-zero exit is an error.
pub fn run(root: &Path, program: &str, args: &[&str]) -> Result<(), String> {
    let status = Command::new(program)
        .args(args)
        .current_dir(root)
        .status()
        .map_err(|e| format!("cannot start `{program} {}`: {e}", args.join(" ")))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "`{program} {}` exited with {status}",
            args.join(" ")
        ))
    }
}
