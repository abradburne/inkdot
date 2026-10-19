//! When to run each frame, and how fast to play sound.
//!
//! In the default mode the game runs one frame per refresh of the top panel. The loop waits for
//! the panel's vblank, then sleeps until the latest moment that still leaves time to run a frame
//! and hand it to the display before its deadline: it reads the buttons then, so a press reaches
//! the screen at the next refresh after it is read. The time a frame takes is learned from the
//! last few seconds (their 99th percentile), with a margin.
//!
//! The deadline is not the next vblank. Measured on the DS+ under nibOS (frame logs from
//! INKDOT_DEBUG=frames, Boulder Dash): a swap that ends more than ~7 ms after the vblank
//! timestamp starts to miss the next refresh, and past 9.5 ms it almost always does, so the frame
//! shows a refresh late and the one after waits for it. [`DEADLINE`] keeps the swap inside that.
//! (The likely reason: the panel takes a new picture as its scan starts, some 7 ms after the
//! vblank event.) A frame whose cost doesn't fit between a vblank and its deadline starts before
//! the vblank instead, so the plan is made from the vblanks' times, which a thread of their own
//! records, rather than by waiting for each one.
//!
//! The game's frame rate (4194304 / 70224 = 59.7275 Hz) is a little below the panels' (59.94 Hz
//! on the DS+), so it runs 0.35% fast, and the sound is made for that: the synthesizer's rate is
//! set so each frame's machine time gives the samples one panel refresh plays. A small, slow
//! correction then keeps the sound queue at its target as the two clocks drift.

use std::collections::VecDeque;
use std::time::Duration;

/// The game's frames per second.
pub const GAME_HZ: f64 = 4_194_304.0 / 70_224.0;

/// Learns the panel's period and the frame's cost, and says when to start the next frame.
#[derive(Clone, Debug)]
pub struct Pacer {
    period: Duration,
    last_vblank: Option<Duration>,
    intervals: VecDeque<Duration>,
    work: VecDeque<Duration>,
    margin: Duration,
    /// The deadline the last frame was planned for.
    last_target: Option<Duration>,
}

/// How long after the vblank the frame must be handed to the display to be shown at the next one
/// (see the module's notes).
pub const DEADLINE: Duration = Duration::from_micros(6_500);

/// How many frames of history to keep: four seconds.
const HISTORY: usize = 240;

impl Pacer {
    /// A pacer for a panel refreshing at `hz`.
    pub fn new(hz: f64) -> Pacer {
        Pacer {
            period: Duration::from_secs_f64(1.0 / hz),
            last_vblank: None,
            intervals: VecDeque::with_capacity(HISTORY),
            work: VecDeque::with_capacity(HISTORY),
            margin: Duration::from_micros(1500),
            last_target: None,
        }
    }

    /// The panel's period as measured.
    pub fn period(&self) -> Duration {
        self.period
    }

    /// A vblank happened at `at` (monotonic time).
    pub fn vblank(&mut self, at: Duration) {
        if let Some(last) = self.last_vblank {
            let gap = at.saturating_sub(last);
            // A gap of about one period refines it; a missed vblank or a suspend doesn't.
            let expected = self.period.as_secs_f64();
            let ratio = gap.as_secs_f64() / expected;
            if (0.9..1.1).contains(&ratio) {
                if self.intervals.len() == HISTORY {
                    self.intervals.pop_front();
                }
                self.intervals.push_back(gap);
                if self.intervals.len() >= 30 {
                    let sum: Duration = self.intervals.iter().sum();
                    self.period = sum / self.intervals.len() as u32;
                }
            }
        }
        self.last_vblank = Some(at);
    }

    /// A frame took `d` to run and draw, up to handing it to the display (not counting any wait
    /// in the swap for the previous frame's flip, which would push the start ever earlier).
    pub fn frame_took(&mut self, d: Duration) {
        if self.work.len() == HISTORY {
            self.work.pop_front();
        }
        self.work.push_back(d);
    }

    /// The time a frame is budgeted: the 99th percentile of the recent ones plus the margin,
    /// and never more than most of a period.
    pub fn budget(&self) -> Duration {
        let mut w: Vec<Duration> = self.work.iter().copied().collect();
        w.sort();
        let p99 = w
            .get((w.len() * 99) / 100)
            .or(w.last())
            .copied()
            .unwrap_or(Duration::from_millis(4));
        (p99 + self.margin).min(self.period * 9 / 10)
    }

    /// The next frame's plan, at `now`: the first deadline it can still make with its budget,
    /// after the last frame's (never two frames for one refresh), and when to start for it.
    /// `None` until a vblank has been seen.
    pub fn plan(&mut self, now: Duration) -> Option<Plan> {
        let last = self.last_vblank?;
        let budget = self.budget();
        let mut deadline = last + DEADLINE;
        // Back to the first deadline after the last frame's, then on to one there is time for.
        while deadline > last + self.period && deadline.saturating_sub(self.period) > now + budget {
            deadline -= self.period;
        }
        if let Some(t) = self.last_target {
            while deadline < t + self.period / 2 {
                deadline += self.period;
            }
        }
        while deadline < now + budget {
            deadline += self.period;
        }
        let repeated = self.last_target.map_or(0, |t| {
            let gap = (deadline - t).as_secs_f64() / self.period.as_secs_f64();
            (gap.round() as u64).saturating_sub(1)
        });
        self.last_target = Some(deadline);
        Some(Plan {
            start: deadline - budget,
            deadline,
            repeated,
        })
    }

    /// The sound's sample rate, in samples per second of machine time, that makes `out_rate`
    /// real samples a second when the game runs a frame per panel refresh.
    pub fn locked_rate(&self, out_rate: f64) -> f64 {
        out_rate * GAME_HZ * self.period.as_secs_f64()
    }
}

/// When to run a frame and for which refresh.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Plan {
    /// When to read the buttons and start.
    pub start: Duration,
    /// When the frame must be handed to the display.
    pub deadline: Duration,
    /// Refreshes since the last frame's that show it again (0 when every refresh gets a frame).
    pub repeated: u64,
}

/// Keeps the sound queue near a target by nudging the sample rate by up to ±0.5%.
#[derive(Clone, Copy, Debug)]
pub struct RateControl {
    /// The queue level aimed at, in sample frames.
    pub target: f64,
    smoothed: f64,
}

impl RateControl {
    pub fn new(target_frames: u32) -> RateControl {
        RateControl {
            target: target_frames as f64,
            smoothed: target_frames as f64,
        }
    }

    /// The rate to use, given the base rate and the queue's level now. Too full: slower, so
    /// fewer samples are made; too empty: faster.
    pub fn rate(&mut self, base: f64, queued_frames: u32) -> f64 {
        // The level jumps by a frame's worth every frame; smoothing over ~half a second keeps
        // the pitch steady.
        self.smoothed += (queued_frames as f64 - self.smoothed) * 0.03;
        let error = ((self.target - self.smoothed) / self.target).clamp(-1.0, 1.0);
        base * (1.0 + 0.005 * error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(x: f64) -> Duration {
        Duration::from_secs_f64(x / 1000.0)
    }

    #[test]
    fn the_period_is_learned_and_misses_are_ignored() {
        let mut p = Pacer::new(60.0);
        let mut t = 0.0;
        for i in 0..100 {
            // One missed vblank (a double gap) in the middle.
            t += if i == 50 { 2.0 * 16.6838 } else { 16.6838 };
            p.vblank(ms(t));
        }
        assert!((p.period.as_secs_f64() * 1000.0 - 16.6838).abs() < 0.001);
    }

    #[test]
    fn frames_start_their_budget_before_each_deadline() {
        let mut p = Pacer::new(59.94);
        let period = p.period;
        for _ in 0..200 {
            p.frame_took(ms(2.0));
        }
        p.frame_took(ms(3.0));
        // At the vblank: this refresh's deadline, 3.5 ms (the cost and margin) before it.
        p.vblank(ms(1000.0));
        let plan = p.plan(ms(1000.1)).unwrap();
        assert_eq!(plan.deadline, ms(1000.0) + DEADLINE);
        assert_eq!(plan.deadline - plan.start, ms(3.5));
        assert_eq!(plan.repeated, 0);
        // The next frame, planned straight after: the next refresh, not this one again.
        let next = p.plan(ms(1002.0)).unwrap();
        assert_eq!(next.deadline, plan.deadline + period);
        assert_eq!(next.repeated, 0);
    }

    /// A frame that can't fit between a vblank and its deadline starts before the vblank, and
    /// every refresh still gets a frame.
    #[test]
    fn a_long_frame_starts_before_the_vblank() {
        let mut p = Pacer::new(59.94);
        let period = p.period;
        for _ in 0..240 {
            p.frame_took(ms(8.0));
        }
        p.vblank(ms(1000.0));
        let first = p.plan(ms(1000.1)).unwrap();
        // 9.5 ms of budget: this refresh's deadline is too close, so the next one's.
        assert_eq!(first.deadline, ms(1000.0) + period + DEADLINE);
        assert_eq!(first.start, first.deadline - ms(9.5));
        assert!(first.start < ms(1000.0) + period, "before the next vblank");
        let second = p.plan(first.deadline).unwrap();
        assert_eq!(second.deadline, first.deadline + period);
        assert_eq!(second.repeated, 0);
    }

    /// Running late (a frame over its budget) skips a refresh, and says so.
    #[test]
    fn a_late_frame_repeats_one_refresh() {
        let mut p = Pacer::new(59.94);
        let period = p.period;
        p.vblank(ms(1000.0));
        let first = p.plan(ms(1000.0)).unwrap();
        // Planned with less than the budget left before the next refresh's deadline.
        let late = p.plan(first.deadline + period - ms(2.0)).unwrap();
        assert_eq!(late.deadline, first.deadline + 2 * period);
        assert_eq!(late.repeated, 1);
    }

    #[test]
    fn locked_sound_rate() {
        let p = Pacer::new(59.937);
        // 48000 samples a real second at 59.937 frames a second, from 59.7275 frames of
        // machine time: about 47832 a machine second.
        assert!((p.locked_rate(48_000.0) - 47_831.9).abs() < 1.0);
    }

    #[test]
    fn rate_control_leans_against_the_queue() {
        let mut r = RateControl::new(1200);
        let full = (0..200).map(|_| r.rate(48_000.0, 2400)).last().unwrap();
        assert!((48_000.0 * 0.995..48_000.0).contains(&full));
        let mut r = RateControl::new(1200);
        let empty = (0..200).map(|_| r.rate(48_000.0, 0)).last().unwrap();
        assert!(empty > 48_000.0 && empty <= 48_000.0 * 1.005, "{empty}");
    }
}
