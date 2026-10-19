//! The audio processing unit: two pulse channels (the first with a period sweep), a wave
//! channel and a noise channel, mixed to two outputs.
//!
//! The channels run on machine time (dots), whatever the CPU's speed. Their length timers,
//! envelopes and the sweep are clocked by the DIV-APU, a counter stepped by a falling edge of
//! the system counter's bit 12 (13 at double speed), which the bus reports through
//! [`Apu::div_apu_step`].
//!
//! Each channel produces a digital value 0-15; its DAC turns that into an analog level, the
//! mixer adds the channels to each side and scales by the master volume. The level changes are
//! turned into 48 kHz samples by [`crate::synth::Synth`], band-limited, then a high-pass filter
//! takes out the DC offset as the console's output capacitor does.
//!
//! Sources: Pan Docs "Audio", "Audio Registers", "Audio Details" (including "Obscure
//! Behavior"); Blargg's dmg_sound and cgb_sound test ROMs and their sources.

use crate::synth::Synth;

/// Bits that read as 1 in $FF10-$FF26, whatever was written.
const READ_OR: [u8; 0x17] = [
    0x80, 0x3f, 0x00, 0xff, 0xbf, // NR10-NR14
    0xff, 0x3f, 0x00, 0xff, 0xbf, // -, NR21-NR24
    0x7f, 0xff, 0x9f, 0xff, 0xbf, // NR30-NR34
    0xff, 0xff, 0x00, 0x00, 0xbf, // -, NR41-NR44
    0x00, 0x00, 0x70, // NR50-NR52
];

/// The pulse channels' waveforms by duty setting, step 0 in bit 7 (Pan Docs "NR11").
const DUTY: [u8; 4] = [0b0000_0001, 0b1000_0001, 0b1000_0111, 0b0111_1110];

/// A length timer, counting down to 0 from 64 (or 256 for the wave channel) minus what was
/// written; the channel turns off when it reaches 0 while enabled.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Length {
    left: u16,
    enabled: bool,
}

/// A volume envelope (NRx2).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Envelope {
    /// NRx2 as written.
    reg: u8,
    volume: u8,
    timer: u8,
}

impl Envelope {
    fn dac_on(&self) -> bool {
        self.reg & 0xf8 != 0
    }

    fn period(&self) -> u8 {
        self.reg & 7
    }

    fn trigger(&mut self, envelope_next: bool) {
        self.volume = self.reg >> 4;
        // A period of 0 counts as 8 for the timer; triggered just before the envelope's step,
        // the timer starts one higher.
        self.timer = if self.period() == 0 { 8 } else { self.period() } + envelope_next as u8;
    }

    fn clock(&mut self) {
        if self.period() == 0 {
            return;
        }
        self.timer = self.timer.saturating_sub(1);
        if self.timer == 0 {
            self.timer = self.period();
            if self.reg & 8 != 0 && self.volume < 15 {
                self.volume += 1;
            } else if self.reg & 8 == 0 && self.volume > 0 {
                self.volume -= 1;
            }
        }
    }
}

/// A pulse channel; channel 1 also uses the sweep fields.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Pulse {
    on: bool,
    duty: u8,
    step: u8,
    /// Set by the first trigger after power-on: until then the duty step doesn't move.
    started: bool,
    period: u16,
    /// Units of 4 dots until the next duty step.
    timer: u16,
    length: Length,
    env: Envelope,
    // Sweep (channel 1).
    nr10: u8,
    sweep_on: bool,
    shadow: u16,
    sweep_timer: u8,
    /// A subtraction was calculated since the last trigger: clearing the direction bit now
    /// turns the channel off.
    negated: bool,
}

impl Pulse {
    fn output(&self) -> u8 {
        if self.on && DUTY[self.duty as usize] >> (7 - self.step) & 1 != 0 {
            self.env.volume
        } else {
            0
        }
    }

    /// Advances by `units` of 4 dots; returns whether the output may have changed.
    fn run(&mut self, units: u16) -> bool {
        let mut changed = false;
        let mut left = units;
        while left > 0 {
            if self.timer > left {
                self.timer -= left;
                break;
            }
            left -= self.timer;
            self.timer = 2048 - self.period;
            if self.started {
                self.step = (self.step + 1) & 7;
                changed = true;
            }
        }
        changed
    }

    fn sweep_calc(&mut self) -> u16 {
        let delta = self.shadow >> (self.nr10 & 7);
        let new = if self.nr10 & 8 != 0 {
            self.negated = true;
            self.shadow.wrapping_sub(delta)
        } else {
            self.shadow + delta
        };
        if new > 2047 {
            self.on = false;
        }
        new
    }

    fn sweep_pace(&self) -> u8 {
        (self.nr10 >> 4) & 7
    }

    fn sweep_clock(&mut self) {
        self.sweep_timer = self.sweep_timer.saturating_sub(1);
        if self.sweep_timer > 0 {
            return;
        }
        let pace = self.sweep_pace();
        self.sweep_timer = if pace == 0 { 8 } else { pace };
        if self.sweep_on && pace != 0 {
            let new = self.sweep_calc();
            if new <= 2047 && self.nr10 & 7 != 0 {
                self.shadow = new;
                self.period = new;
                self.sweep_calc();
            }
        }
    }

    fn trigger(&mut self, envelope_next: bool, sweep: bool) {
        self.on = self.env.dac_on();
        self.started = true;
        // The low two bits of the period divider are kept.
        self.timer = (self.timer & 3) | ((2048 - self.period) & !3);
        self.env.trigger(envelope_next);
        if sweep {
            self.shadow = self.period;
            let pace = self.sweep_pace();
            self.sweep_timer = if pace == 0 { 8 } else { pace };
            self.sweep_on = pace != 0 || self.nr10 & 7 != 0;
            self.negated = false;
            if self.nr10 & 7 != 0 {
                self.sweep_calc();
            }
        }
    }
}

/// The wave channel.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Wave {
    on: bool,
    dac: bool,
    level: u8,
    period: u16,
    /// Units of 2 dots until the next sample.
    timer: u16,
    /// The sample index (0-31) last read.
    position: u8,
    /// The sample last read, which is what the channel outputs.
    sample: u8,
    /// Dots since wave RAM was last read, for the DMG's access window.
    since_read: u32,
    length: Length,
    ram: [u8; 16],
}

impl Wave {
    fn output(&self) -> u8 {
        if !self.on {
            return 0;
        }
        match self.level {
            0 => 0,
            n => self.sample >> (n - 1),
        }
    }

    fn run(&mut self, units: u16) -> bool {
        let mut changed = false;
        let mut left = units;
        self.since_read = self.since_read.saturating_add(units as u32 * 2);
        while left > 0 {
            if self.timer > left {
                self.timer -= left;
                break;
            }
            left -= self.timer;
            self.timer = 2048 - self.period;
            self.position = (self.position + 1) & 31;
            let byte = self.ram[self.position as usize / 2];
            self.sample = if self.position & 1 == 0 {
                byte >> 4
            } else {
                byte & 0xf
            };
            self.since_read = left as u32 * 2;
            changed = true;
        }
        changed
    }
}

/// The noise channel.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Noise {
    on: bool,
    nr43: u8,
    lfsr: u16,
    /// Dots until the next LFSR clock.
    timer: u32,
    length: Length,
    env: Envelope,
}

impl Noise {
    fn period_dots(&self) -> u32 {
        let divider = (self.nr43 & 7) as u32;
        let base = if divider == 0 { 8 } else { divider * 16 };
        base << (self.nr43 >> 4)
    }

    fn output(&self) -> u8 {
        if self.on && self.lfsr & 1 != 0 {
            self.env.volume
        } else {
            0
        }
    }

    fn clock_lfsr(&mut self) {
        let bit = !(self.lfsr ^ (self.lfsr >> 1)) & 1;
        self.lfsr = (self.lfsr & 0x7fff) | bit << 15;
        if self.nr43 & 8 != 0 {
            self.lfsr = (self.lfsr & !0x80) | bit << 7;
        }
        self.lfsr >>= 1;
    }

    fn run(&mut self, dots: u32) -> bool {
        // Clock shifts 14 and 15 give the LFSR no clocks at all.
        if self.nr43 >> 4 >= 14 {
            return false;
        }
        let mut changed = false;
        let mut left = dots;
        while left > 0 {
            if self.timer > left {
                self.timer -= left;
                break;
            }
            left -= self.timer;
            self.timer = self.period_dots();
            self.clock_lfsr();
            changed = true;
        }
        changed
    }
}

/// The APU.
#[derive(Clone, Debug)]
pub struct Apu {
    cgb: bool,
    powered: bool,
    ch1: Pulse,
    ch2: Pulse,
    ch3: Wave,
    ch4: Noise,
    nr50: u8,
    nr51: u8,
    /// The DIV-APU step that comes next (0-7): length on even steps, sweep on 2 and 6,
    /// envelope on 7.
    frame_step: u8,
    /// Dots left over from counts that weren't a multiple of the channels' 4-dot unit.
    dot_phase: u32,
    /// Machine time in dots.
    now: u64,
    synth: Synth,
}

impl Apu {
    /// An APU for a DMG or a CGB, producing `rate` samples a second of machine time.
    pub fn new(cgb: bool, rate: u32) -> Apu {
        Apu {
            cgb,
            powered: false,
            ch1: Pulse::default(),
            ch2: Pulse::default(),
            ch3: Wave::default(),
            ch4: Noise::default(),
            nr50: 0,
            nr51: 0,
            frame_step: 0,
            dot_phase: 0,
            now: 0,
            synth: Synth::new(rate, cgb),
        }
    }

    /// The output: band-limited stereo samples.
    pub fn synth(&mut self) -> &mut Synth {
        &mut self.synth
    }

    /// Appends every finished sample, interleaved left and right, to `out`.
    pub fn take_samples(&mut self, out: &mut alloc::vec::Vec<i16>) {
        self.synth.take(self.now, out);
    }

    fn length_next(&self) -> bool {
        self.frame_step & 1 == 0
    }

    /// One step of the DIV-APU.
    pub fn div_apu_step(&mut self) {
        if !self.powered {
            return;
        }
        let step = self.frame_step;
        self.frame_step = (self.frame_step + 1) & 7;
        if step & 1 == 0 {
            clock_length(&mut self.ch1.length, &mut self.ch1.on);
            clock_length(&mut self.ch2.length, &mut self.ch2.on);
            clock_length(&mut self.ch3.length, &mut self.ch3.on);
            clock_length(&mut self.ch4.length, &mut self.ch4.on);
        }
        if step == 2 || step == 6 {
            self.ch1.sweep_clock();
        }
        if step == 7 {
            self.ch1.env.clock();
            self.ch2.env.clock();
            self.ch4.env.clock();
        }
        self.output_changed();
    }

    /// Runs `dots` of machine time.
    pub fn tick(&mut self, dots: u32) {
        self.now += dots as u64;
        if !self.powered {
            return;
        }
        // The pulse channels count in 4-dot units, the wave channel in 2-dot units.
        let total = self.dot_phase + dots;
        let pulse_units = (total / 4) as u16;
        let wave_units = (total / 2 - self.dot_phase / 2) as u16;
        self.dot_phase = total % 4;
        let mut changed = false;
        if pulse_units > 0 {
            changed |= self.ch1.run(pulse_units);
            changed |= self.ch2.run(pulse_units);
        }
        if wave_units > 0 && self.ch3.on {
            changed |= self.ch3.run(wave_units);
        }
        if self.ch4.on {
            changed |= self.ch4.run(dots);
        }
        if changed {
            self.output_changed();
        }
    }

    /// The analog level of one channel, in units of 1/7.5 of the DAC's range: digital 0 is
    /// +15, 15 is -15, and a DAC that is off outputs 0.
    fn analog(dac: bool, digital: u8) -> i32 {
        if dac {
            15 - 2 * digital as i32
        } else {
            0
        }
    }

    fn output_changed(&mut self) {
        let levels = [
            Self::analog(self.ch1.env.dac_on(), self.ch1.output()),
            Self::analog(self.ch2.env.dac_on(), self.ch2.output()),
            Self::analog(self.ch3.dac, self.ch3.output()),
            Self::analog(self.ch4.env.dac_on(), self.ch4.output()),
        ];
        let mut left = 0;
        let mut right = 0;
        for (i, level) in levels.iter().enumerate() {
            if self.nr51 & (0x10 << i) != 0 {
                left += level;
            }
            if self.nr51 & (1 << i) != 0 {
                right += level;
            }
        }
        left *= ((self.nr50 >> 4) & 7) as i32 + 1;
        right *= (self.nr50 & 7) as i32 + 1;
        let any_dac =
            self.ch1.env.dac_on() || self.ch2.env.dac_on() || self.ch3.dac || self.ch4.env.dac_on();
        self.synth
            .set_level(self.now, left, right, self.powered && any_dac);
    }

    /// The channels' digital outputs, as PCM12 and PCM34 read them on a CGB.
    pub fn pcm(&self) -> (u8, u8) {
        (
            self.ch1.output() | self.ch2.output() << 4,
            self.ch3.output() | self.ch4.output() << 4,
        )
    }

    /// The wave RAM byte the channel is reading, which is all the CPU reaches while it plays.
    fn wave_byte_in_use(&self) -> usize {
        self.ch3.position as usize / 2
    }

    /// Whether the CPU can reach wave RAM while the channel plays: on a CGB always (the byte
    /// in use), on a DMG only in the cycle the channel reads it.
    fn wave_reachable(&self) -> bool {
        self.cgb || self.ch3.since_read < 2
    }

    /// Reads $FF10-$FF3F.
    pub fn read(&self, addr: u16) -> u8 {
        match addr {
            0xff30..=0xff3f => {
                let i = addr as usize - 0xff30;
                if !self.ch3.on {
                    self.ch3.ram[i]
                } else if self.wave_reachable() {
                    self.ch3.ram[self.wave_byte_in_use()]
                } else {
                    0xff
                }
            }
            0xff26 => {
                let on = [self.ch1.on, self.ch2.on, self.ch3.on, self.ch4.on];
                let bits = on
                    .iter()
                    .enumerate()
                    .fold(0, |b, (i, &o)| b | (o as u8) << i);
                0x70 | (self.powered as u8) << 7 | bits
            }
            0xff10..=0xff25 => self.register(addr) | READ_OR[addr as usize - 0xff10],
            _ => 0xff,
        }
    }

    /// The readable part of a register as stored.
    fn register(&self, addr: u16) -> u8 {
        let (c1, c2, c3, c4) = (&self.ch1, &self.ch2, &self.ch3, &self.ch4);
        match addr {
            0xff10 => c1.nr10,
            0xff11 => c1.duty << 6,
            0xff12 => c1.env.reg,
            0xff14 => (c1.length.enabled as u8) << 6,
            0xff16 => c2.duty << 6,
            0xff17 => c2.env.reg,
            0xff19 => (c2.length.enabled as u8) << 6,
            0xff1a => (c3.dac as u8) << 7,
            0xff1c => c3.level << 5,
            0xff1e => (c3.length.enabled as u8) << 6,
            0xff21 => c4.env.reg,
            0xff22 => c4.nr43,
            0xff23 => (c4.length.enabled as u8) << 6,
            0xff24 => self.nr50,
            0xff25 => self.nr51,
            _ => 0,
        }
    }

    /// Writes $FF10-$FF3F.
    pub fn write(&mut self, addr: u16, v: u8) {
        match addr {
            0xff30..=0xff3f => {
                let i = addr as usize - 0xff30;
                if !self.ch3.on {
                    self.ch3.ram[i] = v;
                } else if self.wave_reachable() {
                    let j = self.wave_byte_in_use();
                    self.ch3.ram[j] = v;
                }
                return;
            }
            0xff26 => {
                let on = v & 0x80 != 0;
                if self.powered && !on {
                    self.power_off();
                } else if !self.powered && on {
                    self.powered = true;
                    self.frame_step = 0;
                }
            }
            // While powered off, only the length timers can be written, and only on a DMG.
            _ if !self.powered => {
                if !self.cgb {
                    match addr {
                        0xff11 => self.ch1.length.left = 64 - (v & 63) as u16,
                        0xff16 => self.ch2.length.left = 64 - (v & 63) as u16,
                        0xff1b => self.ch3.length.left = 256 - v as u16,
                        0xff20 => self.ch4.length.left = 64 - (v & 63) as u16,
                        _ => {}
                    }
                }
                return;
            }
            0xff10 => {
                let c = &mut self.ch1;
                if c.negated && c.nr10 & 8 != 0 && v & 8 == 0 {
                    c.on = false;
                }
                c.nr10 = v & 0x7f;
            }
            0xff11 | 0xff16 => {
                let c = if addr == 0xff11 {
                    &mut self.ch1
                } else {
                    &mut self.ch2
                };
                c.duty = v >> 6;
                c.length.left = 64 - (v & 63) as u16;
            }
            0xff12 | 0xff17 | 0xff21 => {
                let (env, on) = match addr {
                    0xff12 => (&mut self.ch1.env, &mut self.ch1.on),
                    0xff17 => (&mut self.ch2.env, &mut self.ch2.on),
                    _ => (&mut self.ch4.env, &mut self.ch4.on),
                };
                env.reg = v;
                if !env.dac_on() {
                    *on = false;
                }
            }
            0xff13 => self.ch1.period = (self.ch1.period & 0x700) | v as u16,
            0xff18 => self.ch2.period = (self.ch2.period & 0x700) | v as u16,
            0xff14 | 0xff19 => {
                let length_next = self.length_next();
                let envelope_next = self.frame_step == 7;
                let c = if addr == 0xff14 {
                    &mut self.ch1
                } else {
                    &mut self.ch2
                };
                c.period = (c.period & 0xff) | ((v & 7) as u16) << 8;
                length_write(&mut c.length, &mut c.on, v, 64, length_next);
                if v & 0x80 != 0 {
                    c.trigger(envelope_next, addr == 0xff14);
                }
            }
            0xff1a => {
                self.ch3.dac = v & 0x80 != 0;
                if !self.ch3.dac {
                    self.ch3.on = false;
                }
            }
            0xff1b => self.ch3.length.left = 256 - v as u16,
            0xff1c => self.ch3.level = (v >> 5) & 3,
            0xff1d => self.ch3.period = (self.ch3.period & 0x700) | v as u16,
            0xff1e => {
                let length_next = self.length_next();
                let c = &mut self.ch3;
                c.period = (c.period & 0xff) | ((v & 7) as u16) << 8;
                length_write(&mut c.length, &mut c.on, v, 256, length_next);
                if v & 0x80 != 0 {
                    self.trigger_wave();
                }
            }
            0xff20 => self.ch4.length.left = 64 - (v & 63) as u16,
            0xff22 => self.ch4.nr43 = v,
            0xff23 => {
                let length_next = self.length_next();
                let envelope_next = self.frame_step == 7;
                let c = &mut self.ch4;
                length_write(&mut c.length, &mut c.on, v, 64, length_next);
                if v & 0x80 != 0 {
                    c.on = c.env.dac_on();
                    c.env.trigger(envelope_next);
                    c.lfsr = 0;
                    c.timer = c.period_dots();
                }
            }
            0xff24 => self.nr50 = v,
            0xff25 => self.nr51 = v,
            _ => {}
        }
        self.output_changed();
    }

    fn trigger_wave(&mut self) {
        let c = &mut self.ch3;
        // On a DMG, retriggering while the channel is reading wave RAM corrupts its first bytes
        // (Pan Docs "Obscure Behavior").
        if !self.cgb && c.on && c.timer == 1 {
            let next = ((c.position + 1) & 31) as usize / 2;
            if next < 4 {
                c.ram[0] = c.ram[next];
            } else {
                let base = next & !3;
                let copy = [
                    c.ram[base],
                    c.ram[base + 1],
                    c.ram[base + 2],
                    c.ram[base + 3],
                ];
                c.ram[..4].copy_from_slice(&copy);
            }
        }
        c.on = c.dac;
        c.position = 0;
        // The first sample comes a little after a full period, and the buffer isn't refilled
        // (Pan Docs "NR34", playback delay).
        c.timer = 2048 - c.period + 3;
    }

    fn power_off(&mut self) {
        let lengths = [
            self.ch1.length.left,
            self.ch2.length.left,
            self.ch3.length.left,
            self.ch4.length.left,
        ];
        let ram = self.ch3.ram;
        self.ch1 = Pulse::default();
        self.ch2 = Pulse::default();
        self.ch3 = Wave {
            ram,
            ..Wave::default()
        };
        self.ch4 = Noise::default();
        // A DMG keeps its length timers through a power cycle.
        if !self.cgb {
            self.ch1.length.left = lengths[0];
            self.ch2.length.left = lengths[1];
            self.ch3.length.left = lengths[2];
            self.ch4.length.left = lengths[3];
        }
        self.nr50 = 0;
        self.nr51 = 0;
        self.powered = false;
    }

    /// The register values the boot ROM leaves (Pan Docs "Power Up Sequence"); channel 1 is
    /// still on after the boot sound, its envelope having faded it out.
    pub fn set_post_boot(&mut self) {
        let boot: [(u16, u8); 21] = [
            (0xff26, 0x80),
            (0xff10, 0x80),
            (0xff11, 0xbf),
            (0xff12, 0xf3),
            (0xff13, 0xff),
            (0xff14, 0x3f),
            (0xff16, 0x3f),
            (0xff17, 0x00),
            (0xff18, 0xff),
            (0xff19, 0xbf),
            (0xff1a, 0x7f),
            (0xff1b, 0xff),
            (0xff1c, 0x9f),
            (0xff1d, 0xff),
            (0xff1e, 0xbf),
            (0xff20, 0xff),
            (0xff21, 0x00),
            (0xff22, 0x00),
            (0xff23, 0xbf),
            (0xff24, 0x77),
            (0xff25, 0xf3),
        ];
        for (addr, v) in boot {
            self.write(addr, v);
        }
        self.ch1.on = true;
        self.ch1.started = true;
        self.ch1.env.volume = 0;
        self.output_changed();
    }
}

impl Length {
    fn visit<V: crate::state::Visitor>(&mut self, v: &mut V) {
        v.u16(&mut self.left);
        v.bool(&mut self.enabled);
    }
}

impl Envelope {
    fn visit<V: crate::state::Visitor>(&mut self, v: &mut V) {
        v.u8(&mut self.reg);
        v.u8(&mut self.volume);
        v.u8(&mut self.timer);
    }
}

impl Pulse {
    fn visit<V: crate::state::Visitor>(&mut self, v: &mut V) {
        v.bool(&mut self.on);
        v.u8(&mut self.duty);
        v.u8(&mut self.step);
        v.bool(&mut self.started);
        v.u16(&mut self.period);
        v.u16(&mut self.timer);
        self.length.visit(v);
        self.env.visit(v);
        v.u8(&mut self.nr10);
        v.bool(&mut self.sweep_on);
        v.u16(&mut self.shadow);
        v.u8(&mut self.sweep_timer);
        v.bool(&mut self.negated);
    }
}

impl Wave {
    fn visit<V: crate::state::Visitor>(&mut self, v: &mut V) {
        v.bool(&mut self.on);
        v.bool(&mut self.dac);
        v.u8(&mut self.level);
        v.u16(&mut self.period);
        v.u16(&mut self.timer);
        v.u8(&mut self.position);
        v.u8(&mut self.sample);
        v.u32(&mut self.since_read);
        self.length.visit(v);
        v.bytes(&mut self.ram);
    }
}

impl Noise {
    fn visit<V: crate::state::Visitor>(&mut self, v: &mut V) {
        v.bool(&mut self.on);
        v.u8(&mut self.nr43);
        v.u16(&mut self.lfsr);
        v.u32(&mut self.timer);
        self.length.visit(v);
        self.env.visit(v);
    }
}

impl Apu {
    /// The APU's state. The synthesizer's buffer isn't part of it: after a load it starts
    /// again from the restored level at the restored time.
    pub(crate) fn visit<V: crate::state::Visitor>(&mut self, v: &mut V) {
        v.bool(&mut self.powered);
        self.ch1.visit(v);
        self.ch2.visit(v);
        self.ch3.visit(v);
        self.ch4.visit(v);
        v.u8(&mut self.nr50);
        v.u8(&mut self.nr51);
        v.u8(&mut self.frame_step);
        v.u32(&mut self.dot_phase);
        v.u64(&mut self.now);
    }

    /// Restarts the synthesizer at the current time and level, after a state is loaded.
    pub(crate) fn restart_output(&mut self) {
        let rate = self.synth.rate();
        self.synth = Synth::new(48_000, self.cgb);
        self.synth.set_rate(rate);
        self.synth.restart_at(self.now);
        self.output_changed();
    }
}

fn clock_length(length: &mut Length, on: &mut bool) {
    if length.enabled && length.left > 0 {
        length.left -= 1;
        if length.left == 0 {
            *on = false;
        }
    }
}

/// An NRx4 write's effect on the length timer. Enabling the timer while the next DIV-APU step
/// won't clock it clocks it once now; triggering with the timer expired reloads it, one short
/// if the timer is enabled and the next step won't clock it (Pan Docs "Obscure Behavior").
fn length_write(length: &mut Length, on: &mut bool, v: u8, max: u16, length_next: bool) {
    let was = length.enabled;
    length.enabled = v & 0x40 != 0;
    let trigger = v & 0x80 != 0;
    if !length_next && !was && length.enabled && length.left > 0 {
        length.left -= 1;
        if length.left == 0 && !trigger {
            *on = false;
        }
    }
    if trigger && length.left == 0 {
        length.left = max;
        if length.enabled && !length_next {
            length.left -= 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn powered() -> Apu {
        let mut a = Apu::new(false, 48_000);
        a.write(0xff26, 0x80);
        a
    }

    #[test]
    fn a_triggered_channel_reports_on_until_its_length_runs_out() {
        let mut a = powered();
        a.write(0xff12, 0xf0);
        a.write(0xff11, 62); // 2 steps of length
        a.write(0xff14, 0xc0);
        assert_eq!(a.read(0xff26) & 1, 1);
        a.div_apu_step();
        a.div_apu_step();
        a.div_apu_step();
        assert_eq!(a.read(0xff26) & 1, 0);
    }

    #[test]
    fn the_dac_off_keeps_a_channel_off() {
        let mut a = powered();
        a.write(0xff17, 0x00);
        a.write(0xff19, 0x80);
        assert_eq!(a.read(0xff26) & 2, 0);
    }

    #[test]
    fn power_off_clears_registers_and_keeps_wave_ram() {
        let mut a = powered();
        a.write(0xff30, 0x12);
        a.write(0xff12, 0xf3);
        a.write(0xff26, 0x00);
        assert_eq!(a.read(0xff12), 0x00);
        assert_eq!(a.read(0xff30), 0x12);
        a.write(0xff12, 0xf3);
        assert_eq!(a.read(0xff12), 0x00);
    }

    #[test]
    fn noise_lfsr_follows_the_documented_xnor() {
        let mut n = Noise::default();
        n.clock_lfsr();
        // 0 xnor 0 = 1 into bit 15, then shifted right.
        assert_eq!(n.lfsr, 0x4000);
    }
}
