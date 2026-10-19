//! When to run each frame, and how fast to play sound.
//!
//! In the default mode the game runs one frame per refresh of the top panel. The loop waits for
//! the panel's vblank, then sleeps until the latest moment that still leaves time to run a frame
//! and draw it before the next vblank: it reads the buttons then, so a press reaches the screen
//! at the next refresh after it is read. The time a frame takes is learned from the last few
//! seconds (their 99th percentile), with a margin.
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
}

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

    /// A frame took `d` to run and draw.
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

    /// When to start the next frame: before the next vblank by the budget.
    pub fn next_start(&self) -> Option<Duration> {
        let last = self.last_vblank?;
        Some(last + self.period.saturating_sub(self.budget()))
    }

    /// The sound's sample rate, in samples per second of machine time, that makes `out_rate`
    /// real samples a second when the game runs a frame per panel refresh.
    pub fn locked_rate(&self, out_rate: f64) -> f64 {
        out_rate * GAME_HZ * self.period.as_secs_f64()
    }
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
    fn the_start_leaves_the_frames_cost_and_a_margin() {
        let mut p = Pacer::new(59.94);
        for _ in 0..200 {
            p.frame_took(ms(2.0));
        }
        p.frame_took(ms(3.0));
        p.vblank(ms(1000.0));
        let start = p.next_start().unwrap();
        let before_next = ms(1000.0) + p.period - start;
        assert!(
            (before_next.as_secs_f64() * 1000.0 - 3.5).abs() < 0.01,
            "{before_next:?}"
        );
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
