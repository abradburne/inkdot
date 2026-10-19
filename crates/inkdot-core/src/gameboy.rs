//! The whole machine: a CPU on the hardware, started as the boot ROM leaves it.

use crate::cart::{CartError, Cartridge};
use crate::cpu::{Cpu, Mode};
use crate::hw::Hardware;
use crate::ppu::FRAME_DOTS;
use crate::state::{self, Identity, Reader, StateError, Visitor, Writer};
use alloc::vec::Vec;

/// Which console to be.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Model {
    /// The original Game Boy (DMG-CPU A/B/C, as in most units sold).
    Dmg,
    /// The Game Boy Color (CPU CGB-C/E). DMG games run in its compatibility mode.
    Cgb,
}

/// A Game Boy with a cartridge in it.
pub struct GameBoy {
    /// The CPU.
    pub cpu: Cpu,
    /// Everything else.
    pub hw: Hardware,
    model: Model,
}

impl GameBoy {
    /// Puts `rom` in a console of `model` and switches it on. The machine starts at $0100 in
    /// the state the console's boot ROM leaves it in (Pan Docs "Power Up Sequence"); no boot ROM
    /// runs. A game that needs a CGB is run on one whatever `model` says.
    pub fn new(rom: Vec<u8>, model: Model) -> Result<GameBoy, CartError> {
        let cart = Cartridge::new(rom)?;
        let header = cart.header().clone();
        let model = if header.requires_cgb() {
            Model::Cgb
        } else {
            model
        };
        let cgb = model == Model::Cgb;
        let cgb_mode = cgb && header.supports_cgb();
        let mut hw = Hardware::new(cart, cgb, cgb_mode);
        let mut cpu = Cpu {
            sp: 0xfffe,
            pc: 0x0100,
            ..Cpu::default()
        };
        if !cgb {
            cpu.a = 0x01;
            cpu.f = if header.header_checksum == 0 {
                0x80
            } else {
                0xb0
            };
            (cpu.b, cpu.c, cpu.d, cpu.e, cpu.h, cpu.l) = (0x00, 0x13, 0x00, 0xd8, 0x01, 0x4d);
        } else if cgb_mode {
            cpu.a = 0x11;
            cpu.f = 0x80;
            (cpu.b, cpu.c, cpu.d, cpu.e, cpu.h, cpu.l) = (0x00, 0x00, 0xff, 0x56, 0x00, 0x0d);
        } else {
            // The CGB boot ROM leaves the title checksum it chose a palette by in B.
            let sum = if header.licensed_by_nintendo() {
                header
                    .title_bytes
                    .iter()
                    .fold(0u8, |s, b| s.wrapping_add(*b))
            } else {
                0
            };
            cpu.a = 0x11;
            cpu.f = 0x80;
            (cpu.b, cpu.c, cpu.d, cpu.e) = (sum, 0x00, 0x00, 0x08);
            (cpu.h, cpu.l) = if sum == 0x43 || sum == 0x58 {
                (0x99, 0x1a)
            } else {
                (0x00, 0x7c)
            };
        }
        hw.post_boot();
        if cgb && !cgb_mode {
            let [bg, obj0, obj1] = crate::compat::for_header(&header);
            hw.ppu.set_palette(false, 0, bg);
            hw.ppu.set_palette(true, 0, obj0);
            hw.ppu.set_palette(true, 1, obj1);
            // The boot ROM wrote 8 bytes of background palette and 16 of object palette with
            // auto-increment.
            hw.ppu.set_palette_indexes(0x88, 0x90);
        }
        Ok(GameBoy { cpu, hw, model })
    }

    /// The console being emulated.
    pub fn model(&self) -> Model {
        self.model
    }

    /// Runs one CPU instruction (or interrupt dispatch, or a cycle of HALT).
    pub fn step(&mut self) {
        self.hw.cpu_halted = self.cpu.mode == Mode::Halted;
        self.cpu.step(&mut self.hw);
    }

    /// Runs until the next VBlank starts (a frame is complete) or, with the LCD off, for one
    /// frame's worth of time.
    pub fn run_frame(&mut self) {
        let start = self.hw.dots;
        self.hw.frame_done = false;
        while !self.hw.frame_done && self.hw.dots - start < FRAME_DOTS as u64 * 2 {
            self.step();
            if self.hw.ppu.read(0xff40) & 0x80 == 0 && self.hw.dots - start >= FRAME_DOTS as u64 {
                break;
            }
        }
    }

    /// Runs for at least `dots` of machine time.
    pub fn run_dots(&mut self, dots: u64) {
        let end = self.hw.dots + dots;
        while self.hw.dots < end {
            self.step();
        }
    }

    /// The last complete picture (see [`crate::ppu::Ppu::frame`]).
    pub fn frame(&self) -> &[u16] {
        self.hw.ppu.frame()
    }

    /// Appends the sound made since the last call, as interleaved stereo samples at the
    /// synthesizer's rate (48 kHz unless changed with [`crate::synth::Synth::set_rate`]).
    pub fn take_audio(&mut self, out: &mut Vec<i16>) {
        self.hw.apu.take_samples(out);
    }

    fn identity(&self) -> Identity {
        let header = self.hw.cart.header();
        Identity {
            cgb: self.hw.cgb,
            cgb_mode: self.hw.cgb_mode,
            checksum: header.global_checksum,
            title: header.title_bytes,
        }
    }

    fn visit_section<V: Visitor>(&mut self, tag: &[u8; 4], v: &mut V) {
        match tag {
            b"CPU " => self.cpu.visit(v),
            b"BUS " => self.hw.visit(v),
            b"TIMR" => self.hw.timer.visit(v),
            b"SERL" => self.hw.serial.visit(v),
            b"PPU " => self.hw.ppu.visit(v),
            b"APU " => self.hw.apu.visit(v),
            _ => self.hw.cart.visit(v),
        }
    }

    const SECTIONS: [&'static [u8; 4]; 7] = [
        b"CPU ", b"BUS ", b"TIMR", b"SERL", b"PPU ", b"APU ", b"CART",
    ];

    /// The whole machine, as a save state ([`crate::state`] describes the format).
    pub fn save_state(&mut self) -> Vec<u8> {
        let mut sections = Vec::new();
        for tag in Self::SECTIONS {
            let mut w = Writer(Vec::new());
            self.visit_section(tag, &mut w);
            sections.push((tag, w.0));
        }
        state::encode(self.identity(), &sections)
    }

    /// The model a state was made on, without loading it (`None`: not a state).
    pub fn state_model(data: &[u8]) -> Option<Model> {
        state::made_on_cgb(data).map(|cgb| if cgb { Model::Cgb } else { Model::Dmg })
    }

    /// Restores a save state made by [`GameBoy::save_state`] for this game and console. On an
    /// error the machine is left as it was.
    pub fn load_state(&mut self, data: &[u8]) -> Result<(), StateError> {
        let sections = state::decode(data, self.identity())?;
        for tag in Self::SECTIONS {
            if !sections.iter().any(|(t, _)| t == tag) {
                return Err(StateError::MissingSection(*tag));
            }
        }
        let before = self.save_state();
        for (tag, payload) in &sections {
            let Some(tag) = Self::SECTIONS.iter().find(|t| **t == tag) else {
                continue;
            };
            let mut r = Reader::new(payload);
            self.visit_section(tag, &mut r);
            if r.short {
                self.restore(&before);
                return Err(StateError::Corrupt);
            }
        }
        self.hw.apu.restart_output();
        Ok(())
    }

    /// Puts back a state this machine just made (so it is known to be good).
    fn restore(&mut self, data: &[u8]) {
        if let Ok(sections) = state::decode(data, self.identity()) {
            for (tag, payload) in sections {
                if let Some(tag) = Self::SECTIONS.iter().find(|t| **t == &tag) {
                    self.visit_section(tag, &mut Reader::new(payload));
                }
            }
        }
        self.hw.apu.restart_output();
    }

    /// Sets the buttons held (the [`crate::hw::button`] bits).
    pub fn set_buttons(&mut self, buttons: u8) {
        self.hw.set_buttons(buttons);
    }
}
