//! Pacing arithmetic (design §3.4), on monotonic nanoseconds. The four overrun rules:
//! 1. a slot whose sample runs past the next slot's deadline completes normally;
//! 2. the following slot starts at once, with `scheduled = actual start` (re-anchor);
//! 3. from there the full interval applies again;
//! 4. missed slots are never made up: no burst, the run takes longer instead.

/// When the next slot may start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Next {
    /// Sleep until this absolute monotonic time (ns), then start.
    At(u64),
    /// Start at once: the previous sample overran (rule 2).
    Immediately,
}

/// Timing facts of one slot, stored per sample.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SlotTiming {
    pub scheduled_ns: u64,
    /// `actual start − scheduled start`, ≥ 0.
    pub late_ns: u64,
    /// Started at once after an overrun (re-anchored).
    pub after_overrun: bool,
}

#[derive(Debug, Clone)]
pub struct Pacer {
    interval_ns: u64,
    next: Next,
    scheduled_ns: u64,
}

impl Pacer {
    /// The first slot is scheduled at `first_ns`.
    pub fn new(first_ns: u64, interval_ns: u64) -> Pacer {
        Pacer {
            interval_ns,
            next: Next::At(first_ns),
            scheduled_ns: first_ns,
        }
    }

    pub fn next(&self) -> Next {
        self.next
    }

    /// Records the actual start of the slot announced by [`Pacer::next`].
    pub fn begin(&mut self, actual_ns: u64) -> SlotTiming {
        match self.next {
            Next::At(deadline) => {
                self.scheduled_ns = deadline;
                SlotTiming {
                    scheduled_ns: deadline,
                    late_ns: actual_ns.saturating_sub(deadline),
                    after_overrun: false,
                }
            }
            Next::Immediately => {
                self.scheduled_ns = actual_ns;
                SlotTiming {
                    scheduled_ns: actual_ns,
                    late_ns: 0,
                    after_overrun: true,
                }
            }
        }
    }

    /// Records the end of the slot's sample and plans the next slot. Returns `true`
    /// if this sample overran the next deadline.
    pub fn end(&mut self, end_ns: u64) -> bool {
        let candidate = self.scheduled_ns.saturating_add(self.interval_ns);
        let overran = end_ns > candidate;
        self.next = if overran {
            Next::Immediately
        } else {
            Next::At(candidate)
        };
        overran
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS: u64 = 1_000_000;

    #[test]
    fn regular_slots_follow_the_interval_and_record_lateness() {
        let mut p = Pacer::new(100 * MS, 10 * MS);
        assert_eq!(p.next(), Next::At(100 * MS));
        let t = p.begin(100 * MS + 30_000);
        assert_eq!(
            t,
            SlotTiming {
                scheduled_ns: 100 * MS,
                late_ns: 30_000,
                after_overrun: false
            }
        );
        assert!(!p.end(101 * MS));
        assert_eq!(
            p.next(),
            Next::At(110 * MS),
            "lateness does not shift the schedule"
        );
    }

    #[test]
    fn the_four_overrun_rules() {
        let mut p = Pacer::new(0, 10 * MS);
        p.begin(0);
        // Rule 1: the overrunning sample completes normally (ends at 35 ms).
        assert!(p.end(35 * MS));
        // Rule 2: the next slot starts at once and is re-anchored.
        assert_eq!(p.next(), Next::Immediately);
        let t = p.begin(35 * MS + 2_000);
        assert_eq!(
            t,
            SlotTiming {
                scheduled_ns: 35 * MS + 2_000,
                late_ns: 0,
                after_overrun: true
            }
        );
        assert!(!p.end(36 * MS));
        // Rule 3: the slot after that is exactly one interval later.
        assert_eq!(p.next(), Next::At(45 * MS + 2_000));
        // Rule 4: nothing is made up — slots at 10, 20, 30 ms never happen.
        let t = p.begin(45 * MS + 2_000);
        assert!(!t.after_overrun);
        assert!(!p.end(46 * MS));
        assert_eq!(p.next(), Next::At(55 * MS + 2_000));
    }

    #[test]
    fn ending_exactly_on_the_deadline_is_not_an_overrun() {
        let mut p = Pacer::new(0, 10 * MS);
        p.begin(0);
        assert!(!p.end(10 * MS));
        assert_eq!(p.next(), Next::At(10 * MS));
    }

    #[test]
    fn a_late_start_that_ends_after_the_next_deadline_is_an_overrun() {
        let mut p = Pacer::new(0, 10 * MS);
        let t = p.begin(9 * MS);
        assert_eq!(t.late_ns, 9 * MS);
        assert!(p.end(11 * MS));
        assert_eq!(p.next(), Next::Immediately);
    }
}
