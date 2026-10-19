//! The whole machine: a CPU on the hardware, started as the boot ROM leaves it.

use crate::cart::{CartError, Cartridge};
use crate::cpu::{Cpu, Mode};
use crate::hw::Hardware;
use crate::ppu::FRAME_DOTS;
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

    /// Sets the buttons held (the [`crate::hw::button`] bits).
    pub fn set_buttons(&mut self, buttons: u8) {
        self.hw.set_buttons(buttons);
    }
}
