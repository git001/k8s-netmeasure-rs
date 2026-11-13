//! Gate `deny` (design §1, §12): `cargo deny check`, plus the rules cargo-deny cannot
//! express for duplicate-version exceptions in `deny.toml` `[bans] skip`:
//! - every exception carries `(review YYYY-MM-DD)` in its reason, and fails once that
//!   date has passed;
//! - a direct dependency (a key of `[workspace.dependencies]`) may never be excepted.

use std::path::Path;

use jiff::civil::Date;

use crate::proc::cargo;

const MARKER: &str = "(review ";

pub fn run(root: &Path) -> Result<(), String> {
    cargo(root, &["deny", "--locked", "--workspace", "check"])?;
    let deny = read(root, "deny.toml")?;
    let manifest = read(root, "Cargo.toml")?;
    let today = jiff::Zoned::now().date();
    let errors = skip_errors(&deny, &direct_dependencies(&manifest), today);
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

fn read(root: &Path, name: &str) -> Result<String, String> {
    std::fs::read_to_string(root.join(name)).map_err(|e| format!("{name}: {e}"))
}

/// Keys of `[workspace.dependencies]`.
pub fn direct_dependencies(manifest: &str) -> Vec<String> {
    section_lines(manifest, "[workspace.dependencies]")
        .filter_map(|l| l.split_once('=').map(|(k, _)| k.trim().to_owned()))
        .filter(|k| !k.is_empty() && !k.starts_with('#'))
        .collect()
}

/// Checks each `{ crate = "name@ver", reason = "… (review YYYY-MM-DD)" }` entry of
/// `skip`. Entries must be written on one line each.
pub fn skip_errors(deny: &str, direct: &[String], today: Date) -> Vec<String> {
    let mut errors = Vec::new();
    for line in skip_entries(deny) {
        let krate = quoted_after(line, "crate").unwrap_or_default();
        let name = krate.split('@').next().unwrap_or_default();
        if direct.iter().any(|d| d == name) {
            errors.push(format!(
                "{krate}: direct dependency, duplicates are forbidden"
            ));
        }
        match review_date(line) {
            None => errors.push(format!("{krate}: reason without `{MARKER}YYYY-MM-DD)`")),
            Some(date) if date < today => {
                errors.push(format!("{krate}: exception review date {date} has passed"));
            }
            Some(_) => {}
        }
    }
    errors
}

fn skip_entries(deny: &str) -> impl Iterator<Item = &str> {
    let mut inside = false;
    deny.lines().filter(move |l| {
        let t = l.trim();
        if t.starts_with("skip = [") {
            inside = true;
            return false;
        }
        if inside && t.starts_with(']') {
            inside = false;
        }
        inside && t.starts_with('{')
    })
}

fn section_lines<'a>(text: &'a str, header: &'a str) -> impl Iterator<Item = &'a str> {
    text.lines()
        .skip_while(move |l| l.trim() != header)
        .skip(1)
        .take_while(|l| !l.trim_start().starts_with('['))
}

fn quoted_after(line: &str, key: &str) -> Option<String> {
    let rest = &line[line.find(key)? + key.len()..];
    let rest = &rest[rest.find('"')? + 1..];
    Some(rest[..rest.find('"')?].to_owned())
}

fn review_date(line: &str) -> Option<Date> {
    let rest = &line[line.find(MARKER)? + MARKER.len()..];
    rest.get(..10)?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    const DENY: &str = r#"
[bans]
skip = [
    { crate = "foo@1.0.0", reason = "waits for bar 2 (review 2027-01-31)" },
    { crate = "baz@0.1.0", reason = "no date" },
    { crate = "clap@3.0.0", reason = "x (review 2027-01-31)" },
]
"#;

    #[test]
    fn reads_workspace_dependency_names() {
        let manifest = "[workspace]\n[workspace.dependencies]\nclap = { version = \"4\" }\n# c\nfoo = \"1\"\n[profile.release]\nlto = true\n";
        assert_eq!(direct_dependencies(manifest), vec!["clap", "foo"]);
    }

    #[test]
    fn checks_dates_and_direct_dependencies() {
        let direct = vec!["clap".to_owned()];
        let today: Date = "2026-11-13".parse().unwrap();
        let errors = skip_errors(DENY, &direct, today);
        assert_eq!(errors.len(), 2, "{errors:?}");
        assert!(errors[0].contains("baz@0.1.0"));
        assert!(errors[1].contains("clap@3.0.0: direct dependency"));
        let later: Date = "2027-02-01".parse().unwrap();
        assert_eq!(skip_errors(DENY, &direct, later).len(), 4);
    }
}
