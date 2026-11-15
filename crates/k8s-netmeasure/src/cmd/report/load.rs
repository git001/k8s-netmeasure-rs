//! Reading result files for `report` and grouping them (design §10.6): files or
//! directories (searched recursively; other JSON files are skipped), one side per
//! declared set, grouped by path; within a path the source nodes form the top
//! resampling level and every run pools its real targets.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use netmeasure_core::schema::result::ResultFile;
use netmeasure_core::stats::blocklen;
use netmeasure_core::stats::series::{Series, series};
use netmeasure_util::time::parse_rfc3339_ns;

/// One run, prepared for the statistics.
pub struct RunData {
    pub file: ResultFile,
    /// All real targets of the run as segments of one series (blocks never cross
    /// targets, because every target's stretches are separate segments).
    pub targets: Series,
    pub loopback: Series,
    /// Floor of the block length in samples of one target series (§10.2).
    pub floor: f64,
    pub loopback_floor: f64,
    pub node: String,
    pub start_ns: i128,
    pub end_ns: i128,
}

/// Every result file below `paths`, sorted by name; `Err` for unreadable or invalid
/// result files.
pub fn load(paths: &[PathBuf]) -> Result<Vec<RunData>, String> {
    let mut files = Vec::new();
    for p in paths {
        collect(p, &mut files).map_err(|e| format!("{}: {e}", p.display()))?;
    }
    files.sort();
    let mut out = Vec::new();
    for f in files {
        let text = std::fs::read_to_string(&f).map_err(|e| format!("{}: {e}", f.display()))?;
        if !text.contains("\"k8s-netmeasure/result\"") {
            tracing::info!(file = %f.display(), "skipping: not a result file");
            continue;
        }
        let file: ResultFile = serde_json::from_str(&text)
            .map_err(|e| format!("{}: not a valid result file: {e}", f.display()))?;
        out.push(prepare(f, file)?);
    }
    Ok(out)
}

fn collect(p: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    if p.is_dir() {
        for entry in std::fs::read_dir(p)? {
            collect(&entry?.path(), out)?;
        }
    } else if p.is_file() {
        if p.extension().is_some_and(|e| e == "json") {
            out.push(p.to_owned());
        }
    } else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "not found",
        ));
    }
    Ok(())
}

fn prepare(path: PathBuf, file: ResultFile) -> Result<RunData, String> {
    let all = series(&file).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut iter = all.into_iter();
    let loopback = iter
        .next()
        .ok_or_else(|| format!("{}: no targets", path.display()))?;
    let real: Vec<Series> = iter.collect();
    let first = real
        .first()
        .ok_or_else(|| format!("{}: no real target", path.display()))?;
    let targets = Series {
        run_id: first.run_id.clone(),
        target: 1,
        name: "all targets".into(),
        timeout_ns: first.timeout_ns,
        segments: real.iter().flat_map(|s| s.segments.clone()).collect(),
    };
    let c = &file.run.config;
    let every = u64::from(c.loopback_every);
    let spacing = |lo| blocklen::spacing_ns(c.interval_ns, real.len() as u64, every, lo);
    let floor = blocklen::floor_samples(blocklen::FLOOR_NS, spacing(false));
    let loopback_floor = blocklen::floor_samples(blocklen::FLOOR_NS, spacing(true));
    let node = file
        .source
        .context_before
        .node_name
        .value
        .clone()
        .unwrap_or_else(|| "unknown".into());
    let time =
        |t: &str| parse_rfc3339_ns(t).ok_or_else(|| format!("{}: bad time {t}", path.display()));
    let (start_ns, end_ns) = (time(&file.run.started_at)?, time(&file.run.ended_at)?);
    Ok(RunData {
        file,
        targets,
        loopback,
        floor,
        loopback_floor,
        node,
        start_ns,
        end_ns,
    })
}

/// `path → (before runs, after runs)`, indices into the two loaded sets.
pub fn by_path(
    before: &[RunData],
    after: &[RunData],
) -> BTreeMap<String, (Vec<usize>, Vec<usize>)> {
    let mut m: BTreeMap<String, (Vec<usize>, Vec<usize>)> = BTreeMap::new();
    for (i, r) in before.iter().enumerate() {
        m.entry(r.file.run.config.path.clone())
            .or_default()
            .0
            .push(i);
    }
    for (i, r) in after.iter().enumerate() {
        m.entry(r.file.run.config.path.clone())
            .or_default()
            .1
            .push(i);
    }
    m
}
