//! The smoke grid of the simulation suite (design §11): 8 scenarios, fixed here.

use super::Model;

pub struct Scenario {
    pub name: &'static str,
    pub model: Model,
    pub effect: f64,
    /// `Some(1.0)`: the naive (non-block) bootstrap, for the negative control.
    pub block: Option<f64>,
}

const fn model(phi: f64, run_sd: f64, bursts: bool, timeouts: f64) -> Model {
    Model {
        phi,
        run_sd,
        bursts,
        timeouts,
        runs: 5,
        samples: 1000,
    }
}

pub fn smoke() -> Vec<Scenario> {
    vec![
        Scenario {
            name: "iid, no effect",
            model: model(0.0, 0.0, false, 0.0),
            effect: 1.0,
            block: None,
        },
        Scenario {
            name: "AR(1) 0.9, no effect",
            model: model(0.9, 0.0, false, 0.0),
            effect: 1.0,
            block: None,
        },
        Scenario {
            name: "run effects, no effect",
            model: model(0.5, 0.03, false, 0.0),
            effect: 1.0,
            block: None,
        },
        Scenario {
            name: "bursts, no effect",
            model: model(0.5, 0.0, true, 0.0),
            effect: 1.0,
            block: None,
        },
        Scenario {
            name: "iid, regression ×1.3",
            model: model(0.0, 0.0, false, 0.0),
            effect: 1.3,
            block: None,
        },
        Scenario {
            name: "AR(1) 0.5, ×1.15 (outside 10 %)",
            model: model(0.5, 0.0, false, 0.0),
            effect: 1.15,
            block: None,
        },
        Scenario {
            name: "2 % timeouts, no effect",
            model: model(0.0, 0.0, false, 0.02),
            effect: 1.0,
            block: None,
        },
        // One run per side: with several runs the run level of the bootstrap
        // already carries the between-run variance and the naive block hides little.
        Scenario {
            name: "naive bootstrap, AR(1) 0.95",
            model: Model {
                runs: 1,
                ..model(0.95, 0.0, false, 0.0)
            },
            effect: 1.0,
            block: Some(1.0),
        },
    ]
}
