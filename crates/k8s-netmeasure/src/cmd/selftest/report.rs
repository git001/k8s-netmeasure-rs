//! Result types of `selftest` and the summary table.

use serde::Serialize;

/// What a check could establish (design §3.8). Only `Inconclusive` leads to exit 6.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// The assumption holds on this kernel.
    Confirmed,
    /// Tested, and the assumption does not hold: `run` uses `unknown` or a fallback.
    Unreliable,
    /// A kernel feature exists / does not exist (a finding, not a failure).
    Present,
    Absent,
    /// Reported values only.
    Info,
    /// The check could not establish what it tests.
    Inconclusive,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Confirmed => "confirmed",
            Status::Unreliable => "unreliable",
            Status::Present => "present",
            Status::Absent => "absent",
            Status::Info => "info",
            Status::Inconclusive => "inconclusive",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Check {
    pub name: &'static str,
    pub status: Status,
    pub summary: String,
    pub details: serde_json::Value,
}

/// One line per check, aligned, for stdout.
pub fn table(checks: &[Check]) -> String {
    let width = checks.iter().map(|c| c.name.len()).max().unwrap_or(0);
    let mut out = String::new();
    for c in checks {
        let status = c.status.as_str();
        out.push_str(&format!("{:width$}  {status:12}  {}\n", c.name, c.summary));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_is_aligned() {
        let checks = [
            Check {
                name: "a",
                status: Status::Confirmed,
                summary: "x".into(),
                details: serde_json::Value::Null,
            },
            Check {
                name: "long-name",
                status: Status::Inconclusive,
                summary: "y".into(),
                details: serde_json::Value::Null,
            },
        ];
        assert_eq!(
            table(&checks),
            "a          confirmed     x\nlong-name  inconclusive  y\n"
        );
    }
}
