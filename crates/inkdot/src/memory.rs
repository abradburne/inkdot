//! RetroAchievements' Game Boy address space over inkdot's machine ([`Bus`], the
//! [`Memory`] rcheevos reads).
//!
//! The service lays a Game Boy's memory out as the CPU's bus, `$0000`-`$FFFF`, then the banks a
//! game pages in that the bus can't show at once: the Game Boy Color's work-RAM banks 2-7 at
//! `$10000`-`$15FFF` and cartridge RAM banks 1-15 at `$16000`-`$33FFF` (rcheevos'
//! `src/rcheevos/consoleinfo.c`, the `_rc_memory_regions_gameboy_color` table). A plain Game
//! Boy has the same bus and cartridge banks, and nothing at `$10000`-`$15FFF` (the
//! `_rc_memory_regions_gameboy` table marks it unused), so there it reads as unbacked. `$D000`-`$DFFF` is work-RAM bank 1 whatever the
//! game has paged there, and `$A000`-`$BFFF` cartridge RAM bank 0: a set's addresses mean the
//! same thing whatever the game is doing at the moment.
//!
//! Reads come straight from the arrays (work RAM, video RAM, OAM, cartridge RAM) or, for the
//! ROM and the I/O registers, as the CPU would see them without taking time (`peek`). Video
//! RAM is the first bank. An address outside every region reads as zero and reports itself
//! unbacked, so rcheevos marks an achievement that needs it unsupported rather than wrong.

use inkdot_cheevos::Memory;
use inkdot_core::{GameBoy, Model};

/// One address past the last the service defines for a Game Boy Color.
#[cfg(test)]
const END: u32 = 0x34000;

/// The machine, as rcheevos reads it.
pub struct Bus<'a>(pub &'a GameBoy);

impl Memory for Bus<'_> {
    /// Reads `buf.len()` bytes at `address`; how many of them were backed. Unbacked bytes are
    /// zero.
    fn read(&self, address: u32, buf: &mut [u8]) -> u32 {
        let mut backed = 0;
        for (i, b) in buf.iter_mut().enumerate() {
            let at = address.wrapping_add(i as u32);
            match byte(self.0, at) {
                Some(v) => {
                    *b = v;
                    backed += 1;
                }
                None => *b = 0,
            }
        }
        backed
    }
}

/// One byte of the service's address space, or `None` where nothing backs it.
fn byte(gb: &GameBoy, at: u32) -> Option<u8> {
    let hw = &gb.hw;
    if gb.model() == Model::Dmg && (0x10000..=0x15fff).contains(&at) {
        return None;
    }
    Some(match at {
        0x0000..=0x7fff => hw.cart.read_rom(at as u16),
        0x8000..=0x9fff => hw.ppu.vram[(at - 0x8000) as usize],
        0xa000..=0xbfff => *hw.cart.ram().get((at - 0xa000) as usize)?,
        0xc000..=0xdfff => hw.wram()[(at - 0xc000) as usize],
        0xe000..=0xfdff => hw.wram()[(at - 0xe000) as usize],
        0xfe00..=0xfe9f => hw.ppu.oam[(at - 0xfe00) as usize],
        0xfea0..=0xfeff => return None,
        0xff00..=0xffff => hw.peek(at as u16),
        0x10000..=0x15fff => hw.wram()[(at - 0x10000 + 0x2000) as usize],
        0x16000..=0x33fff => *hw.cart.ram().get((at - 0x16000 + 0x2000) as usize)?,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(gb: &GameBoy, address: u32, buf: &mut [u8]) -> u32 {
        Bus(gb).read(address, buf)
    }

    /// A 64 KiB MBC5 ROM with 32 KiB of RAM, each ROM byte its bank number.
    fn machine() -> GameBoy {
        machine_on(Model::Cgb)
    }

    fn machine_on(model: Model) -> GameBoy {
        let mut rom = vec![0u8; 0x10000];
        for (i, b) in rom.iter_mut().enumerate() {
            *b = (i / 0x4000) as u8;
        }
        rom[0x143] = 0x80;
        rom[0x147] = 0x1b; // MBC5 + RAM + battery
        rom[0x149] = 3; // 32 KiB
        GameBoy::new(rom, model).unwrap()
    }

    /// A Game Boy has no work-RAM banks beyond the bus; its cartridge RAM banks are there.
    #[test]
    fn a_game_boy_has_no_wram_banks() {
        let gb = machine_on(Model::Dmg);
        let mut b = [0xffu8; 2];
        assert_eq!(read(&gb, 0x10000, &mut b), 0);
        assert_eq!(b, [0, 0]);
        assert_eq!(
            read(&gb, 0x15fff, &mut b),
            1,
            "the first cartridge bank is backed"
        );
        assert_eq!(read(&gb, 0x16000, &mut b), 2);
        assert_eq!(read(&gb, 0xd000, &mut b), 2);
    }

    #[test]
    fn the_bus_and_the_banks() {
        let mut gb = machine();
        gb.hw.ie = 0x1f;
        let mut b = [0u8; 2];
        assert_eq!(read(&gb, 0x0100, &mut b), 2);
        assert_eq!(b, [0, 0]);
        // Bank 1 is at $4000 after power-on.
        assert_eq!(read(&gb, 0x4000, &mut b), 2);
        assert_eq!(b, [1, 1]);
        let mut one = [0u8; 1];
        assert_eq!(read(&gb, 0xffff, &mut one), 1);
        assert_eq!(one, [0x1f]);
        // Cartridge RAM, every bank, is backed; past its 32 KiB is not.
        assert_eq!(read(&gb, 0xa000, &mut one), 1);
        assert_eq!(read(&gb, 0x16000, &mut b), 2);
        assert_eq!(read(&gb, 0x16000 + 0x6000 - 1, &mut b), 1);
        // Work RAM's last bank runs straight into cartridge RAM bank 1; the end of the space.
        assert_eq!(read(&gb, 0x15fff, &mut b), 2);
        assert_eq!(read(&gb, END, &mut one), 0);
        assert_eq!(one, [0]);
        // The hole between OAM and I/O.
        assert_eq!(read(&gb, 0xfea0, &mut one), 0);
    }

    #[test]
    fn echo_ram_is_work_ram() {
        let gb = machine();
        let mut a = [0u8; 16];
        let mut b = [0u8; 16];
        read(&gb, 0xc000, &mut a);
        read(&gb, 0xe000, &mut b);
        assert_eq!(a, b);
    }
}
