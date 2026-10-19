//! The serial port, with nothing plugged in.
//!
//! A transfer with the internal clock shifts SB out one bit at a time and shifts in 1s (an
//! unplugged cable reads high), then requests the serial interrupt. With the external clock
//! nothing ever clocks it, so the transfer never ends, as on hardware. The bytes sent are kept,
//! which is how Blargg's test ROMs report their results.
//!
//! The internal clock is a bit of the system counter (bit 8 at 8192 Hz, bit 3 on a CGB's fast
//! setting), so transfers line up with DIV as on hardware (Mooneye `boot_sclk_align`).
//!
//! Source: Pan Docs "Serial Data Transfer (Link Cable)".

use alloc::vec::Vec;

/// The serial port's state.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Serial {
    sb: u8,
    sc: u8,
    bits_left: u8,
    /// Every byte the game has sent.
    pub sent: Vec<u8>,
}

impl Serial {
    /// One M-cycle, given the system counter before and after it. Returns whether the serial
    /// interrupt is requested.
    pub fn tick(&mut self, before: u16, after: u16, cgb_mode: bool) -> bool {
        if self.bits_left == 0 || self.sc & 1 == 0 {
            return false;
        }
        let bit = if cgb_mode && self.sc & 2 != 0 {
            1 << 3
        } else {
            1 << 8
        };
        if before & bit == 0 || after & bit != 0 {
            return false;
        }
        self.sb = self.sb << 1 | 1;
        self.bits_left -= 1;
        if self.bits_left == 0 {
            self.sc &= 0x7f;
            return true;
        }
        false
    }

    /// SC as the CGB boot ROM leaves it.
    pub fn set_post_boot(&mut self, sc: u8) {
        self.sc = sc;
    }

    /// Reads SB or SC.
    pub fn read(&self, addr: u16, cgb_mode: bool) -> u8 {
        if addr == 0xff01 {
            self.sb
        } else if cgb_mode {
            self.sc | 0x7c
        } else {
            self.sc | 0x7e
        }
    }

    /// Writes SB or SC.
    pub fn write(&mut self, addr: u16, v: u8) {
        if addr == 0xff01 {
            self.sb = v;
            return;
        }
        self.sc = v & 0x83;
        if v & 0x80 != 0 {
            self.bits_left = 8;
            self.sent.push(self.sb);
        } else {
            self.bits_left = 0;
        }
    }
}

impl Serial {
    pub(crate) fn visit<V: crate::state::Visitor>(&mut self, v: &mut V) {
        v.u8(&mut self.sb);
        v.u8(&mut self.sc);
        v.u8(&mut self.bits_left);
    }
}
