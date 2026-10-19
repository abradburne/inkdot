//! Cartridges: the header, the memory bank controllers, battery RAM and the MBC3 clock.
//!
//! Sources: Pan Docs "The Cartridge Header", "MBC1", "MBC2", "MBC3", "MBC5"; the Mooneye
//! emulator-only MBC tests and rtc3test for the details the documents leave open.

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

/// Why a ROM can't be run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CartError {
    /// Smaller than a header, or not a whole number of 16 KiB banks.
    BadSize(usize),
    /// A cartridge type this emulator doesn't have (the header's $0147 byte).
    Unsupported(u8),
}

impl core::fmt::Display for CartError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            CartError::BadSize(n) => write!(f, "not a Game Boy ROM ({n} bytes)"),
            CartError::Unsupported(t) => write!(f, "cartridge type ${t:02X} is not supported"),
        }
    }
}

/// What the header at $0100-$014F says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Header {
    /// The title, up to 16 characters (11 on later cartridges, which use the rest for a code).
    pub title: String,
    /// The raw 16 title bytes, which the CGB uses to pick a compatibility palette.
    pub title_bytes: [u8; 16],
    /// $0143: $80 plays on both, $C0 needs a CGB.
    pub cgb_flag: u8,
    /// $0147.
    pub cart_type: u8,
    /// $0149's RAM size in bytes.
    pub ram_size: usize,
    /// $014B.
    pub old_licensee: u8,
    /// $0144-$0145.
    pub new_licensee: [u8; 2],
    /// $014D: the header checksum.
    pub header_checksum: u8,
    /// $014E-$014F, big-endian: a checksum of the whole ROM.
    pub global_checksum: u16,
}

impl Header {
    /// Reads the header of `rom`, which must be at least $150 bytes.
    pub fn parse(rom: &[u8]) -> Header {
        let mut title_bytes = [0; 16];
        title_bytes.copy_from_slice(&rom[0x134..0x144]);
        let title = rom[0x134..0x144]
            .iter()
            .take_while(|&&b| b != 0)
            .filter(|b| b.is_ascii_graphic() || **b == b' ')
            .map(|&b| b as char)
            .collect::<String>();
        Header {
            title: String::from(title.trim_end()),
            title_bytes,
            cgb_flag: rom[0x143],
            cart_type: rom[0x147],
            ram_size: match rom[0x149] {
                2 => 0x2000,
                3 => 0x8000,
                4 => 0x20000,
                5 => 0x10000,
                _ => 0,
            },
            old_licensee: rom[0x14b],
            new_licensee: [rom[0x144], rom[0x145]],
            header_checksum: rom[0x14d],
            global_checksum: u16::from_be_bytes([rom[0x14e], rom[0x14f]]),
        }
    }

    /// Whether the game uses the CGB's colour features (and so runs in CGB mode on a CGB).
    pub fn supports_cgb(&self) -> bool {
        self.cgb_flag & 0x80 != 0
    }

    /// Whether the game needs a CGB.
    pub fn requires_cgb(&self) -> bool {
        self.cgb_flag == 0xc0
    }

    /// Whether the publisher in the header is Nintendo, which the CGB checks before picking a
    /// compatibility palette by title (Pan Docs "Compatibility palettes").
    pub fn licensed_by_nintendo(&self) -> bool {
        if self.old_licensee == 0x33 {
            self.new_licensee == *b"01"
        } else {
            self.old_licensee == 0x01
        }
    }
}

/// The MBC3's real-time clock. It counts in 32768 Hz ticks, like the crystal on the cartridge.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Rtc {
    /// Seconds, minutes, hours, day low, day high (with halt and carry), as the registers hold
    /// them. Seconds and minutes are 6 bits wide and hours 5, so values a program writes out of
    /// range count up to the register's limit and wrap to 0 without carrying (rtc3test).
    pub regs: [u8; 5],
    /// The registers as last latched, which is what the game reads.
    pub latched: [u8; 5],
    /// 32768 Hz ticks into the current second.
    pub subsecond: u32,
    latch_armed: bool,
    /// Machine dots not yet turned into ticks (128 dots a tick).
    dots: u32,
}

const RTC_HALT: u8 = 0x40;
const RTC_CARRY: u8 = 0x80;

impl Rtc {
    fn halted(&self) -> bool {
        self.regs[4] & RTC_HALT != 0
    }

    /// Advances the clock by `dots` of machine time (the 4 MiHz dot clock, whatever the CPU
    /// speed).
    fn tick_dots(&mut self, dots: u32) {
        self.dots += dots;
        let ticks = self.dots / 128;
        self.dots %= 128;
        if ticks > 0 && !self.halted() {
            self.subsecond += ticks;
            while self.subsecond >= 32768 {
                self.subsecond -= 32768;
                self.add_second();
            }
        }
    }

    fn add_second(&mut self) {
        let r = &mut self.regs;
        r[0] = (r[0] + 1) & 0x3f;
        if r[0] != 60 {
            return;
        }
        r[0] = 0;
        r[1] = (r[1] + 1) & 0x3f;
        if r[1] != 60 {
            return;
        }
        r[1] = 0;
        r[2] = (r[2] + 1) & 0x1f;
        if r[2] != 24 {
            return;
        }
        r[2] = 0;
        let day = (u16::from(r[4] & 1) << 8 | u16::from(r[3])) + 1;
        r[3] = day as u8;
        r[4] = (r[4] & !1) | ((day >> 8) as u8 & 1);
        if day > 0x1ff {
            r[3] = 0;
            r[4] = (r[4] & !1) | RTC_CARRY;
        }
    }

    /// Moves the clock on by whole seconds: the time the game was off, applied at load.
    pub fn advance_seconds(&mut self, seconds: u64) {
        if self.halted() {
            return;
        }
        // Beyond two carries of the day counter the result no longer depends on the exact count.
        let seconds = seconds.min(2 * 512 * 86400 + 86400);
        for _ in 0..seconds {
            self.add_second();
        }
    }

    fn write(&mut self, reg: u8, v: u8) {
        match reg {
            0x08 => {
                self.regs[0] = v & 0x3f;
                // Writing the seconds restarts the second (rtc3test "sub-second writes").
                self.subsecond = 0;
            }
            0x09 => self.regs[1] = v & 0x3f,
            0x0a => self.regs[2] = v & 0x1f,
            0x0b => self.regs[3] = v,
            _ => self.regs[4] = v & 0xc1,
        }
        // A write is visible at once in the latched copy as well.
        self.latched[(reg - 8) as usize] = self.regs[(reg - 8) as usize];
    }

    fn read(&self, reg: u8) -> u8 {
        let v = self.latched[(reg - 8) as usize];
        match reg {
            0x08 | 0x09 => v | 0xc0,
            0x0a => v | 0xe0,
            0x0c => v | 0x3e,
            _ => v,
        }
    }
}

#[derive(Clone, Debug)]
enum Mbc {
    None,
    Mbc1 {
        bank: u8,
        upper: u8,
        mode: bool,
        multicart: bool,
    },
    Mbc2 {
        bank: u8,
    },
    Mbc3 {
        bank: u8,
        select: u8,
        rtc: Option<Rtc>,
    },
    Mbc5 {
        bank: u16,
        ram_bank: u8,
        rumble: bool,
    },
}

/// A cartridge: ROM, the bank controller's state, and cartridge RAM.
#[derive(Clone, Debug)]
pub struct Cartridge {
    rom: Vec<u8>,
    ram: Vec<u8>,
    header: Header,
    mbc: Mbc,
    ram_enabled: bool,
    battery: bool,
    /// Set when the game writes to battery-backed RAM or the clock; cleared by the frontend
    /// once it has saved.
    pub dirty: bool,
    /// The ROM offsets of the banks at $0000 and $4000, and of RAM at $A000, kept up to date on
    /// every bank write so a read is one addition.
    rom0: usize,
    rom1: usize,
    ram_offset: usize,
}

impl Cartridge {
    /// Makes a cartridge from a ROM image.
    pub fn new(mut rom: Vec<u8>) -> Result<Cartridge, CartError> {
        if rom.len() < 0x150 {
            return Err(CartError::BadSize(rom.len()));
        }
        // Pad to a whole number of banks (a power of two), so bank masks can't read past it.
        let size = rom.len().next_power_of_two().max(0x8000);
        rom.resize(size, 0xff);
        let header = Header::parse(&rom);
        let t = header.cart_type;
        let battery = matches!(t, 0x03 | 0x06 | 0x09 | 0x0f | 0x10 | 0x13 | 0x1b | 0x1e);
        let mbc = match t {
            0x00 | 0x08 | 0x09 => Mbc::None,
            0x01..=0x03 => Mbc::Mbc1 {
                bank: 1,
                upper: 0,
                mode: false,
                multicart: is_mbc1m(&rom),
            },
            0x05 | 0x06 => Mbc::Mbc2 { bank: 1 },
            0x0f..=0x13 => Mbc::Mbc3 {
                bank: 1,
                select: 0,
                rtc: matches!(t, 0x0f | 0x10).then(Rtc::default),
            },
            0x19..=0x1e => Mbc::Mbc5 {
                bank: 1,
                ram_bank: 0,
                rumble: t >= 0x1c,
            },
            _ => return Err(CartError::Unsupported(t)),
        };
        let ram_size = match mbc {
            Mbc::Mbc2 { .. } => 512,
            _ => header.ram_size,
        };
        let mut cart = Cartridge {
            rom,
            ram: vec![0xff; ram_size],
            header,
            mbc,
            ram_enabled: false,
            battery,
            dirty: false,
            rom0: 0,
            rom1: 0x4000,
            ram_offset: 0,
        };
        cart.remap();
        Ok(cart)
    }

    /// The header.
    pub fn header(&self) -> &Header {
        &self.header
    }

    /// Whether RAM (or the clock) is kept by a battery, and so saved.
    pub fn has_battery(&self) -> bool {
        self.battery
    }

    /// Cartridge RAM, for saving (empty when there is none).
    pub fn ram(&self) -> &[u8] {
        &self.ram
    }

    /// Restores cartridge RAM from a save; a save of another size is used as far as it goes.
    pub fn load_ram(&mut self, data: &[u8]) {
        let n = data.len().min(self.ram.len());
        self.ram[..n].copy_from_slice(&data[..n]);
    }

    /// The MBC3 clock, if this cartridge has one.
    pub fn rtc(&self) -> Option<&Rtc> {
        match &self.mbc {
            Mbc::Mbc3 { rtc, .. } => rtc.as_ref(),
            _ => None,
        }
    }

    /// The MBC3 clock, for restoring it from a save.
    pub fn rtc_mut(&mut self) -> Option<&mut Rtc> {
        match &mut self.mbc {
            Mbc::Mbc3 { rtc, .. } => rtc.as_mut(),
            _ => None,
        }
    }

    /// The MBC3 clock as saved next to battery RAM (`<ROM name>.rtc`): `"IRTC"`, a version
    /// byte (1), the five registers, the five latched registers, the 32768 Hz ticks into the
    /// current second (u32), and the wall-clock time of the save in Unix seconds (u64), all
    /// little-endian: 27 bytes. `None` for a cartridge without a clock.
    pub fn rtc_save(&self, unix_now: u64) -> Option<Vec<u8>> {
        let rtc = self.rtc()?;
        let mut out = Vec::with_capacity(27);
        out.extend_from_slice(b"IRTC\x01");
        out.extend_from_slice(&rtc.regs);
        out.extend_from_slice(&rtc.latched);
        out.extend_from_slice(&rtc.subsecond.to_le_bytes());
        out.extend_from_slice(&unix_now.to_le_bytes());
        Some(out)
    }

    /// Restores the clock from [`Cartridge::rtc_save`]'s bytes and moves it on by the time
    /// since they were saved, as the cartridge's battery-powered clock would have run. Returns
    /// whether the bytes were a clock save.
    pub fn rtc_load(&mut self, data: &[u8], unix_now: u64) -> bool {
        let Some(rtc) = self.rtc_mut() else {
            return false;
        };
        if data.len() != 27 || &data[..5] != b"IRTC\x01" {
            return false;
        }
        rtc.regs.copy_from_slice(&data[5..10]);
        rtc.latched.copy_from_slice(&data[10..15]);
        rtc.subsecond = u32::from_le_bytes([data[15], data[16], data[17], data[18]]) % 32768;
        let saved = u64::from_le_bytes(data[19..27].try_into().unwrap_or([0; 8]));
        rtc.advance_seconds(unix_now.saturating_sub(saved));
        true
    }

    /// Whether an MBC5 rumble cartridge has its motor on.
    pub fn rumble(&self) -> bool {
        matches!(self.mbc, Mbc::Mbc5 { rumble: true, ram_bank, .. } if ram_bank & 8 != 0)
    }

    /// Advances the cartridge's clock, if it has one, by `dots` of machine time.
    pub fn tick(&mut self, dots: u32) {
        if let Mbc::Mbc3 { rtc: Some(rtc), .. } = &mut self.mbc {
            rtc.tick_dots(dots);
        }
    }

    fn remap(&mut self) {
        let banks = self.rom.len() / 0x4000;
        let (rom0, rom1, ram_bank) = match self.mbc {
            Mbc::None => (0, 1, 0),
            Mbc::Mbc1 {
                bank,
                upper,
                mode,
                multicart,
            } => {
                let low = if bank & 0x1f == 0 { 1 } else { bank & 0x1f } as usize;
                let (low, shift) = if multicart { (low & 0xf, 4) } else { (low, 5) };
                let high = (upper as usize) << shift;
                let rom0 = if mode { high } else { 0 };
                let ram = if mode { upper as usize } else { 0 };
                (rom0, high | low, ram)
            }
            Mbc::Mbc2 { bank } => (0, (bank & 0xf).max(1) as usize, 0),
            Mbc::Mbc3 { bank, select, .. } => (0, bank.max(1) as usize, (select & 7) as usize),
            Mbc::Mbc5 {
                bank,
                ram_bank,
                rumble,
            } => (
                0,
                bank as usize,
                (ram_bank & if rumble { 7 } else { 0xf }) as usize,
            ),
        };
        self.rom0 = (rom0 % banks) * 0x4000;
        self.rom1 = (rom1 % banks) * 0x4000;
        self.ram_offset = if self.ram.len() >= 0x2000 {
            (ram_bank * 0x2000) % self.ram.len()
        } else {
            0
        };
    }

    /// A read from $0000-$7FFF.
    pub fn read_rom(&self, addr: u16) -> u8 {
        let base = if addr < 0x4000 { self.rom0 } else { self.rom1 };
        self.rom[base + (addr as usize & 0x3fff)]
    }

    /// A read from $A000-$BFFF.
    pub fn read_ram(&self, addr: u16) -> u8 {
        if !self.ram_enabled {
            return 0xff;
        }
        match &self.mbc {
            Mbc::Mbc2 { .. } => self.ram[addr as usize & 0x1ff] | 0xf0,
            Mbc::Mbc3 {
                select: s @ 0x08..=0x0c,
                rtc: Some(rtc),
                ..
            } => rtc.read(*s),
            Mbc::Mbc3 { select: 0x08.., .. } => 0xff,
            _ if self.ram.is_empty() => 0xff,
            _ => self.ram[self.ram_offset + (addr as usize & 0x1fff) % self.ram.len()],
        }
    }

    /// A write to $A000-$BFFF.
    pub fn write_ram(&mut self, addr: u16, v: u8) {
        if !self.ram_enabled {
            return;
        }
        match &mut self.mbc {
            Mbc::Mbc2 { .. } => self.ram[addr as usize & 0x1ff] = v & 0xf,
            Mbc::Mbc3 {
                select: s @ 0x08..=0x0c,
                rtc: Some(rtc),
                ..
            } => rtc.write(*s, v),
            Mbc::Mbc3 { select: 0x08.., .. } => return,
            _ if self.ram.is_empty() => return,
            _ => {
                let i = self.ram_offset + (addr as usize & 0x1fff) % self.ram.len();
                self.ram[i] = v;
            }
        }
        self.dirty |= self.battery;
    }

    /// A write to $0000-$7FFF: the bank controller's registers.
    pub fn write_rom(&mut self, addr: u16, v: u8) {
        let enable = v & 0xf == 0xa;
        match &mut self.mbc {
            Mbc::None => return,
            Mbc::Mbc1 {
                bank, upper, mode, ..
            } => match addr {
                0x0000..=0x1fff => self.ram_enabled = enable,
                0x2000..=0x3fff => *bank = v & 0x1f,
                0x4000..=0x5fff => *upper = v & 3,
                _ => *mode = v & 1 != 0,
            },
            Mbc::Mbc2 { bank } => {
                if addr < 0x4000 {
                    if addr & 0x100 == 0 {
                        self.ram_enabled = enable;
                    } else {
                        *bank = v & 0xf;
                    }
                }
            }
            Mbc::Mbc3 { bank, select, rtc } => match addr {
                0x0000..=0x1fff => self.ram_enabled = enable,
                0x2000..=0x3fff => *bank = v & 0x7f,
                0x4000..=0x5fff => *select = v & 0xf,
                _ => {
                    if let Some(rtc) = rtc {
                        if rtc.latch_armed && v == 1 {
                            rtc.latched = rtc.regs;
                        }
                        rtc.latch_armed = v == 0;
                    }
                }
            },
            Mbc::Mbc5 { bank, ram_bank, .. } => match addr {
                0x0000..=0x1fff => self.ram_enabled = enable,
                0x2000..=0x2fff => *bank = (*bank & 0x100) | v as u16,
                0x3000..=0x3fff => *bank = (*bank & 0xff) | ((v as u16 & 1) << 8),
                0x4000..=0x5fff => *ram_bank = v & 0xf,
                _ => {}
            },
        }
        self.remap();
    }
}

impl Rtc {
    fn visit<V: crate::state::Visitor>(&mut self, v: &mut V) {
        v.bytes(&mut self.regs);
        v.bytes(&mut self.latched);
        v.u32(&mut self.subsecond);
        v.bool(&mut self.latch_armed);
        v.u32(&mut self.dots);
    }
}

impl Cartridge {
    /// The bank controller's state and cartridge RAM. The game is the same one (the state's
    /// header says so), so the controller type and RAM size match.
    pub(crate) fn visit<V: crate::state::Visitor>(&mut self, v: &mut V) {
        v.bytes(&mut self.ram);
        v.bool(&mut self.ram_enabled);
        match &mut self.mbc {
            Mbc::None => {}
            Mbc::Mbc1 {
                bank, upper, mode, ..
            } => {
                v.u8(bank);
                v.u8(upper);
                v.bool(mode);
            }
            Mbc::Mbc2 { bank } => v.u8(bank),
            Mbc::Mbc3 { bank, select, rtc } => {
                v.u8(bank);
                v.u8(select);
                if let Some(rtc) = rtc {
                    rtc.visit(v);
                }
            }
            Mbc::Mbc5 { bank, ram_bank, .. } => {
                v.u16(bank);
                v.u8(ram_bank);
            }
        }
        self.remap();
        // Loading a state changes battery RAM, which then needs saving.
        self.dirty = self.battery;
    }
}

/// An MBC1 multicart (MBC1M) is 1 MiB with a second game's header in bank $10: its upper bank
/// bits select a game of 16 banks each (Pan Docs "MBC1M").
fn is_mbc1m(rom: &[u8]) -> bool {
    const LOGO_AT: usize = 0x104;
    rom.len() == 0x100000
        && rom[0x40000 + LOGO_AT..0x40000 + LOGO_AT + 48] == rom[LOGO_AT..LOGO_AT + 48]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rom(cart_type: u8, banks: usize, ram_code: u8) -> Vec<u8> {
        let mut rom = vec![0; banks * 0x4000];
        for (bank, chunk) in rom.chunks_mut(0x4000).enumerate() {
            chunk[0] = bank as u8;
        }
        rom[0x147] = cart_type;
        rom[0x149] = ram_code;
        rom
    }

    #[test]
    fn mbc1_bank_zero_reads_as_one_and_upper_bits_extend() {
        let mut c = Cartridge::new(rom(0x01, 128, 0)).unwrap();
        c.write_rom(0x2000, 0);
        assert_eq!(c.read_rom(0x4000), 1);
        c.write_rom(0x2000, 0x05);
        c.write_rom(0x4000, 0x02);
        assert_eq!(c.read_rom(0x4000), 0x45);
        // Banks $20/$40/$60 can't be reached at $4000; in mode 1 they appear at $0000.
        c.write_rom(0x2000, 0);
        assert_eq!(c.read_rom(0x4000), 0x41);
        assert_eq!(c.read_rom(0x0000), 0);
        c.write_rom(0x6000, 1);
        assert_eq!(c.read_rom(0x0000), 0x40);
    }

    #[test]
    fn mbc1_ram_needs_enabling_and_banks_only_in_mode_1() {
        let mut c = Cartridge::new(rom(0x03, 4, 3)).unwrap();
        c.write_ram(0xa000, 0x12);
        assert_eq!(c.read_ram(0xa000), 0xff);
        c.write_rom(0x0000, 0x0a);
        c.write_ram(0xa000, 0x12);
        c.write_rom(0x4000, 1);
        assert_eq!(c.read_ram(0xa000), 0x12);
        c.write_rom(0x6000, 1);
        assert_eq!(c.read_ram(0xa000), 0xff);
        assert!(c.dirty);
    }

    #[test]
    fn mbc2_ram_is_four_bits_and_echoes() {
        let mut c = Cartridge::new(rom(0x06, 16, 0)).unwrap();
        c.write_rom(0x0000, 0x0a);
        c.write_ram(0xa001, 0x5c);
        assert_eq!(c.read_ram(0xa201), 0xfc);
        c.write_rom(0x0100, 3);
        assert_eq!(c.read_rom(0x4000), 3);
    }

    #[test]
    fn mbc5_bank_zero_is_bank_zero_and_nine_bits() {
        let mut c = Cartridge::new(rom(0x19, 512, 0)).unwrap();
        c.write_rom(0x2000, 0);
        assert_eq!(c.read_rom(0x4000), 0);
        c.write_rom(0x2000, 0x34);
        c.write_rom(0x3000, 1);
        assert_eq!(c.read_rom(0x4000), 0x34);
        assert_eq!(c.rom1, 0x134 * 0x4000);
    }

    #[test]
    fn rtc_counts_latches_and_carries() {
        let mut c = Cartridge::new(rom(0x10, 4, 3)).unwrap();
        c.write_rom(0x0000, 0x0a);
        let rtc = c.rtc_mut().unwrap();
        rtc.regs = [59, 59, 23, 0xff, 1];
        c.tick(128 * 32768);
        c.write_rom(0x6000, 0);
        c.write_rom(0x6000, 1);
        c.write_rom(0x4000, 0x0c);
        // Day 511 + 1 overflows: day 0 with the carry bit.
        assert_eq!(c.read_ram(0xa000), 0x80 | 0x3e);
        c.write_rom(0x4000, 0x08);
        assert_eq!(c.read_ram(0xa000) & 0x3f, 0);
    }

    #[test]
    fn rtc_save_carries_the_time_while_off() {
        let mut c = Cartridge::new(rom(0x10, 4, 3)).unwrap();
        c.rtc_mut().unwrap().regs = [10, 20, 3, 5, 0];
        let saved = c.rtc_save(1_000_000).unwrap();
        assert_eq!(saved.len(), 27);
        let mut d = Cartridge::new(rom(0x10, 4, 3)).unwrap();
        // Off for a day, an hour, a minute and five seconds.
        assert!(d.rtc_load(&saved, 1_000_000 + 86_400 + 3_600 + 65));
        assert_eq!(d.rtc().unwrap().regs, [15, 21, 4, 6, 0]);
        assert!(!d.rtc_load(b"not a clock", 0));
    }

    #[test]
    fn header_fields() {
        let mut r = rom(0x1b, 64, 4);
        r[0x134..0x13d].copy_from_slice(b"INKDOT TS");
        r[0x143] = 0x80;
        let c = Cartridge::new(r).unwrap();
        assert_eq!(c.header().title, "INKDOT TS");
        assert!(c.header().supports_cgb() && !c.header().requires_cgb());
        assert_eq!(c.ram().len(), 0x20000);
        assert!(c.has_battery());
    }
}
