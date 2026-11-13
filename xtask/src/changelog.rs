//! `cargo xtask changelog <version>`: prints the CHANGELOG.md section of one release
//! (Keep a Changelog format), for the GitHub release notes. A missing or empty
//! section is an error, so a version tag without a changelog entry fails the release.

use std::path::Path;

pub fn run(root: &Path, version: &str) -> Result<(), String> {
    let path = root.join("CHANGELOG.md");
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    print!("{}", section(&text, version)?);
    Ok(())
}

/// The body of `## [<version>]` (a leading `v` is ignored), up to the next `## `
/// heading or the link reference definitions.
pub fn section(text: &str, version: &str) -> Result<String, String> {
    let version = version.strip_prefix('v').unwrap_or(version);
    let heading = format!("## [{version}]");
    let mut lines = text.lines().skip_while(|l| !l.starts_with(&heading));
    if lines.next().is_none() {
        return Err(format!("CHANGELOG.md has no section `{heading}`"));
    }
    let body: Vec<&str> = lines
        .take_while(|l| !l.starts_with("## ") && !is_link_reference(l))
        .collect();
    let body = body.join("\n").trim().to_owned();
    if body.is_empty() {
        return Err(format!("CHANGELOG.md section `{heading}` is empty"));
    }
    Ok(body + "\n")
}

fn is_link_reference(line: &str) -> bool {
    line.starts_with('[') && line.contains("]: ")
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEXT: &str = "# Changelog\n\n## [Unreleased]\n\n### Added\n\n- next\n\n## [0.2.0] - 2027-01-02\n\n### Fixed\n\n- a bug\n\n## [0.1.0] - 2026-11-20\n\n### Added\n\n- first\n\n[0.2.0]: https://example.invalid/compare\n";

    #[test]
    fn extracts_one_section_with_or_without_v() {
        assert_eq!(section(TEXT, "v0.2.0").unwrap(), "### Fixed\n\n- a bug\n");
        assert_eq!(section(TEXT, "0.1.0").unwrap(), "### Added\n\n- first\n");
    }

    #[test]
    fn missing_or_empty_section_is_an_error() {
        assert!(section(TEXT, "0.3.0").unwrap_err().contains("no section"));
        let empty = "## [1.0.0] - 2027-01-01\n\n## [0.9.0]\n- x\n";
        assert!(section(empty, "1.0.0").unwrap_err().contains("empty"));
    }
}
