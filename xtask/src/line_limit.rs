//! Gate `line-limit` (design §2, §12):
//! - every `.rs` file ≤ 200 counted lines, or ≤ 400 with a first line
//!   `//! line-limit: <reason>`; lines of `#[cfg(test)]` items do not count;
//! - every crate root forbids `clippy::too_many_arguments`, and nothing allows it or
//!   raises clippy's threshold.

use std::path::{Path, PathBuf};

use crate::test_spans::test_line_ranges;

pub const TARGET: usize = 200;
pub const MAXIMUM: usize = 400;
const JUSTIFICATION: &str = "//! line-limit:";
/// The lint and the groups that contain it: allowing any of them allows the lint.
const LINTS: &[&str] = &[
    "clippy::too_many_arguments",
    "clippy::complexity",
    "clippy::all",
];
const FORBID_ARGS: &str = "#![forbid(clippy::too_many_arguments)]";
/// The one crate root that may only `deny`: clap derive output allows
/// `clippy::complexity`, a hard error under `forbid` (design §2).
const DENY_ONLY_ROOT: &str = "crates/netmeasure-cli/src/lib.rs";
const DENY_ARGS: &str = "#![deny(clippy::too_many_arguments)]";

/// Outcome of checking one file's size.
#[derive(Debug, PartialEq, Eq)]
pub enum Size {
    Ok,
    Justified(String),
    UnneededJustification,
    NeedsJustification,
    TooLarge,
}

pub fn run(root: &Path) -> Result<(), String> {
    let mut files = Vec::new();
    rust_files(root, &mut files).map_err(|e| format!("walking {}: {e}", root.display()))?;
    files.sort();
    let mut errors = Vec::new();
    let mut justified = Vec::new();
    for file in &files {
        let rel = file
            .strip_prefix(root)
            .unwrap_or(file)
            .display()
            .to_string();
        let source = std::fs::read_to_string(file).map_err(|e| format!("{rel}: {e}"))?;
        let counted = match counted_lines(&source) {
            Ok(n) => n,
            Err(e) => {
                errors.push(format!("{rel}: {e}"));
                continue;
            }
        };
        match classify(&source, counted) {
            Size::Ok => {}
            Size::Justified(reason) => justified.push(format!("{rel} ({counted}): {reason}")),
            Size::UnneededJustification => {
                eprintln!("warning: {rel}: {counted} lines, `{JUSTIFICATION}` is not needed");
            }
            Size::NeedsJustification => errors.push(format!(
                "{rel}: {counted} lines > {TARGET} without a first line `{JUSTIFICATION} <reason>`"
            )),
            Size::TooLarge => errors.push(format!("{rel}: {counted} lines > {MAXIMUM}")),
        }
        if let Some(lint) = weakened_lint(&source) {
            errors.push(format!("{rel}: weakens {lint}"));
        }
        if is_crate_root(file) {
            let required = if rel == DENY_ONLY_ROOT {
                DENY_ARGS
            } else {
                FORBID_ARGS
            };
            if !source.contains(required) {
                errors.push(format!("{rel}: crate root without `{required}`"));
            }
        }
    }
    errors.extend(clippy_config_errors(root));
    eprintln!(
        "line-limit: {} files checked, {} justified",
        files.len(),
        justified.len()
    );
    for j in &justified {
        eprintln!("  justified: {j}");
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(format!("\n  {}", errors.join("\n  ")))
    }
}

/// Physical lines minus the lines of `#[cfg(test)]` items.
pub fn counted_lines(source: &str) -> Result<usize, String> {
    let total = source.lines().count();
    let mut excluded = vec![false; total + 1];
    for (start, end) in test_line_ranges(source)? {
        if let Some(lines) = excluded.get_mut(start..=end.min(total)) {
            lines.fill(true);
        }
    }
    Ok(total - excluded.iter().filter(|&&e| e).count())
}

pub fn classify(source: &str, counted: usize) -> Size {
    let reason = source
        .lines()
        .next()
        .and_then(|l| l.strip_prefix(JUSTIFICATION))
        .map(str::trim)
        .filter(|r| !r.is_empty());
    match (counted, reason) {
        (n, _) if n > MAXIMUM => Size::TooLarge,
        (n, Some(r)) if n > TARGET => Size::Justified(r.to_owned()),
        (n, None) if n > TARGET => Size::NeedsJustification,
        (_, Some(_)) => Size::UnneededJustification,
        _ => Size::Ok,
    }
}

/// The first lint of [`LINTS`] named inside an `allow(…)`, `expect(…)` or `warn(…)`
/// list anywhere in the source (whitespace ignored), e.g. `#[allow(dead_code,
/// clippy::complexity)]`.
fn weakened_lint(source: &str) -> Option<&'static str> {
    let compact: String = source.split_whitespace().collect();
    for level in ["allow(", "expect(", "warn("] {
        let mut rest = compact.as_str();
        while let Some(pos) = rest.find(level) {
            rest = &rest[pos + level.len()..];
            let list = &rest[..rest.find(')').unwrap_or(rest.len())];
            if let Some(lint) = LINTS
                .iter()
                .copied()
                .find(|l| list.split(',').any(|x| x == *l))
            {
                return Some(lint);
            }
        }
    }
    None
}

fn is_crate_root(file: &Path) -> bool {
    let name = file.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let parent = file
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str());
    matches!(
        (name, parent),
        ("lib.rs" | "main.rs", Some("src")) | ("build.rs", _)
    ) || parent == Some("tests") && name.ends_with(".rs")
}

fn clippy_config_errors(root: &Path) -> Vec<String> {
    [".clippy.toml", "clippy.toml"]
        .iter()
        .map(|n| root.join(n))
        .filter_map(|p| std::fs::read_to_string(&p).ok().map(|s| (p, s)))
        .filter(|(_, s)| s.contains("too-many-arguments-threshold"))
        .map(|(p, _)| format!("{}: raises too-many-arguments-threshold", p.display()))
        .collect()
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if name.starts_with('.') || name == "target" || name == "local" {
            continue;
        }
        if path.is_dir() {
            rust_files(&path, out)?;
        } else if name.ends_with(".rs") {
            out.push(path);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(n: usize) -> String {
        "// x\n".repeat(n)
    }

    #[test]
    fn size_classes() {
        assert_eq!(classify(&lines(200), 200), Size::Ok);
        assert_eq!(classify(&lines(201), 201), Size::NeedsJustification);
        let justified = format!("{JUSTIFICATION} one coherent idea\n{}", lines(300));
        assert_eq!(
            classify(&justified, 301),
            Size::Justified("one coherent idea".to_owned())
        );
        assert_eq!(classify(&justified, 401), Size::TooLarge);
        assert_eq!(classify(&justified, 10), Size::UnneededJustification);
        let empty_reason = format!("{JUSTIFICATION}   \n{}", lines(300));
        assert_eq!(classify(&empty_reason, 301), Size::NeedsJustification);
    }

    #[test]
    fn test_items_are_not_counted() {
        let src = "fn a() {}\n#[cfg(test)]\nmod tests {\n    fn t() {}\n}\n";
        assert_eq!(counted_lines(src).unwrap(), 1);
    }

    #[test]
    fn weakening_the_lint_or_its_groups_is_found() {
        let lint = ["clippy", "too_many_arguments"].join("::");
        assert_eq!(weakened_lint(&format!("#[allow({lint})]")), Some(LINTS[0]));
        let group = format!("#[expect(dead_code,\n  {})]", LINTS[1]);
        assert_eq!(weakened_lint(&group), Some(LINTS[1]));
        assert_eq!(
            weakened_lint(&format!("#![warn({})]", LINTS[2])),
            Some(LINTS[2])
        );
        assert_eq!(weakened_lint(&format!("#![forbid({lint})]")), None);
        assert_eq!(weakened_lint("#[allow(dead_code)] fn f() {}"), None);
    }

    #[test]
    fn crate_roots() {
        assert!(is_crate_root(Path::new("crates/a/src/lib.rs")));
        assert!(is_crate_root(Path::new("crates/a/src/main.rs")));
        assert!(is_crate_root(Path::new("crates/a/build.rs")));
        assert!(is_crate_root(Path::new("crates/a/tests/cli.rs")));
        assert!(!is_crate_root(Path::new("crates/a/src/cli.rs")));
    }
}
