//! Slot plan (design §3.4) and cycle plan (§3.5).
//!
//! Global slot `i` (0-based) goes to the loopback control if `(i + 1) % every == 0`,
//! otherwise round-robin to the real targets. Cycles are cut by **counted samples per
//! real target**: each holds `samples / cycles` (remainder from cycle 0), and cycle 0
//! additionally holds the warm-up. A cycle ends with the real-target slot that
//! completes its quota; a loopback slot right after it belongs to the next cycle.

use std::num::NonZeroU32;

/// What a slot measures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotTarget {
    Loopback,
    /// Index into the real targets, `0..targets`.
    Real(u32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SlotPlan {
    pub targets: NonZeroU32,
    /// Every n-th slot is loopback; at least 2 (2 = strict alternation in `pair`).
    pub loopback_every: NonZeroU32,
}

impl SlotPlan {
    pub fn new(targets: u32, loopback_every: u32) -> Result<SlotPlan, String> {
        let targets = NonZeroU32::new(targets).ok_or("at least one real target is required")?;
        if loopback_every < 2 {
            return Err(format!(
                "--loopback-every must be at least 2, got {loopback_every}"
            ));
        }
        let loopback_every = NonZeroU32::new(loopback_every).ok_or("unreachable")?;
        Ok(SlotPlan {
            targets,
            loopback_every,
        })
    }

    pub fn target(&self, slot: u64) -> SlotTarget {
        let every = u64::from(self.loopback_every.get());
        if (slot + 1).is_multiple_of(every) {
            return SlotTarget::Loopback;
        }
        let real_before = slot - (slot + 1) / every;
        SlotTarget::Real((real_before % u64::from(self.targets.get())) as u32)
    }

    /// Global slot index of the `n`-th real-target slot (0-based).
    pub fn real_slot_index(&self, n: u64) -> u64 {
        let per_block = u64::from(self.loopback_every.get()) - 1;
        n + n / per_block
    }
}

/// Inputs of the cycle plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CycleInput {
    pub samples: u64,
    pub warmup: u64,
    pub cycles: u64,
}

/// Slot count per cycle; their sum is the run's slot count.
pub fn cycle_slots(plan: &SlotPlan, input: &CycleInput) -> Result<Vec<u64>, String> {
    if input.cycles == 0 || input.cycles > input.samples {
        return Err(format!(
            "--cycles must be between 1 and --samples ({}), got {}",
            input.samples, input.cycles
        ));
    }
    let t = u64::from(plan.targets.get());
    let base = input.samples / input.cycles;
    let extra = input.samples % input.cycles;
    let mut per_target_done = input.warmup;
    let mut start = 0;
    let mut out = Vec::with_capacity(input.cycles as usize);
    for c in 0..input.cycles {
        per_target_done += base + u64::from(c < extra);
        let last_real = per_target_done * t - 1;
        let end = plan.real_slot_index(last_real) + 1;
        out.push(end - start);
        start = end;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan(t: u32, e: u32) -> SlotPlan {
        SlotPlan::new(t, e).unwrap()
    }

    #[test]
    fn design_defaults_in_pair_mode() {
        let input = CycleInput {
            samples: 5000,
            warmup: 50,
            cycles: 10,
        };
        let slots = cycle_slots(&plan(1, 10), &input).unwrap();
        assert_eq!(
            slots,
            vec![611, 555, 556, 555, 556, 555, 556, 555, 556, 556]
        );
        assert_eq!(slots.iter().sum::<u64>(), 5611);
    }

    #[test]
    fn every_cycle_holds_the_same_counted_samples_per_target() {
        let p = plan(3, 7);
        let input = CycleInput {
            samples: 1003,
            warmup: 20,
            cycles: 10,
        };
        let slots = cycle_slots(&p, &input).unwrap();
        let mut slot = 0;
        for (c, n) in slots.iter().enumerate() {
            let mut per_target = [0u64; 3];
            for _ in 0..*n {
                if let SlotTarget::Real(i) = p.target(slot) {
                    per_target[i as usize] += 1;
                }
                slot += 1;
            }
            let expected = 100 + u64::from(c < 3) + if c == 0 { 20 } else { 0 };
            assert_eq!(per_target, [expected; 3], "cycle {c}");
        }
    }

    #[test]
    fn slot_targets() {
        let p = plan(2, 4);
        let seq: Vec<_> = (0..8).map(|s| p.target(s)).collect();
        use SlotTarget::*;
        assert_eq!(
            seq,
            vec![
                Real(0),
                Real(1),
                Real(0),
                Loopback,
                Real(1),
                Real(0),
                Real(1),
                Loopback
            ]
        );
        assert_eq!(p.real_slot_index(3), 4);
        let alternate = plan(1, 2);
        assert_eq!(alternate.target(0), Real(0));
        assert_eq!(alternate.target(1), Loopback);
    }

    #[test]
    fn invalid_inputs() {
        assert!(SlotPlan::new(0, 10).is_err());
        assert!(SlotPlan::new(1, 1).unwrap_err().contains("at least 2"));
        let p = plan(1, 10);
        assert!(
            cycle_slots(
                &p,
                &CycleInput {
                    samples: 5,
                    warmup: 0,
                    cycles: 0
                }
            )
            .is_err()
        );
        assert!(
            cycle_slots(
                &p,
                &CycleInput {
                    samples: 5,
                    warmup: 0,
                    cycles: 6
                }
            )
            .is_err()
        );
    }
}
