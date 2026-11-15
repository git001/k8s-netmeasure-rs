//! Number formatting of `report`: quantile names, estimates, intervals, durations.

use netmeasure_core::stats::describe::Estimate;

pub fn pname(p: f64) -> String {
    format!("p{}", (p * 1000.0).round() / 10.0)
}

pub fn est(e: Option<Estimate>) -> String {
    match e {
        Some(Estimate::Ns(v)) => ns(v as f64),
        Some(Estimate::AtLeast(t)) => format!("≥ {}", ns(t as f64)),
        None => "-".into(),
    }
}

pub fn iv(i: Option<(f64, f64)>, ratio: bool) -> String {
    let f = |x: f64| {
        if x.is_infinite() {
            "∞".into()
        } else if ratio {
            format!("{x:.3}")
        } else {
            ns(x)
        }
    };
    i.map_or("-".into(), |(lo, hi)| format!("[{}, {}]", f(lo), f(hi)))
}

pub fn ns(v: f64) -> String {
    match v.abs() {
        a if a >= 1e9 => format!("{:.2} s", v / 1e9),
        a if a >= 1e6 => format!("{:.3} ms", v / 1e6),
        _ => format!("{:.1} µs", v / 1e3),
    }
}
