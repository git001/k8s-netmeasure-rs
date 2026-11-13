//! Assembling and writing the result file (design §5).

use std::collections::BTreeMap;
use std::path::Path;

use netmeasure_core::measure::record::{self, Sample};
use netmeasure_core::schema::control::NodeContext;
use netmeasure_core::schema::json;
use netmeasure_core::schema::result::{
    self, Clock, Config, Cycle, Measurement, ResultFile, Run, RunStatus, SideContext, Target,
};

use crate::build_info::BUILD_INFO;
use crate::output;

/// Everything known about the run when it ends.
pub struct Finished {
    pub run_id: String,
    pub status: RunStatus,
    pub started_at: String,
    pub ended_at: String,
    pub clock: Clock,
    pub labels: BTreeMap<String, String>,
    pub config: Config,
    pub config_source: BTreeMap<String, String>,
    pub context_before: NodeContext,
    pub context_after: Option<NodeContext>,
    pub targets: Vec<Target>,
    pub cycles: Vec<Cycle>,
    pub samples: Vec<Sample>,
}

pub fn write(path: &Path, f: Finished) -> std::io::Result<()> {
    let run_id = f.run_id.clone();
    let doc = ResultFile {
        schema: result::SCHEMA.into(),
        schema_version: result::SCHEMA_VERSION,
        run: Run {
            run_id: f.run_id,
            status: f.status,
            started_at: f.started_at,
            ended_at: f.ended_at,
            clock: f.clock,
            labels: f.labels,
            build_info: BUILD_INFO.schema(),
            config: f.config,
            config_source: f.config_source,
        },
        source: SideContext {
            context_before: f.context_before,
            context_after: f.context_after,
        },
        targets: f.targets,
        measurement: Measurement {
            cycles: f.cycles,
            samples: record::columns(&f.samples),
        },
    };
    let text = json::to_text(&doc).map_err(std::io::Error::other)?;
    output::write_atomic_as(path, text.as_bytes(), &run_id)
}
