//! The audio processing unit. For now its registers only: they read back as the hardware's
//! do (unused bits set), and nothing makes sound yet.
//!
//! Source: Pan Docs "Audio Registers".

/// Bits that read as 1 in $FF10-$FF2F, whatever was written.
const READ_OR: [u8; 32] = [
    0x80, 0x3f, 0x00, 0xff, 0xbf, // NR10-NR14
    0xff, 0x3f, 0x00, 0xff, 0xbf, // -, NR21-NR24
    0x7f, 0xff, 0x9f, 0xff, 0xbf, // NR30-NR34
    0xff, 0xff, 0x00, 0x00, 0xbf, // -, NR41-NR44
    0x00, 0x00, 0x70, // NR50-NR52
    0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
];

/// The APU's registers and wave RAM.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Apu {
    regs: [u8; 32],
    wave: [u8; 16],
}

impl Apu {
    fn powered(&self) -> bool {
        self.regs[0x16] & 0x80 != 0
    }

    /// Reads $FF10-$FF3F.
    pub fn read(&self, addr: u16) -> u8 {
        match addr {
            0xff30..=0xff3f => self.wave[addr as usize - 0xff30],
            _ => {
                let i = addr as usize - 0xff10;
                self.regs[i] | READ_OR[i]
            }
        }
    }

    /// Writes $FF10-$FF3F.
    pub fn write(&mut self, addr: u16, v: u8) {
        match addr {
            0xff30..=0xff3f => self.wave[addr as usize - 0xff30] = v,
            0xff26 => {
                self.regs[0x16] = v & 0x80;
                if v & 0x80 == 0 {
                    self.regs[..0x16].fill(0);
                }
            }
            _ if self.powered() => self.regs[addr as usize - 0xff10] = v,
            _ => {}
        }
    }

    /// The register values the boot ROM leaves (Pan Docs "Power Up Sequence").
    pub fn set_post_boot(&mut self) {
        let boot: [(u16, u8); 21] = [
            (0xff26, 0xf1),
            (0xff10, 0x80),
            (0xff11, 0xbf),
            (0xff12, 0xf3),
            (0xff13, 0xff),
            (0xff14, 0xbf),
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
        // Channel 1 is still playing the boot sound's last note.
        self.regs[0x16] = 0x81;
    }
}
