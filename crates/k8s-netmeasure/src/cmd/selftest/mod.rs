//! `selftest` (design §3.8): verifies on this kernel what the measurement relies on.
//! One JSON log line per check, a summary table on stdout, optionally a JSON report
//! file. Exit 0 when every check established its result, 6 otherwise.

mod lo;
mod paced;
mod report;
mod syn;

use std::collections::BTreeMap;
use std::process::ExitCode;

use netmeasure_cli::SelftestArgs;
use serde_json::json;

use self::report::{Check, Status};
use crate::build_info::BUILD_INFO;
use crate::diag::{cgroup, node};
use crate::{exit, output};

pub fn run(args: &SelftestArgs, sources: &BTreeMap<String, &'static str>) -> ExitCode {
    let mut checks = syn::run();
    checks.push(total_rto(&checks));
    checks.extend(paced::run());
    checks.push(cgroup_check());
    for c in &checks {
        tracing::info!(check = c.name, status = c.status.as_str(), summary = %c.summary, "selftest check");
        tracing::debug!(check = c.name, details = %c.details, "selftest check details");
    }
    print!("{}", report::table(&checks));
    if let Some(path) = &args.output {
        let doc = json!({
            "schema": "k8s-netmeasure/selftest", "schema_version": 1,
            "build_info": BUILD_INFO.to_json(), "node": node::read(),
            "config_source": sources, "checks": checks,
        });
        let bytes = serde_json::to_vec_pretty(&doc).unwrap_or_default();
        if let Err(e) = output::write_atomic(path, &bytes) {
            tracing::error!(path = %path.display(), error = %e, "cannot write selftest report");
            return exit::code(exit::INTERNAL);
        }
    }
    if checks.iter().any(|c| c.status == Status::Inconclusive) {
        exit::code(exit::SELFTEST_INCOMPLETE)
    } else {
        exit::code(exit::OK)
    }
}

/// `tcpi_total_rto` presence (design §3.3): decided by the `TCP_INFO` length the
/// kernel returned for the SYN check's connect; a number means the field exists.
fn total_rto(checks: &[Check]) -> Check {
    let rto = checks
        .first()
        .and_then(|c| c.details.get("total_rto").cloned());
    let present = matches!(rto, Some(serde_json::Value::Number(_)));
    Check {
        name: "tcpi-total-rto",
        status: if present {
            Status::Present
        } else {
            Status::Absent
        },
        summary: if present {
            "kernel provides tcpi_total_rto".into()
        } else {
            "tcpi_total_rto not provided by this kernel".into()
        },
        details: json!({ "total_rto": rto }),
    }
}

fn cgroup_check() -> Check {
    let r = cgroup::read();
    let quota = r
        .own
        .as_ref()
        .and_then(|l| l.cpu_max)
        .map(|m| m.millicores());
    let summary = match (&r.path, r.readable, quota) {
        (None, ..) => "no cgroup v2".to_owned(),
        (Some(_), false, _) => {
            "own cgroup not readable: run refuses to start without --no-cgroup-check".to_owned()
        }
        (Some(p), true, Some(Some(m))) => format!(
            "{p}: quota {m}m, {} visible ancestors",
            r.visible_ancestors.len()
        ),
        (Some(p), true, _) => format!(
            "{p}: no quota (max), {} visible ancestors",
            r.visible_ancestors.len()
        ),
    };
    let status = if r.readable {
        Status::Confirmed
    } else {
        Status::Unreliable
    };
    Check {
        name: "cgroup-v2",
        status,
        summary,
        details: json!(r),
    }
}
