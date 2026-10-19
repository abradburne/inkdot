//! Everything on the CPU's bus: the memory map, the I/O registers, interrupts, OAM DMA, the
//! CGB's VRAM DMA and speed switch, and the clock that steps the other parts.
//!
//! Every bus access is one M-cycle: the rest of the machine is advanced by that cycle first,
//! then the access is made.
//!
//! Sources: Pan Docs "Memory Map", "Interrupts", "OAM DMA Transfer", "CGB Registers",
//! "Joypad Input".

use crate::apu::Apu;
use crate::cart::Cartridge;
use crate::cpu::Bus;
use crate::ppu::{event, Ppu};
use crate::serial::Serial;
use crate::timer::Timer;
use alloc::boxed::Box;
use alloc::vec::Vec;

/// The buttons, as bits of [`Hardware::set_buttons`].
pub mod button {
    #![allow(missing_docs)]
    pub const A: u8 = 1;
    pub const B: u8 = 2;
    pub const SELECT: u8 = 4;
    pub const START: u8 = 8;
    pub const RIGHT: u8 = 0x10;
    pub const LEFT: u8 = 0x20;
    pub const UP: u8 = 0x40;
    pub const DOWN: u8 = 0x80;
}

/// The CGB's VRAM DMA.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Hdma {
    src: u16,
    dst: u16,
    /// Blocks of 16 bytes left, minus one, as HDMA5 reads.
    remaining: u8,
    /// An HBlank transfer is running.
    hblank: bool,
    /// An HBlank started and its block hasn't been copied yet.
    block_due: bool,
}

/// The machine around the CPU.
pub struct Hardware {
    /// The cartridge.
    pub cart: Cartridge,
    /// The picture.
    pub ppu: Ppu,
    /// The sound.
    pub apu: Apu,
    /// DIV and TIMA.
    pub timer: Timer,
    /// The link port.
    pub serial: Serial,
    wram: Box<[u8; 0x8000]>,
    hram: [u8; 0x7f],
    /// IF.
    pub if_: u8,
    /// IE.
    pub ie: u8,
    p1: u8,
    buttons: u8,
    /// A CGB, in either mode.
    pub cgb: bool,
    /// A CGB running a CGB game.
    pub cgb_mode: bool,
    svbk: u8,
    key1: u8,
    /// CGB double speed.
    pub double_speed: bool,
    dma_src: u16,
    dma_index: u8,
    dma_active: bool,
    /// A DMA written to $FF46 starts after a cycle's delay: (source, cycles left).
    dma_start: Option<(u16, u8)>,
    dma_reg: u8,
    hdma: Hdma,
    /// Set by the machine before each CPU step: HBlank DMA pauses while the CPU is halted.
    pub cpu_halted: bool,
    /// An optional boot ROM, mapped at $0000 (and $0200-$08FF on a CGB) until $FF50 is written.
    boot_rom: Option<Vec<u8>>,
    /// VBlank started during the last step: a frame is complete.
    pub frame_done: bool,
    /// Machine time in dots (2^22 Hz) since power-on.
    pub dots: u64,
}

impl Hardware {
    /// The hardware for `cart`, on a CGB or not, in CGB mode or not.
    pub fn new(cart: Cartridge, cgb: bool, cgb_mode: bool) -> Hardware {
        Hardware {
            cart,
            ppu: Ppu::new(cgb, cgb_mode),
            apu: Apu::default(),
            timer: Timer::default(),
            serial: Serial::default(),
            wram: Box::new([0; 0x8000]),
            hram: [0; 0x7f],
            if_: 0,
            ie: 0,
            p1: 0x30,
            buttons: 0,
            cgb,
            cgb_mode,
            svbk: 0,
            key1: 0,
            double_speed: false,
            dma_src: 0,
            dma_index: 0,
            dma_active: false,
            dma_start: None,
            dma_reg: 0xff,
            hdma: Hdma {
                remaining: 0xff,
                ..Hdma::default()
            },
            cpu_halted: false,
            boot_rom: None,
            frame_done: false,
            dots: 0,
        }
    }

    /// Maps a boot ROM until the game unmaps it by writing $FF50.
    pub fn set_boot_rom(&mut self, rom: Vec<u8>) {
        self.boot_rom = Some(rom);
    }

    /// Sets which buttons are held (the [`button`] bits). Pressing one requests the joypad
    /// interrupt if the game is watching its line.
    pub fn set_buttons(&mut self, buttons: u8) {
        let before = self.p1_low();
        self.buttons = buttons;
        if before & !self.p1_low() != 0 {
            self.if_ |= 0x10;
        }
    }

    fn p1_low(&self) -> u8 {
        let mut low = 0x0f;
        if self.p1 & 0x10 == 0 {
            low &= !(self.buttons >> 4);
        }
        if self.p1 & 0x20 == 0 {
            low &= !(self.buttons & 0x0f);
        }
        low
    }

    /// One M-cycle of everything but the CPU.
    fn tick(&mut self) {
        self.tick_devices();
        self.tick_timer();
    }

    /// The system counter and what it clocks (the timer, serial). In each M-cycle it counts on
    /// after the CPU's access: a write to TAC or DIV acts on the counter as it was before the
    /// cycle's increment (Mooneye `rapid_toggle`), and TIMA's reload lands at the end of the
    /// cycle after an overflow.
    fn tick_timer(&mut self) {
        let before = self.timer.counter;
        if self.timer.tick() {
            self.if_ |= 4;
        }
        if self.serial.tick(before, self.timer.counter, self.cgb_mode) {
            self.if_ |= 8;
        }
    }

    /// One M-cycle of the parts that change before the CPU's access in it.
    fn tick_devices(&mut self) {
        self.tick_dma();
        let dots = if self.double_speed { 2 } else { 4 };
        let ev = self.ppu.tick(dots);
        if ev & event::VBLANK != 0 {
            self.if_ |= 1;
            self.frame_done = true;
        }
        if ev & event::STAT != 0 {
            self.if_ |= 2;
        }
        if ev & event::HBLANK != 0 && self.hdma.hblank {
            self.hdma.block_due = true;
        }
        self.cart.tick(dots);
        self.dots += dots as u64;
    }

    fn tick_dma(&mut self) {
        if self.dma_active {
            let v = self.dma_read(self.dma_src + self.dma_index as u16);
            self.ppu.oam[self.dma_index as usize] = v;
            self.dma_index += 1;
            if self.dma_index == 160 {
                self.dma_active = false;
            }
        }
        if let Some((src, left)) = self.dma_start {
            if left == 0 {
                self.dma_start = None;
                self.dma_src = src;
                self.dma_index = 0;
                self.dma_active = true;
            } else {
                self.dma_start = Some((src, left - 1));
            }
        }
        self.ppu.dma_active = self.dma_active;
    }

    /// What a DMA reads: the memory map without the CPU's access rules.
    fn dma_read(&self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x7fff => self.cart.read_rom(addr),
            0x8000..=0x9fff => self.ppu.vram[addr as usize & 0x1fff],
            0xa000..=0xbfff => self.cart.read_ram(addr),
            0xc000..=0xdfff => self.read_wram(addr),
            _ => self.read_wram(addr - 0x2000),
        }
    }

    fn wram_index(&self, addr: u16) -> usize {
        if addr < 0xd000 {
            addr as usize - 0xc000
        } else {
            self.svbk.max(1) as usize * 0x1000 + (addr as usize - 0xd000)
        }
    }

    fn read_wram(&self, addr: u16) -> u8 {
        self.wram[self.wram_index(addr)]
    }

    /// The HBlank DMA's block, when one is due: 16 bytes, with the CPU stalled meanwhile.
    fn run_hdma_block(&mut self) {
        self.hdma.block_due = false;
        self.copy_hdma_block();
        if self.hdma.remaining == 0 {
            self.hdma.remaining = 0xff;
            self.hdma.hblank = false;
        } else {
            self.hdma.remaining -= 1;
        }
    }

    /// Copies 16 bytes for the VRAM DMA, taking 8 M-cycles at normal speed (16 at double).
    fn copy_hdma_block(&mut self) {
        for _ in 0..16 {
            let v = self.dma_read(self.hdma.src);
            self.ppu
                .dma_write_vram(0x8000 | (self.hdma.dst & 0x1fff), v);
            self.hdma.src = self.hdma.src.wrapping_add(1);
            self.hdma.dst = self.hdma.dst.wrapping_add(1) & 0x1fff;
        }
        let cycles = if self.double_speed { 16 } else { 8 };
        for _ in 0..cycles {
            self.tick();
        }
    }

    /// The start of an M-cycle on the bus: any HBlank DMA block due is copied first (stalling
    /// the CPU), then the machine advances up to the access; [`Hardware::tick_timer`] finishes
    /// the cycle after it.
    fn cycle_start(&mut self) {
        if self.hdma.block_due && !self.cpu_halted {
            self.run_hdma_block();
        }
        self.tick_devices();
    }

    fn read_io(&self, addr: u16) -> u8 {
        match addr {
            0xff00 => 0xc0 | (self.p1 & 0x30) | self.p1_low(),
            0xff01 | 0xff02 => self.serial.read(addr, self.cgb_mode),
            0xff04..=0xff07 => self.timer.read(addr),
            0xff0f => 0xe0 | self.if_,
            0xff10..=0xff3f => self.apu.read(addr),
            0xff46 => self.dma_reg,
            0xff4d if self.cgb_mode => 0x7e | (self.double_speed as u8) << 7 | (self.key1 & 1),
            0xff55 if self.cgb_mode => {
                let active = self.hdma.hblank as u8;
                (active ^ 1) << 7 | (self.hdma.remaining & 0x7f)
            }
            0xff70 if self.cgb_mode => 0xf8 | self.svbk,
            0xff40..=0xff4b | 0xff4f | 0xff68..=0xff6c => self.ppu.read(addr),
            _ => 0xff,
        }
    }

    fn write_io(&mut self, addr: u16, v: u8) {
        match addr {
            0xff00 => {
                let before = self.p1_low();
                self.p1 = v & 0x30;
                if before & !self.p1_low() != 0 {
                    self.if_ |= 0x10;
                }
            }
            0xff01 | 0xff02 => self.serial.write(addr, v),
            0xff04..=0xff07 => self.timer.write(addr, v),
            0xff0f => self.if_ = v & 0x1f,
            0xff10..=0xff3f => self.apu.write(addr, v),
            0xff46 => {
                self.dma_reg = v;
                self.dma_start = Some(((v as u16) << 8, 1));
            }
            0xff4d if self.cgb_mode => self.key1 = v & 1,
            0xff50 => self.boot_rom = None,
            0xff51 if self.cgb_mode => self.hdma.src = (self.hdma.src & 0xff) | (v as u16) << 8,
            0xff52 if self.cgb_mode => self.hdma.src = (self.hdma.src & 0xff00) | (v & 0xf0) as u16,
            0xff53 if self.cgb_mode => {
                self.hdma.dst = (self.hdma.dst & 0xff) | ((v & 0x1f) as u16) << 8
            }
            0xff54 if self.cgb_mode => self.hdma.dst = (self.hdma.dst & 0x1f00) | (v & 0xf0) as u16,
            0xff55 if self.cgb_mode => self.write_hdma5(v),
            0xff70 if self.cgb_mode => self.svbk = v & 7,
            0xff40..=0xff4b | 0xff4f | 0xff68..=0xff6c => {
                let ev = self.ppu.write(addr, v);
                if ev & event::STAT != 0 {
                    self.if_ |= 2;
                }
            }
            _ => {}
        }
    }

    fn write_hdma5(&mut self, v: u8) {
        if self.hdma.hblank && v & 0x80 == 0 {
            // Stops an HBlank transfer; HDMA5 then reads the blocks left with bit 7 set.
            self.hdma.hblank = false;
            self.hdma.block_due = false;
            return;
        }
        self.hdma.remaining = v & 0x7f;
        if v & 0x80 != 0 {
            self.hdma.hblank = true;
            // Started during HBlank, the first block is copied at once.
            if self.ppu.read(0xff41) & 3 == 0 && self.ppu.read(0xff40) & 0x80 != 0 {
                self.hdma.block_due = true;
            }
            return;
        }
        // A general-purpose transfer: all of it now, the CPU waiting.
        let blocks = self.hdma.remaining as usize + 1;
        for _ in 0..blocks {
            self.copy_hdma_block();
        }
        self.hdma.remaining = 0xff;
    }

    fn read_mem(&self, addr: u16) -> u8 {
        if let Some(boot) = &self.boot_rom {
            let a = addr as usize;
            if a < 0x100 || (self.cgb && (0x200..0x900).contains(&a) && a < boot.len()) {
                return boot[a];
            }
        }
        match addr {
            0x0000..=0x7fff => self.cart.read_rom(addr),
            0x8000..=0x9fff => self.ppu.read_vram(addr),
            0xa000..=0xbfff => self.cart.read_ram(addr),
            0xc000..=0xdfff => self.read_wram(addr),
            0xe000..=0xfdff => self.read_wram(addr - 0x2000),
            0xfe00..=0xfe9f => {
                if self.dma_active {
                    0xff
                } else {
                    self.ppu.read_oam(addr)
                }
            }
            0xfea0..=0xfeff => {
                if self.cgb {
                    0xff
                } else {
                    0
                }
            }
            0xff80..=0xfffe => self.hram[addr as usize - 0xff80],
            0xffff => self.ie,
            _ => self.read_io(addr),
        }
    }

    fn write_mem(&mut self, addr: u16, v: u8) {
        match addr {
            0x0000..=0x7fff => self.cart.write_rom(addr, v),
            0x8000..=0x9fff => self.ppu.write_vram(addr, v),
            0xa000..=0xbfff => self.cart.write_ram(addr, v),
            0xc000..=0xdfff => self.wram[self.wram_index(addr)] = v,
            0xe000..=0xfdff => self.wram[self.wram_index(addr - 0x2000)] = v,
            0xfe00..=0xfe9f => {
                if !self.dma_active {
                    self.ppu.write_oam(addr, v);
                }
            }
            0xfea0..=0xfeff => {}
            0xff80..=0xfffe => self.hram[addr as usize - 0xff80] = v,
            0xffff => self.ie = v,
            _ => self.write_io(addr, v),
        }
    }

    /// The I/O state at $0100, as the boot ROM leaves it (Pan Docs "Power Up Sequence",
    /// hardware registers).
    pub fn post_boot(&mut self) {
        self.p1 = 0;
        self.if_ = 0x01;
        self.apu.set_post_boot();
        // DIV is $AB on a DMG; the low byte places the counter where the boot ROM's last
        // instruction leaves it (Mooneye `boot_div`).
        self.timer.counter = if self.cgb { 0x1ea0 } else { 0xabcc };
        if self.cgb {
            self.serial.set_post_boot(0x03);
            self.dma_reg = 0;
        }
        self.ppu.set_post_boot(0x91, 0xfc, 400);
    }

    /// Reads memory as the CPU would see it, without taking time (for debuggers and tests).
    pub fn peek(&self, addr: u16) -> u8 {
        self.read_mem(addr)
    }

    /// The whole of work RAM, for save states and tests.
    pub fn wram(&self) -> &[u8] {
        &self.wram[..]
    }
}

impl Bus for Hardware {
    fn read(&mut self, addr: u16) -> u8 {
        self.cycle_start();
        let v = self.read_mem(addr);
        self.tick_timer();
        v
    }

    fn write(&mut self, addr: u16, value: u8) {
        self.cycle_start();
        self.write_mem(addr, value);
        self.tick_timer();
    }

    fn idle(&mut self, _addr: u16) {
        self.cycle_start();
        self.tick_timer();
    }

    fn interrupts(&self) -> u8 {
        self.if_ & self.ie & 0x1f
    }

    fn acknowledge(&mut self, bit: u8) {
        self.if_ &= !(1 << bit);
    }

    fn stop(&mut self) -> bool {
        if self.cgb_mode && self.key1 & 1 != 0 {
            // The speed switch: the CPU pauses for 2050 M-cycles and DIV is reset (Pan Docs
            // "FF4D — KEY1").
            self.key1 = 0;
            self.double_speed = !self.double_speed;
            self.timer.reset_counter();
            for _ in 0..2050 {
                self.tick();
            }
            return false;
        }
        self.timer.reset_counter();
        true
    }

    fn stop_wake(&mut self) -> bool {
        self.buttons != 0
    }
}
