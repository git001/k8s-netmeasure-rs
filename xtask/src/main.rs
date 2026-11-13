//! `cargo xtask <gate>`: every CI gate of design §12, runnable locally and identically
//! in CI. Workflow logic lives here, not in shell fragments inside YAML.
#![forbid(unsafe_code)]
#![forbid(clippy::too_many_arguments)]

mod changelog;
mod coverage;
mod deny;
mod line_limit;
mod proc;
mod test_spans;

use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = "usage: cargo xtask <gate>

gates:
  fmt          cargo fmt --check
  clippy       cargo clippy --all-targets -D warnings
  line-limit   .rs file size rule, argument-count rule (design §2, §12)
  test         cargo test --locked
  deny         cargo deny check, plus duplicate-exception review dates
  audit        cargo audit (RustSec)
  coverage     cargo llvm-cov with per-crate line thresholds
  ci           all of the above, in this order
  musl         static musl build (with cargo-auditable) and test suite (emergency
               artifact, design §1); needs musl-gcc (musl-tools) and
               cargo-auditable, not part of `ci`

tools:
  changelog <version>   print the CHANGELOG.md section of a release (release notes)";

type Gate = fn(&std::path::Path) -> Result<(), String>;

const GATES: &[(&str, Gate)] = &[
    ("fmt", |root| {
        proc::cargo(root, &["fmt", "--all", "--check"])
    }),
    ("clippy", |root| {
        proc::cargo(
            root,
            &[
                "clippy",
                "--workspace",
                "--all-targets",
                "--locked",
                "--",
                "-D",
                "warnings",
            ],
        )
    }),
    ("line-limit", line_limit::run),
    ("test", |root| {
        proc::cargo(root, &["test", "--workspace", "--locked"])
    }),
    ("deny", deny::run),
    ("audit", |root| {
        proc::cargo(root, &["audit", "--deny", "warnings"])
    }),
    ("coverage", coverage::run),
];

/// Gates outside `ci`, because they need extra system packages.
const EXTRA_GATES: &[(&str, Gate)] = &[("musl", musl)];

const MUSL: &str = "x86_64-unknown-linux-musl";

fn musl(root: &std::path::Path) -> Result<(), String> {
    proc::run(root, "rustup", &["target", "add", MUSL])?;
    proc::cargo(
        root,
        &[
            "auditable",
            "build",
            "--release",
            "--locked",
            "--target",
            MUSL,
            "--package",
            "k8s-netmeasure",
        ],
    )?;
    proc::cargo(
        root,
        &[
            "test",
            "--locked",
            "--target",
            MUSL,
            "--workspace",
            "--exclude",
            "xtask",
        ],
    )
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let root = workspace_root();
    if let [tool, version] = args.as_slice()
        && tool == "changelog"
    {
        return match changelog::run(&root, version) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("xtask changelog: {e}");
                ExitCode::FAILURE
            }
        };
    }
    let [name] = args.as_slice() else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    let selected: Vec<&(&str, Gate)> = if name == "ci" {
        GATES.iter().collect()
    } else if let Some(gate) = GATES.iter().chain(EXTRA_GATES).find(|(n, _)| n == name) {
        vec![gate]
    } else {
        eprintln!("unknown gate `{name}`\n\n{USAGE}");
        return ExitCode::from(2);
    };
    for (gate, run) in selected {
        eprintln!("==> xtask {gate}");
        if let Err(e) = run(&root) {
            eprintln!("xtask {gate}: FAILED: {e}");
            return ExitCode::FAILURE;
        }
    }
    ExitCode::SUCCESS
}

fn workspace_root() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent()
        .expect("xtask lives one level below the workspace root")
        .to_path_buf()
}
