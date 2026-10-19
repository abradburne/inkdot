//! Turns the APU's output level, which changes at arbitrary dots, into stereo samples at an
//! output rate, without aliasing.
//!
//! Each change of level is a step. A step is band-limited by adding, at its exact fractional
//! position in the output, a windowed-sinc impulse scaled by the change; a running sum of those
//! impulses then gives the band-limited level. The position of a dot in the output is a
//! fixed-point ratio ([`Synth::set_rate`]), so the frontend can resample by a fraction of a
//! percent (to lock the game's 59.73 Hz frames to the panels' 59.94 Hz) at no cost and with no
//! extra filtering.
//!
//! A high-pass filter then models the console's output capacitor, which pulls the signal back
//! to 0: Pan Docs "Audio Details" gives its charge factor per dot, 0.999958 on a DMG and
//! 0.998943 on later models; at the output rate it is that factor to the power of the dots per
//! sample.
//!
//! The kernel is computed once here (the core has no maths library): a sinc cut off at 20 kHz,
//! under a Blackman window, 16 taps, at 64 fractional positions, each normalised to sum to 1 so
//! that a step settles exactly on its new level.

use alloc::vec::Vec;

const TAPS: usize = 16;
const PHASES: usize = 64;
/// Kernel values are fractions of this.
const ONE: i32 = 1 << 15;
/// Machine dots per second.
const DOT_RATE: u64 = 1 << 22;
/// Samples kept for a caller that isn't taking them: about a second.
const MAX_BUFFERED: u64 = 1 << 16;
/// The APU's level (±480 at most) to 16-bit samples.
const SCALE: i32 = 64;

/// The band-limited synthesizer and output filter.
#[derive(Clone, Debug)]
pub struct Synth {
    kernel: Vec<[i32; TAPS]>,
    /// Output samples per dot, 32.32 fixed point.
    step: u64,
    /// The output position of `time`, 32.32 fixed point.
    pos: u64,
    time: u64,
    level: [i32; 2],
    dacs_on: bool,
    /// Impulses for samples from `base` on.
    impulses: Vec<[i32; 2]>,
    base: u64,
    sum: [i64; 2],
    capacitor: [f32; 2],
    charge: f32,
    rate: f64,
    cgb: bool,
}

impl Synth {
    /// A synthesizer giving `rate` samples per second of machine time, with a DMG's or a CGB's
    /// output filter.
    pub fn new(rate: u32, cgb: bool) -> Synth {
        let mut s = Synth {
            kernel: build_kernel(),
            step: 0,
            pos: 0,
            time: 0,
            level: [0; 2],
            dacs_on: false,
            impulses: Vec::new(),
            base: 0,
            sum: [0; 2],
            capacitor: [0.0; 2],
            charge: 0.0,
            rate: 0.0,
            cgb,
        };
        s.set_rate(rate as f64);
        s
    }

    /// Sets how many output samples a second of machine time makes. The frontend uses this to
    /// resample: 48000 × (panel rate / game rate) makes 48 kHz audio for a game run at the
    /// panel's frame rate.
    pub fn set_rate(&mut self, rate: f64) {
        self.rate = rate;
        self.step = ((rate * (1u64 << 32) as f64) / DOT_RATE as f64) as u64;
        let per_dot: f64 = if self.cgb { 0.998943 } else { 0.999958 };
        self.charge = powf(per_dot, DOT_RATE as f64 / rate) as f32;
    }

    /// The rate set by [`Synth::set_rate`].
    pub fn rate(&self) -> f64 {
        self.rate
    }

    fn advance(&mut self, time: u64) {
        if time > self.time {
            self.pos += (time - self.time) * self.step;
            self.time = time;
        }
    }

    /// The APU's output became `left`/`right` at dot `time`; `dacs_on` says whether any DAC is
    /// on (with none on, the output is disconnected and reads 0).
    pub fn set_level(&mut self, time: u64, left: i32, right: i32, dacs_on: bool) {
        self.advance(time);
        self.dacs_on = dacs_on;
        let delta = [left - self.level[0], right - self.level[1]];
        if delta == [0, 0] {
            return;
        }
        self.level = [left, right];
        let whole = self.pos >> 32;
        let phase = ((self.pos >> (32 - 6)) & (PHASES as u64 - 1)) as usize;
        // Nobody is taking the samples (a headless run): drop the oldest rather than grow.
        if whole - self.base > MAX_BUFFERED {
            let keep_from = whole - MAX_BUFFERED / 2;
            let drop = (keep_from - self.base) as usize;
            for imp in self.impulses.drain(..drop.min(self.impulses.len())) {
                self.sum[0] += imp[0] as i64;
                self.sum[1] += imp[1] as i64;
            }
            self.base = keep_from;
        }
        let start = (whole - self.base) as usize;
        if self.impulses.len() < start + TAPS {
            self.impulses.resize(start + TAPS, [0; 2]);
        }
        for (k, &h) in self.kernel[phase].iter().enumerate() {
            let cell = &mut self.impulses[start + k];
            cell[0] += delta[0] * h;
            cell[1] += delta[1] * h;
        }
    }

    /// Finishes every sample up to dot `time` and appends them, interleaved left and right, to
    /// `out`.
    pub fn take(&mut self, time: u64, out: &mut Vec<i16>) {
        self.advance(time);
        let ready = ((self.pos >> 32) - self.base) as usize;
        for i in 0..ready {
            let imp = self.impulses.get(i).copied().unwrap_or([0; 2]);
            for (side, &v) in imp.iter().enumerate() {
                self.sum[side] += v as i64;
                let level = (self.sum[side] / ONE as i64) as f32 * SCALE as f32;
                let filtered = if self.dacs_on {
                    let o = level - self.capacitor[side];
                    self.capacitor[side] = level - o * self.charge;
                    o
                } else {
                    0.0
                };
                out.push(filtered.clamp(-32768.0, 32767.0) as i16);
            }
        }
        self.impulses.drain(..ready.min(self.impulses.len()));
        self.base += ready as u64;
    }
}

fn build_kernel() -> Vec<[i32; TAPS]> {
    let cutoff = 20_000.0 / 48_000.0;
    let mut kernel = Vec::with_capacity(PHASES);
    for p in 0..PHASES {
        let frac = p as f64 / PHASES as f64;
        let mut h = [0f64; TAPS];
        for (j, v) in h.iter_mut().enumerate() {
            // Distance from the impulse, in samples, centred in the window.
            let t = j as f64 - (TAPS / 2) as f64 + 1.0 - frac;
            let x = 2.0 * cutoff * t;
            let sinc = if x > -1e-9 && x < 1e-9 {
                1.0
            } else {
                sin(core::f64::consts::PI * x) / (core::f64::consts::PI * x)
            };
            let w = (t + TAPS as f64 / 2.0) / TAPS as f64;
            let window = if (0.0..=1.0).contains(&w) {
                0.42 - 0.5 * cos(2.0 * core::f64::consts::PI * w)
                    + 0.08 * cos(4.0 * core::f64::consts::PI * w)
            } else {
                0.0
            };
            *v = sinc * window;
        }
        let total: f64 = h.iter().sum();
        let mut q = [0i32; TAPS];
        for (j, v) in h.iter().enumerate() {
            let r = (v / total) * ONE as f64;
            q[j] = if r >= 0.0 {
                (r + 0.5) as i32
            } else {
                (r - 0.5) as i32
            };
        }
        // Rounding can leave the sum off by a little; the largest tap takes the difference so a
        // step settles on exactly its level.
        let err = ONE - q.iter().sum::<i32>();
        let peak = (0..TAPS).max_by_key(|&j| q[j]).unwrap_or(0);
        q[peak] += err;
        kernel.push(q);
    }
    kernel
}

/// sin(x), to about 1e-12, from a Taylor series after reducing x to [-π/2, π/2].
fn sin(x: f64) -> f64 {
    use core::f64::consts::PI;
    let mut x = x % (2.0 * PI);
    if x > PI {
        x -= 2.0 * PI;
    } else if x < -PI {
        x += 2.0 * PI;
    }
    if x > PI / 2.0 {
        x = PI - x;
    } else if x < -PI / 2.0 {
        x = -PI - x;
    }
    let x2 = x * x;
    let mut term = x;
    let mut sum = x;
    for n in 1..12 {
        term *= -x2 / ((2 * n) as f64 * (2 * n + 1) as f64);
        sum += term;
    }
    sum
}

fn cos(x: f64) -> f64 {
    sin(x + core::f64::consts::FRAC_PI_2)
}

/// base^exp for base in (0, 1] and exp ≥ 0: exp's integer part by squaring, its fraction by a
/// series for e^(f·ln base).
fn powf(base: f64, exp: f64) -> f64 {
    let whole = exp as u64;
    let frac = exp - whole as f64;
    let mut result = 1.0;
    let mut b = base;
    let mut e = whole;
    while e > 0 {
        if e & 1 != 0 {
            result *= b;
        }
        b *= b;
        e >>= 1;
    }
    // ln(base) by the series for ln((1+y)/(1-y)), y = (base-1)/(base+1).
    let y = (base - 1.0) / (base + 1.0);
    let mut ln = 0.0;
    let mut t = y;
    for k in 0..20 {
        ln += t / (2 * k + 1) as f64;
        t *= y * y;
    }
    ln *= 2.0;
    let z = frac * ln;
    let mut exp_z = 1.0;
    let mut term = 1.0;
    for k in 1..20 {
        term *= z / k as f64;
        exp_z += term;
    }
    result * exp_z
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maths_helpers() {
        assert!((sin(1.0) - 0.8414709848078965).abs() < 1e-12);
        assert!((sin(10.0) - -0.5440211108893698).abs() < 1e-12);
        assert!((powf(0.999958, 87.38133) - 0.996337).abs() < 1e-5);
    }

    #[test]
    fn a_step_settles_on_its_level() {
        let mut s = Synth::new(48_000, false);
        s.set_level(1000, 100, -100, true);
        let mut out = Vec::new();
        s.take(1000 + 4 * 4096, &mut out);
        // Within a few samples the band-limited step has risen; the high-pass filter then pulls
        // it back slowly, so it is near 100 * SCALE a few samples in.
        let early = out[2 * 24];
        assert!((early as i32 - 100 * SCALE).abs() < 200, "{early}");
        assert!(out[out.len() - 2] < early);
    }

    #[test]
    fn rate_gives_samples_per_second() {
        let mut s = Synth::new(48_000, false);
        let mut out = Vec::new();
        s.take(DOT_RATE, &mut out);
        assert_eq!(out.len(), 2 * 48_000);
    }

    #[test]
    fn no_dac_means_silence() {
        let mut s = Synth::new(48_000, false);
        s.set_level(10, 300, 300, false);
        let mut out = Vec::new();
        s.take(100_000, &mut out);
        assert!(out.iter().all(|&v| v == 0));
    }
}
