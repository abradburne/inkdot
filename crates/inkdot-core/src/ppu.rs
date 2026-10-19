//! The picture processing unit, DMG and CGB.
//!
//! The PPU is stepped one dot (2^22 Hz) at a time. Each of the 154 lines is 456 dots: on the
//! 144 visible lines, an OAM scan (mode 2, 80 dots), then pixel transfer (mode 3), then HBlank
//! (mode 0) for the rest; lines 144-153 are VBlank (mode 1).
//!
//! Mode 3 is modelled as the hardware draws it: a background fetcher fills a pixel FIFO, which
//! shifts one pixel out per dot. The first fetch of a line is done twice, the first SCX % 8
//! pixels are discarded, starting the window restarts the fetcher, and objects stall the output
//! while their tiles are fetched. So mode 3's length, and where on the line a mid-line register
//! write takes effect, follow from the model rather than from a table. The stall for each object
//! is Pan Docs' "OBJ penalty algorithm".
//!
//! Sources: Pan Docs "Rendering", "Pixel FIFO", "STAT", "LCDC", "OAM", "Tile Data", "Tile
//! Maps", "Window", "Palettes", "Accessing VRAM and OAM"; the Mooneye PPU tests, dmg-acid2 and
//! cgb-acid2 for the details.

/// The screen's width in pixels.
pub const WIDTH: usize = 160;
/// The screen's height in pixels.
pub const HEIGHT: usize = 144;
/// Dots in one line.
pub const LINE_DOTS: u16 = 456;
// The line's schedule, in dots from its start. The PPU's internal mode (which drives the STAT
// interrupt and the memory locks) leads what STAT reports by a few dots; these values are the
// ones the Mooneye PPU tests pin down to the dot (`intr_2_*_timing`, `hblank_ly_scx_timing`,
// `lcdon_timing`, `lcdon_write_timing`).
/// LY shows the new line.
const LY_AT: u16 = 4;
/// STAT reports mode 2 (the mode 2 interrupt and the OAM lock come at dot 0).
const MODE2_READ_AT: u16 = 4;
/// VRAM is locked for reads ahead of the pixel transfer; writes still go through until mode 3.
const VRAM_LOCK_AT: u16 = 80;
/// OAM writes are locked during the scan from here (reads from dot 0)...
const OAM_WRITE_LOCK_AT: u16 = 4;
/// ...until the scan ends, and again from mode 3: a write in between goes through (Mooneye
/// `lcdon_write_timing`).
const OAM_SCAN_END: u16 = 80;
/// Pixel transfer starts and STAT reports mode 3.
const MODE3_AT: u16 = 84;
/// Turning the LCD on starts line 0 this many dots in: that line is shorter and has no OAM
/// scan.
const LCD_ON_DOT: u16 = 8;
/// Dots in one frame.
pub const FRAME_DOTS: u32 = 456 * 154;

/// Events a dot can raise, as bits.
pub mod event {
    /// VBlank started: request the VBlank interrupt; a frame is complete.
    pub const VBLANK: u8 = 1;
    /// The STAT interrupt line rose.
    pub const STAT: u8 = 2;
    /// HBlank started on a visible line (for the CGB's HBlank DMA).
    pub const HBLANK: u8 = 4;
}

/// An object found by the OAM scan.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Sprite {
    x: u8,
    y: u8,
    tile: u8,
    attr: u8,
    index: u8,
}

/// One pixel in the object FIFO.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct ObjPixel {
    /// 0 is transparent.
    color: u8,
    /// The attribute byte (palette, priority).
    attr: u8,
    /// The OAM index, for the CGB's priority by OAM order.
    index: u8,
}

/// The background fetcher's state.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Fetcher {
    /// Dots into the current fetch: 0-5 read the tile number, data low, data high (two dots
    /// each); 6 and on, waiting to push into an empty FIFO.
    dot: u8,
    /// The tile column being fetched, counted from the start of the line or of the window.
    x: u8,
    tile: u8,
    attr: u8,
    lo: u8,
    hi: u8,
}

/// The PPU: video memory, registers and rendering state.
#[derive(Clone, Debug)]
pub struct Ppu {
    /// Video RAM, both banks (the second only exists on a CGB).
    pub vram: [u8; 0x4000],
    /// Object attribute memory.
    pub oam: [u8; 160],
    lcdc: u8,
    /// STAT's interrupt selects (bits 3-6).
    stat: u8,
    scy: u8,
    scx: u8,
    lyc: u8,
    bgp: u8,
    obp: [u8; 2],
    wy: u8,
    wx: u8,
    /// A CGB, in either mode.
    cgb: bool,
    /// A CGB running a CGB game; false on a DMG and for DMG games on a CGB.
    cgb_mode: bool,
    vbk: u8,
    bg_pal: [u8; 64],
    obj_pal: [u8; 64],
    bgpi: u8,
    obpi: u8,
    /// OPRI bit 0: objects take priority by X (as on a DMG) rather than by OAM index.
    pub opri: u8,

    /// The line being drawn (0-153).
    line: u8,
    /// LY as read: equal to `line`, except early on line 153, which reads 0 after a few dots.
    ly: u8,
    /// Dots into the line.
    dot: u16,
    /// The mode, as the STAT interrupt line sees it.
    mode: u8,
    /// The mode STAT reports, a few dots behind `mode` at each change.
    read_mode: u8,
    /// The mode 0 interrupt came last dot: STAT reports it and memory unlocks now.
    hblank_next: bool,
    oam_lock: bool,
    vram_lock: bool,
    oam_write_lock: bool,
    vram_write_lock: bool,
    /// The STAT interrupt line, whose rising edge requests the interrupt.
    stat_line: bool,
    lyc_match: bool,
    /// Dots until LY is compared with LYC after a change.
    compare_in: u8,
    /// The LY value to compare at the start of the next dot.
    compare_ly: Option<u8>,
    /// The LCD was turned on this frame: the first line has no OAM scan and the frame isn't
    /// shown.
    first_frame: bool,
    /// The window's Y condition: WY matched LY at the start of some line this frame.
    win_y: bool,
    /// The window row to draw next.
    win_line: u8,
    /// The window row being drawn on this line.
    win_row: u8,
    in_window: bool,

    sprites: [Sprite; 10],
    n_sprites: u8,
    next_sprite: u8,
    /// Set by the bus while an OAM DMA is copying: the scan sees no objects.
    pub dma_active: bool,

    /// Pixels shifted out on this line (0-160).
    lx: u8,
    /// Pixels still to discard at the start of the line (SCX % 8).
    discard: u8,
    /// Dots of object fetching still to wait.
    stall: u8,
    /// This line's pixel transfer is on the fast path: nothing it depends on has changed
    /// since it began, so it is drawn in one pass when it ends (`fast_len` dots in), with the
    /// window from pixel `fast_window` if it starts mid-line.
    fast: bool,
    fast_len: u16,
    fast_window: Option<u8>,
    /// Always run the pixel FIFO, never the fast path (for tests that compare the two).
    pub fifo_only: bool,
    /// The end of the run of idle dots last found ([`Ppu::idle_dots`]); 0 when none is known.
    /// Anything that could change what is idle clears it.
    idle_end: u16,
    /// The background FIFO: two bit planes shifting out from bit 7, all pixels of one tile.
    bg_lo: u8,
    bg_hi: u8,
    bg_len: u8,
    bg_attr: u8,
    fetcher: Fetcher,
    /// The first fetch of a line is done twice; this counts down the dots of the first.
    warmup: u8,
    obj: [ObjPixel; 8],
    obj_head: u8,
    /// The last background tile an object's penalty was counted for.
    last_penalty_tile: Option<i32>,

    /// The finished picture: shades 0-3 on a DMG, RGB555 colours on a CGB.
    frame: [u16; WIDTH * HEIGHT],
    /// The picture being drawn.
    back: [u16; WIDTH * HEIGHT],
}

impl Ppu {
    /// A PPU for a DMG (`cgb` false) or a CGB, in CGB mode or not.
    pub fn new(cgb: bool, cgb_mode: bool) -> Ppu {
        Ppu {
            vram: [0; 0x4000],
            oam: [0; 160],
            lcdc: 0,
            stat: 0,
            scy: 0,
            scx: 0,
            lyc: 0,
            bgp: 0,
            obp: [0; 2],
            wy: 0,
            wx: 0,
            cgb,
            cgb_mode,
            vbk: 0,
            bg_pal: [0xff; 64],
            obj_pal: [0; 64],
            bgpi: 0,
            obpi: 0,
            opri: if cgb_mode { 0 } else { 1 },
            line: 0,
            ly: 0,
            dot: 0,
            mode: 0,
            read_mode: 0,
            hblank_next: false,
            oam_lock: false,
            vram_lock: false,
            oam_write_lock: false,
            vram_write_lock: false,
            stat_line: false,
            lyc_match: false,
            compare_in: 0,
            compare_ly: None,
            first_frame: false,
            win_y: false,
            win_line: 0,
            win_row: 0,
            in_window: false,
            sprites: [Sprite::default(); 10],
            n_sprites: 0,
            next_sprite: 0,
            dma_active: false,
            lx: 0,
            discard: 0,
            stall: 0,
            fast: false,
            fast_len: 0,
            fast_window: None,
            fifo_only: false,
            idle_end: 0,
            bg_lo: 0,
            bg_hi: 0,
            bg_len: 0,
            bg_attr: 0,
            fetcher: Fetcher::default(),
            warmup: 0,
            obj: [ObjPixel::default(); 8],
            obj_head: 0,
            last_penalty_tile: None,
            frame: [0; WIDTH * HEIGHT],
            back: [0; WIDTH * HEIGHT],
        }
    }

    /// The last complete picture, row by row: on a DMG, shades 0 (lightest) to 3; on a CGB,
    /// RGB555 colours (red in bits 0-4).
    pub fn frame(&self) -> &[u16; WIDTH * HEIGHT] {
        &self.frame
    }

    /// Whether the picture holds RGB555 colours (a CGB) rather than DMG shades.
    pub fn is_color(&self) -> bool {
        self.cgb
    }

    /// The line being drawn and the dot within it, for tests and debuggers.
    pub fn position(&self) -> (u8, u16) {
        (self.line, self.dot)
    }

    fn lcd_on(&self) -> bool {
        self.lcdc & 0x80 != 0
    }

    /// Puts the PPU where the boot ROM leaves it at $0100: on line 153 after LY has reset to
    /// 0, part-way into the line (`dot`).
    pub fn set_post_boot(&mut self, lcdc: u8, bgp: u8, dot: u16) {
        self.lcdc = lcdc;
        self.bgp = bgp;
        self.idle_end = 0;
        self.line = 153;
        self.ly = 0;
        self.dot = dot;
        self.mode = 1;
        self.read_mode = 1;
        self.lyc_match = self.lyc == 0;
        self.stat_line = self.compute_stat_line();
    }

    /// Sets the palette index registers, as the CGB boot ROM leaves them after writing the
    /// compatibility palettes.
    pub fn set_palette_indexes(&mut self, bgpi: u8, obpi: u8) {
        self.bgpi = bgpi;
        self.obpi = obpi;
    }

    /// Writes a CGB colour palette directly, as the CGB boot ROM does for DMG games (palette
    /// RAM, `obj` selects which, `index` 0-7, four RGB555 colours).
    pub fn set_palette(&mut self, obj: bool, index: usize, colors: [u16; 4]) {
        let ram = if obj {
            &mut self.obj_pal
        } else {
            &mut self.bg_pal
        };
        for (i, c) in colors.iter().enumerate() {
            let [lo, hi] = c.to_le_bytes();
            ram[index * 8 + i * 2] = lo;
            ram[index * 8 + i * 2 + 1] = hi;
        }
    }

    fn compute_stat_line(&self) -> bool {
        let s = self.stat;
        // With the LCD off the comparison clock stops: the LY=LYC flag, and its part of the
        // interrupt line, keep their last value (Mooneye `stat_lyc_onoff`).
        let lyc = self.lyc_match && s & 0x40 != 0;
        if !self.lcd_on() {
            return lyc;
        }
        lyc || (self.mode == 0 && s & 0x08 != 0)
            || (self.mode == 1 && s & 0x10 != 0)
            || (self.mode == 2 && s & 0x20 != 0)
    }

    /// Re-evaluates the STAT line after a change, returning [`event::STAT`] on a rising edge.
    fn update_stat(&mut self) -> u8 {
        let line = self.compute_stat_line();
        let rose = line && !self.stat_line;
        self.stat_line = line;
        if rose {
            event::STAT
        } else {
            0
        }
    }

    // CPU access.

    fn vram_blocked(&self) -> bool {
        self.lcd_on() && self.vram_lock
    }

    fn oam_blocked(&self) -> bool {
        self.lcd_on() && self.oam_lock
    }

    /// A CPU read of $8000-$9FFF.
    pub fn read_vram(&self, addr: u16) -> u8 {
        if self.vram_blocked() {
            return 0xff;
        }
        self.vram[(self.vbk as usize) << 13 | (addr as usize & 0x1fff)]
    }

    /// A CPU write to $8000-$9FFF.
    pub fn write_vram(&mut self, addr: u16, v: u8) {
        if !(self.lcd_on() && self.vram_write_lock) {
            self.vram[(self.vbk as usize) << 13 | (addr as usize & 0x1fff)] = v;
        }
    }

    /// A CPU read of $FE00-$FE9F.
    pub fn read_oam(&self, addr: u16) -> u8 {
        if self.oam_blocked() {
            return 0xff;
        }
        self.oam[addr as usize & 0xff]
    }

    /// A CPU write to $FE00-$FE9F.
    pub fn write_oam(&mut self, addr: u16, v: u8) {
        if !(self.lcd_on() && self.oam_write_lock) {
            self.oam[addr as usize & 0xff] = v;
        }
    }

    /// A VRAM write by the CGB's VRAM DMA, into the bank VBK selects.
    pub fn dma_write_vram(&mut self, addr: u16, v: u8) {
        self.leave_fast_path();
        self.vram[(self.vbk as usize) << 13 | (addr as usize & 0x1fff)] = v;
    }

    /// Reads a PPU register ($FF40-$FF4B, $FF4F, $FF68-$FF6C).
    pub fn read(&self, addr: u16) -> u8 {
        match addr {
            0xff40 => self.lcdc,
            0xff41 => {
                let mode = if self.lcd_on() { self.read_mode } else { 0 };
                0x80 | self.stat | (self.lyc_match as u8) << 2 | mode
            }
            0xff42 => self.scy,
            0xff43 => self.scx,
            0xff44 => self.ly,
            0xff45 => self.lyc,
            0xff47 => self.bgp,
            0xff48 => self.obp[0],
            0xff49 => self.obp[1],
            0xff4a => self.wy,
            0xff4b => self.wx,
            // On a CGB, VBK and the palette indexes read back in either mode; in DMG
            // compatibility mode only the CGB boot ROM could write them (Mooneye
            // `boot_hwio-C`, `unused_hwio-C`).
            0xff4f if self.cgb => 0xfe | self.vbk,
            0xff68 if self.cgb => self.bgpi | 0x40,
            0xff69 if self.cgb_mode => {
                if self.vram_blocked() {
                    0xff
                } else {
                    self.bg_pal[(self.bgpi & 0x3f) as usize]
                }
            }
            0xff6a if self.cgb => self.obpi | 0x40,
            0xff6b if self.cgb_mode => {
                if self.vram_blocked() {
                    0xff
                } else {
                    self.obj_pal[(self.obpi & 0x3f) as usize]
                }
            }
            0xff6c if self.cgb => 0xfe | self.opri,
            _ => 0xff,
        }
    }

    /// Writes a PPU register. Returns any events the write raises (a STAT interrupt).
    pub fn write(&mut self, addr: u16, v: u8) -> u8 {
        self.idle_end = 0;
        // What the line being drawn depends on: the FIFO takes over before it changes.
        if matches!(
            addr,
            0xff40 | 0xff42 | 0xff43 | 0xff47..=0xff49 | 0xff4b | 0xff6c
        ) {
            self.leave_fast_path();
        }
        match addr {
            0xff40 => {
                let was_on = self.lcd_on();
                self.lcdc = v;
                // Clearing the window enable on a CGB forgets the frame's Y condition.
                if self.cgb && v & 0x20 == 0 {
                    self.win_y = false;
                }
                if was_on && !self.lcd_on() {
                    self.turn_off();
                } else if !was_on && self.lcd_on() {
                    self.turn_on();
                }
                return self.update_stat();
            }
            0xff41 => {
                // The DMG raises a spurious STAT interrupt on any write to STAT while one of
                // its conditions holds: as if $FF were written for a cycle (Pan Docs "Spurious
                // STAT interrupts"). The CGB doesn't.
                let mut ev = 0;
                if !self.cgb {
                    self.stat = 0x78;
                    ev = self.update_stat();
                }
                self.stat = v & 0x78;
                return ev | self.update_stat();
            }
            0xff42 => self.scy = v,
            0xff43 => self.scx = v,
            0xff45 => {
                self.lyc = v;
                if self.lcd_on() {
                    self.lyc_match = self.ly == self.lyc;
                }
                return self.update_stat();
            }
            0xff47 => self.bgp = v,
            0xff48 => self.obp[0] = v,
            0xff49 => self.obp[1] = v,
            0xff4a => self.wy = v,
            0xff4b => self.wx = v,
            0xff4f if self.cgb_mode => self.vbk = v & 1,
            0xff68 if self.cgb_mode => self.bgpi = v & 0xbf,
            0xff69 if self.cgb_mode => {
                if !self.vram_blocked() {
                    self.bg_pal[(self.bgpi & 0x3f) as usize] = v;
                }
                // The index moves on even when the write is blocked.
                if self.bgpi & 0x80 != 0 {
                    self.bgpi = 0x80 | (self.bgpi.wrapping_add(1) & 0x3f);
                }
            }
            0xff6a if self.cgb_mode => self.obpi = v & 0xbf,
            0xff6b if self.cgb_mode => {
                if !self.vram_blocked() {
                    self.obj_pal[(self.obpi & 0x3f) as usize] = v;
                }
                if self.obpi & 0x80 != 0 {
                    self.obpi = 0x80 | (self.obpi.wrapping_add(1) & 0x3f);
                }
            }
            0xff6c if self.cgb_mode => self.opri = v & 1,
            _ => {}
        }
        0
    }

    fn turn_off(&mut self) {
        self.idle_end = 0;
        self.line = 0;
        self.ly = 0;
        self.dot = 0;
        self.unlock();
        self.in_window = false;
        // The screen goes blank: white on a DMG, white on a CGB too.
        let white = if self.cgb { 0x7fff } else { 0 };
        self.frame.fill(white);
    }

    fn turn_on(&mut self) {
        self.idle_end = 0;
        self.line = 0;
        self.ly = 0;
        self.dot = LCD_ON_DOT;
        // The first line after the LCD comes on has no OAM scan: it reads as mode 0 until
        // mode 3, and nothing is locked until then (Mooneye `lcdon_timing`).
        self.unlock();
        self.first_frame = true;
        self.win_y = self.wy == 0;
        self.win_line = 0;
        self.n_sprites = 0;
        self.lyc_match = self.lyc == 0;
        self.compare_in = 0;
        self.compare_ly = None;
    }

    fn unlock(&mut self) {
        self.mode = 0;
        self.read_mode = 0;
        self.hblank_next = false;
        self.oam_lock = false;
        self.vram_lock = false;
        self.oam_write_lock = false;
        self.vram_write_lock = false;
    }

    /// Runs `dots` dots, returning the [`event`]s raised. Runs of dots in which nothing can
    /// happen (see [`Ppu::idle_dots`]) are counted off at once.
    pub fn tick(&mut self, dots: u32) -> u8 {
        let mut ev = 0;
        if !self.lcd_on() {
            return 0;
        }
        let mut left = dots as u16;
        // Still within a run of idle dots found earlier: nothing to do but count.
        if self.dot + left <= self.idle_end {
            self.dot += left;
            return 0;
        }
        while left > 0 {
            let idle = self.idle_dots();
            if idle > 0 {
                self.idle_end = self.dot + idle;
                let n = idle.min(left);
                self.dot += n;
                left -= n;
            } else {
                ev |= self.step();
                left -= 1;
            }
        }
        ev
    }

    /// How many of the coming dots do nothing at all: no scheduled event, no pixel work, no
    /// change of LY or of the STAT line. That is the rest of HBlank, VBlank lines after their
    /// first few dots, and a fast-path pixel transfer until its last dot. The line's last dot
    /// and the first ones of the next always go through [`Ppu::step`].
    fn idle_dots(&self) -> u16 {
        let d = self.dot;
        if self.hblank_next || self.compare_in > 0 || self.compare_ly.is_some() || d <= LY_AT + 4 {
            return 0;
        }
        let last = LINE_DOTS - 1;
        if self.line < 144 {
            match self.mode {
                0 if d > MODE3_AT => last - d,
                3 if self.fast => (MODE3_AT + self.fast_len - 1).saturating_sub(d),
                _ => 0,
            }
        } else if self.line > 144 || d > MODE2_READ_AT {
            last - d
        } else {
            0
        }
    }

    /// One dot. Most dots outside pixel transfer have nothing scheduled and go straight to the
    /// counters; the STAT line is looked at again only when something it depends on changed.
    fn step(&mut self) -> u8 {
        let mut ev = 0;
        let mut changed = false;
        if self.hblank_next {
            self.hblank_next = false;
            self.read_mode = 0;
            self.oam_lock = false;
            self.vram_lock = false;
            self.oam_write_lock = false;
            self.vram_write_lock = false;
            changed = true;
        }
        let (line, d) = (self.line, self.dot);
        if self.mode != 3 && ((line < 144 && d <= MODE3_AT) || (line == 144 && d <= MODE2_READ_AT))
        {
            ev |= self.scheduled(line, d);
            changed = true;
        }
        if self.mode == 3 {
            if !self.fast {
                self.mode3_dot();
            } else if d + 1 - MODE3_AT >= self.fast_len {
                self.render_line_fast();
            }
            if self.lx == WIDTH as u8 {
                self.mode = 0;
                self.hblank_next = true;
                if self.in_window {
                    self.win_line = self.win_line.wrapping_add(1);
                }
                ev |= event::HBLANK;
                changed = true;
            }
        }
        self.dot += 1;
        if self.dot == LINE_DOTS {
            self.dot = 0;
            self.idle_end = 0;
            self.line = if self.line == 153 { 0 } else { self.line + 1 };
        }
        if self.dot <= LY_AT + 4 || self.compare_in > 0 || self.compare_ly.is_some() {
            self.ly_dot();
            changed = true;
        }
        if changed {
            ev |= self.update_stat();
        }
        ev
    }

    /// What happens at the scheduled dots: the start of a visible line up to mode 3 (the OAM
    /// scan, the locks, the reported mode), and the start of VBlank.
    fn scheduled(&mut self, line: u8, d: u16) -> u8 {
        let mut ev = 0;
        if line < 144 {
            let scan = !self.first_line_after_on();
            if d == 0 {
                self.start_line();
            }
            if scan {
                if d < 80 && d & 1 == 1 {
                    self.scan_entry((d / 2) as usize);
                }
                if d == MODE2_READ_AT {
                    self.read_mode = 2;
                }
                if d == OAM_WRITE_LOCK_AT {
                    self.oam_write_lock = true;
                }
                if d == OAM_SCAN_END {
                    self.oam_write_lock = false;
                }
                if d == VRAM_LOCK_AT {
                    self.vram_lock = true;
                }
            }
            if d == MODE3_AT {
                self.mode = 3;
                self.read_mode = 3;
                self.oam_lock = true;
                self.vram_lock = true;
                self.oam_write_lock = true;
                self.vram_write_lock = true;
                self.start_mode3();
                if !self.fifo_only {
                    self.plan_fast_line();
                }
            }
        } else {
            if d == 0 {
                self.mode = 1;
                self.win_y = false;
                self.win_line = 0;
                // Entering VBlank also raises the mode 2 condition for a moment (Mooneye
                // `vblank_stat_intr`).
                if self.stat & 0x20 != 0 && !self.stat_line {
                    ev |= event::STAT;
                    self.stat_line = true;
                }
            }
            // The VBlank interrupt comes with it on a DMG, and an M-cycle later on a CGB
            // (Mooneye `vblank_stat_intr-C`).
            if d == if self.cgb { 4 } else { 0 } {
                ev |= event::VBLANK;
                if self.first_frame {
                    self.first_frame = false;
                } else {
                    core::mem::swap(&mut self.frame, &mut self.back);
                }
            }
            if d == MODE2_READ_AT {
                self.read_mode = 1;
            }
        }
        ev
    }

    /// LY and the LY=LYC comparison, near the start of a line. When LY changes, the flag drops
    /// and is compared again 4 dots later; line 153 changes LY twice, to 153 and then to 0 for
    /// the rest of the line.
    fn ly_dot(&mut self) {
        if let Some(value) = self.compare_ly.take() {
            self.lyc_match = value == self.lyc;
        }
        let new_ly = match (self.line, self.dot) {
            (_, LY_AT) => Some(self.line),
            (153, d) if d == LY_AT + 4 => Some(0),
            _ => None,
        };
        if let Some(ly) = new_ly.filter(|&ly| ly != self.ly) {
            self.ly = ly;
            self.lyc_match = false;
            self.compare_in = 4;
        }
        if self.compare_in > 0 {
            self.compare_in -= 1;
            if self.compare_in == 0 {
                self.compare_ly = Some(self.ly);
            }
        }
    }

    fn first_line_after_on(&self) -> bool {
        self.first_frame && self.line == 0
    }

    fn start_line(&mut self) {
        if !self.first_line_after_on() {
            self.mode = 2;
            self.oam_lock = true;
        }
        self.n_sprites = 0;
        if self.wy == self.line {
            self.win_y = true;
        }
    }

    fn scan_entry(&mut self, i: usize) {
        if self.n_sprites == 10 || self.dma_active {
            return;
        }
        let y = self.oam[i * 4];
        let height = if self.lcdc & 4 != 0 { 16 } else { 8 };
        let top = self.line as i16 + 16 - y as i16;
        if (0..height).contains(&top) {
            self.sprites[self.n_sprites as usize] = Sprite {
                y,
                x: self.oam[i * 4 + 1],
                tile: self.oam[i * 4 + 2],
                attr: self.oam[i * 4 + 3],
                index: i as u8,
            };
            self.n_sprites += 1;
        }
    }

    fn start_mode3(&mut self) {
        let n = self.n_sprites as usize;
        // Objects are fetched left to right; ties keep OAM order (a stable sort).
        self.sprites[..n].sort_by_key(|s| s.x);
        self.next_sprite = 0;
        self.lx = 0;
        self.discard = self.scx & 7;
        self.stall = 0;
        self.bg_len = 0;
        self.fetcher = Fetcher::default();
        self.warmup = 6;
        self.obj = [ObjPixel::default(); 8];
        self.obj_head = 0;
        self.last_penalty_tile = None;
        self.in_window = false;
        self.fast = false;
        // WX below 7 starts the window before the first pixel, shifted left.
        if self.window_enabled() && self.wx < 7 {
            self.start_window();
            self.discard = if self.wx == 0 {
                self.scx & 7
            } else {
                7 - self.wx
            };
        }
    }

    fn window_enabled(&self) -> bool {
        // On a DMG, LCDC bit 0 off hides the window too.
        self.win_y && self.lcdc & 0x20 != 0 && (self.cgb_mode || self.lcdc & 1 != 0)
    }

    fn start_window(&mut self) {
        self.in_window = true;
        self.win_row = self.win_line;
        self.bg_len = 0;
        self.fetcher = Fetcher::default();
    }

    fn mode3_dot(&mut self) {
        if self.warmup > 0 {
            self.warmup -= 1;
            return;
        }
        if self.stall > 0 {
            self.stall -= 1;
            return;
        }
        if !self.in_window
            && self.discard == 0
            && self.window_enabled()
            && self.lx as u16 + 7 == self.wx as u16
        {
            self.start_window();
        }
        self.fetch_dot();
        if self.bg_len == 0 {
            return;
        }
        if self.discard > 0 {
            self.pop_bg();
            self.discard -= 1;
            return;
        }
        if self.sprite_hit() {
            return;
        }
        self.output_pixel();
    }

    /// Whether an object starts at the current pixel. If one does, its tile is fetched into
    /// the object FIFO and the output stalls for its penalty.
    fn sprite_hit(&mut self) -> bool {
        let mut penalty = 0u8;
        let objects_on = self.cgb || self.lcdc & 2 != 0;
        while (self.next_sprite as usize) < self.n_sprites as usize {
            let s = self.sprites[self.next_sprite as usize];
            let start = s.x as i16 - 8;
            if start > self.lx as i16 {
                break;
            }
            self.next_sprite += 1;
            if !objects_on {
                continue;
            }
            penalty += self.sprite_penalty(s, self.in_window);
            self.fetch_sprite(s);
        }
        if penalty > 0 {
            // This dot is the first of the stall.
            self.stall = penalty - 1;
            true
        } else {
            false
        }
    }

    /// Pan Docs "OBJ penalty algorithm": 6 dots to fetch the object's tile, plus the wait for
    /// the background fetch of the tile the object's leftmost pixel is in, counted once per
    /// tile. Objects at OAM X 0 wait as if their pixel began a tile whatever SCX is, so the
    /// first costs 11 and the others on the line 6 (Mooneye `intr_2_mode0_timing_sprites`).
    fn sprite_penalty(&mut self, s: Sprite, in_window: bool) -> u8 {
        let pixel = s.x as i32 - 8;
        let (tile, offset) = if s.x == 0 {
            (i32::MIN, 0)
        } else {
            let pos = if in_window {
                pixel - (self.wx as i32 - 7)
            } else {
                pixel + (self.scx & 7) as i32
            };
            (pos.div_euclid(8), pos.rem_euclid(8))
        };
        let mut penalty = 6;
        if self.last_penalty_tile != Some(tile) {
            self.last_penalty_tile = Some(tile);
            penalty += (7 - offset - 2).max(0) as u8;
        }
        penalty
    }

    /// An object's row on this line, both bit planes, flipped as its attributes say.
    fn sprite_row(&self, s: Sprite) -> (u8, u8) {
        let tall = self.lcdc & 4 != 0;
        let mut row = (self.line as i16 + 16 - s.y as i16) as u8;
        let mut tile = s.tile;
        if tall {
            tile &= 0xfe;
        }
        if s.attr & 0x40 != 0 {
            row = if tall { 15 } else { 7 } - row;
        }
        let bank = if self.cgb_mode && s.attr & 8 != 0 {
            0x2000
        } else {
            0
        };
        let addr = bank + tile as usize * 16 + row as usize * 2;
        let (lo, hi) = (self.vram[addr], self.vram[addr + 1]);
        if s.attr & 0x20 != 0 {
            (lo.reverse_bits(), hi.reverse_bits())
        } else {
            (lo, hi)
        }
    }

    fn fetch_sprite(&mut self, s: Sprite) {
        let (lo, hi) = self.sprite_row(s);
        let start = s.x as i16 - 8;
        for i in 0..8i16 {
            let k = start + i - self.lx as i16;
            if k < 0 {
                continue;
            }
            let color = (hi >> (7 - i) & 1) << 1 | (lo >> (7 - i) & 1);
            let slot = &mut self.obj[(self.obj_head as usize + k as usize) & 7];
            // A pixel already in the FIFO keeps its place unless it is transparent; on a CGB
            // in CGB mode, a lower OAM index wins instead (OPRI clear).
            let by_index = self.cgb_mode && self.opri & 1 == 0;
            let replace = color != 0 && (slot.color == 0 || (by_index && s.index < slot.index));
            if replace {
                *slot = ObjPixel {
                    color,
                    attr: s.attr,
                    index: s.index,
                };
            }
        }
    }

    /// Starts this line's pixel transfer on the fast path: works out how long the FIFO would
    /// take, by its own rules (12 dots for the first fetch done twice, the fine-scroll discard,
    /// 160 pixels, 6 if the window starts mid-line, each object's penalty), so the line can be
    /// drawn in one pass at the end. `tests::the_fast_path_matches_the_fifo` checks the two.
    fn plan_fast_line(&mut self) {
        let from_start = self.in_window;
        let mid = (!from_start && self.window_enabled() && (7..167).contains(&self.wx))
            .then(|| self.wx - 7);
        let mut len = 12 + self.discard as u16 + WIDTH as u16 + if mid.is_some() { 6 } else { 0 };
        if self.cgb || self.lcdc & 2 != 0 {
            for i in 0..self.n_sprites as usize {
                let s = self.sprites[i];
                let start = s.x as i16 - 8;
                if start >= WIDTH as i16 {
                    break;
                }
                let at = start.max(0);
                let in_window = from_start || mid.is_some_and(|w| at >= w as i16);
                len += self.sprite_penalty(s, in_window) as u16;
            }
            self.last_penalty_tile = None;
        }
        self.fast = true;
        self.fast_len = len;
        self.fast_window = mid;
    }

    /// Leaves the fast path part-way through a line, before something it depends on changes:
    /// the FIFO is run from the start of pixel transfer to now, with the values that held until
    /// now, and carries on from there.
    fn leave_fast_path(&mut self) {
        if self.mode != 3 || !self.fast || !self.lcd_on() {
            return;
        }
        let elapsed = self.dot - MODE3_AT;
        self.idle_end = 0;
        self.start_mode3();
        for _ in 0..elapsed {
            self.mode3_dot();
        }
    }

    /// Draws the whole line as the FIFO would have: the background up to where the window
    /// starts, the window, and the objects by the FIFO's priority (the first fetched wins, or
    /// on a CGB in CGB mode the lowest OAM index).
    fn render_line_fast(&mut self) {
        let mut bg = [0u8; WIDTH];
        let mut attrs = [0u8; WIDTH];
        let from_start = self.in_window;
        let mid = self.fast_window;
        let mut fill = |ppu: &Ppu, window: bool, from: usize, skip: usize, y: u8, first_col: u8| {
            let mut lx = from;
            let mut col = first_col;
            let mut first = skip;
            let end = if window {
                WIDTH
            } else {
                mid.map_or(WIDTH, |w| w as usize)
            };
            while lx < end {
                let (tile, attr) = ppu.map_tile(window, col, y);
                let lo = ppu.tile_byte(tile, attr, y, false);
                let hi = ppu.tile_byte(tile, attr, y, true);
                for bit in first..8 {
                    if lx >= end {
                        break;
                    }
                    bg[lx] = (hi >> (7 - bit) & 1) << 1 | (lo >> (7 - bit) & 1);
                    attrs[lx] = attr;
                    lx += 1;
                }
                first = 0;
                col = col.wrapping_add(1);
            }
        };
        if !from_start {
            fill(
                self,
                false,
                0,
                (self.scx & 7) as usize,
                self.line.wrapping_add(self.scy),
                self.scx >> 3,
            );
        }
        if from_start || mid.is_some() {
            self.in_window = true;
            self.win_row = self.win_line;
            let (from, skip) = if from_start {
                (0, self.discard as usize)
            } else {
                (mid.unwrap_or(0) as usize, 0)
            };
            fill(self, true, from, skip, self.win_row, 0);
        }
        let mut obj = [ObjPixel::default(); WIDTH];
        if self.cgb || self.lcdc & 2 != 0 {
            let by_index = self.cgb_mode && self.opri & 1 == 0;
            for i in 0..self.n_sprites as usize {
                let s = self.sprites[i];
                let start = s.x as i16 - 8;
                if start >= WIDTH as i16 {
                    break;
                }
                let (lo, hi) = self.sprite_row(s);
                for b in 0..8i16 {
                    let x = start + b;
                    if !(0..WIDTH as i16).contains(&x) {
                        continue;
                    }
                    let color = (hi >> (7 - b) & 1) << 1 | (lo >> (7 - b) & 1);
                    let slot = &mut obj[x as usize];
                    if color != 0 && (slot.color == 0 || (by_index && s.index < slot.index)) {
                        *slot = ObjPixel {
                            color,
                            attr: s.attr,
                            index: s.index,
                        };
                    }
                }
            }
        }
        let row = self.line as usize * WIDTH;
        for lx in 0..WIDTH {
            self.back[row + lx] = self.mix(bg[lx], attrs[lx], obj[lx]);
        }
        self.lx = WIDTH as u8;
        self.fast = false;
    }

    /// A background or window tile: its number and (CGB) attributes, for tile column `col` of
    /// map row `y / 8`.
    fn map_tile(&self, window: bool, col: u8, y: u8) -> (u8, u8) {
        let map = if window {
            self.lcdc & 0x40 != 0
        } else {
            self.lcdc & 0x08 != 0
        };
        let idx = if map { 0x1c00 } else { 0x1800 } + (y as usize / 8) * 32 + (col & 31) as usize;
        let attr = if self.cgb_mode {
            self.vram[0x2000 + idx]
        } else {
            0
        };
        (self.vram[idx], attr)
    }

    /// One byte (low or high plane) of a background tile's row `y % 8`, flipped as its
    /// attributes say.
    fn tile_byte(&self, tile: u8, attr: u8, y: u8, high: bool) -> u8 {
        let mut row = y & 7;
        if attr & 0x40 != 0 {
            row = 7 - row;
        }
        let base = if self.lcdc & 0x10 != 0 {
            tile as usize * 16
        } else {
            (0x1000 + (tile as i8 as i32) * 16) as usize
        };
        let bank = if attr & 8 != 0 { 0x2000 } else { 0 };
        let v = self.vram[bank + base + row as usize * 2 + high as usize];
        if attr & 0x20 != 0 {
            v.reverse_bits()
        } else {
            v
        }
    }

    fn fetch_dot(&mut self) {
        let f = self.fetcher;
        let y = if self.in_window {
            self.win_row
        } else {
            self.line.wrapping_add(self.scy)
        };
        match f.dot {
            1 => {
                let col = if self.in_window {
                    f.x
                } else {
                    (self.scx >> 3).wrapping_add(f.x)
                };
                (self.fetcher.tile, self.fetcher.attr) = self.map_tile(self.in_window, col, y);
            }
            3 => self.fetcher.lo = self.tile_byte(f.tile, f.attr, y, false),
            5 => self.fetcher.hi = self.tile_byte(f.tile, f.attr, y, true),
            _ => {}
        }
        if self.fetcher.dot < 6 {
            self.fetcher.dot += 1;
        } else if self.bg_len == 0 {
            self.bg_lo = self.fetcher.lo;
            self.bg_hi = self.fetcher.hi;
            self.bg_attr = self.fetcher.attr;
            self.bg_len = 8;
            self.fetcher.x = self.fetcher.x.wrapping_add(1);
            self.fetcher.dot = 0;
        }
    }

    fn pop_bg(&mut self) -> u8 {
        let c = (self.bg_hi >> 7) << 1 | self.bg_lo >> 7;
        self.bg_lo <<= 1;
        self.bg_hi <<= 1;
        self.bg_len -= 1;
        c
    }

    fn pop_obj(&mut self) -> ObjPixel {
        let p = core::mem::take(&mut self.obj[self.obj_head as usize]);
        self.obj_head = (self.obj_head + 1) & 7;
        p
    }

    fn color(ram: &[u8; 64], palette: u8, index: u8) -> u16 {
        let i = palette as usize * 8 + index as usize * 2;
        u16::from_le_bytes([ram[i], ram[i + 1]]) & 0x7fff
    }

    /// The colour of one pixel from its background pixel (colour index and CGB attributes)
    /// and the object pixel over it: the same rules for the FIFO and for a whole line.
    fn mix(&self, mut bg: u8, attr: u8, obj: ObjPixel) -> u16 {
        let objects_on = self.lcdc & 2 != 0;
        if self.cgb_mode {
            // LCDC bit 0 is the master priority on a CGB: off, objects are always on top.
            let master = self.lcdc & 1 != 0;
            let bg_wins = obj.color == 0
                || !objects_on
                || (master && bg != 0 && (attr & 0x80 != 0 || obj.attr & 0x80 != 0));
            if bg_wins {
                Self::color(&self.bg_pal, attr & 7, bg)
            } else {
                Self::color(&self.obj_pal, obj.attr & 7, obj.color)
            }
        } else {
            if self.lcdc & 1 == 0 {
                bg = 0;
            }
            let bg_wins = obj.color == 0 || !objects_on || (obj.attr & 0x80 != 0 && bg != 0);
            let (pal, index, obj_pal) = if bg_wins {
                (self.bgp, bg, None)
            } else {
                let n = (obj.attr >> 4) & 1;
                (self.obp[n as usize], obj.color, Some(n))
            };
            let shade = (pal >> (index * 2)) & 3;
            if self.cgb {
                // A DMG game on a CGB: the shade indexes the compatibility palettes.
                match obj_pal {
                    None => Self::color(&self.bg_pal, 0, shade),
                    Some(n) => Self::color(&self.obj_pal, n, shade),
                }
            } else {
                shade as u16
            }
        }
    }

    fn output_pixel(&mut self) {
        let attr = self.bg_attr;
        let bg = self.pop_bg();
        let obj = self.pop_obj();
        let out = self.mix(bg, attr, obj);
        self.back[self.line as usize * WIDTH + self.lx as usize] = out;
        self.lx += 1;
    }
}

impl Ppu {
    pub(crate) fn visit<V: crate::state::Visitor>(&mut self, v: &mut V) {
        v.bytes(&mut self.vram);
        v.bytes(&mut self.oam);
        for r in [
            &mut self.lcdc,
            &mut self.stat,
            &mut self.scy,
            &mut self.scx,
            &mut self.lyc,
        ] {
            v.u8(r);
        }
        v.u8(&mut self.bgp);
        v.bytes(&mut self.obp);
        for r in [
            &mut self.wy,
            &mut self.wx,
            &mut self.vbk,
            &mut self.bgpi,
            &mut self.obpi,
        ] {
            v.u8(r);
        }
        v.bytes(&mut self.bg_pal);
        v.bytes(&mut self.obj_pal);
        v.u8(&mut self.opri);
        v.u8(&mut self.line);
        v.u8(&mut self.ly);
        v.u16(&mut self.dot);
        v.u8(&mut self.mode);
        v.u8(&mut self.read_mode);
        for r in [
            &mut self.hblank_next,
            &mut self.oam_lock,
            &mut self.vram_lock,
            &mut self.oam_write_lock,
            &mut self.vram_write_lock,
            &mut self.stat_line,
            &mut self.lyc_match,
        ] {
            v.bool(r);
        }
        v.u8(&mut self.compare_in);
        v.opt_u8(&mut self.compare_ly);
        v.bool(&mut self.first_frame);
        v.bool(&mut self.win_y);
        v.u8(&mut self.win_line);
        v.u8(&mut self.win_row);
        v.bool(&mut self.in_window);
        for s in &mut self.sprites {
            for r in [&mut s.x, &mut s.y, &mut s.tile, &mut s.attr, &mut s.index] {
                v.u8(r);
            }
        }
        v.u8(&mut self.n_sprites);
        v.u8(&mut self.next_sprite);
        v.bool(&mut self.dma_active);
        for r in [&mut self.lx, &mut self.discard, &mut self.stall] {
            v.u8(r);
        }
        for r in [
            &mut self.bg_lo,
            &mut self.bg_hi,
            &mut self.bg_len,
            &mut self.bg_attr,
        ] {
            v.u8(r);
        }
        let f = &mut self.fetcher;
        for r in [
            &mut f.dot,
            &mut f.x,
            &mut f.tile,
            &mut f.attr,
            &mut f.lo,
            &mut f.hi,
        ] {
            v.u8(r);
        }
        v.u8(&mut self.warmup);
        for p in &mut self.obj {
            for r in [&mut p.color, &mut p.attr, &mut p.index] {
                v.u8(r);
            }
        }
        v.u8(&mut self.obj_head);
        v.opt_i32(&mut self.last_penalty_tile);
        v.bool(&mut self.fast);
        v.u16(&mut self.fast_len);
        v.opt_u8(&mut self.fast_window);
        self.idle_end = 0;
        v.u16s(&mut self.frame);
        v.u16s(&mut self.back);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;

    /// A PPU with the LCD on, at the start of line 1 (line 0 after turning on is short and has
    /// no OAM scan).
    fn running(scx: u8) -> Ppu {
        let mut p = Ppu::new(false, false);
        p.write(0xff40, 0x91);
        p.scx = scx;
        while p.position() != (1, 0) {
            p.step();
        }
        p
    }

    /// Dots from the start of mode 3 until the mode 0 interrupt.
    fn mode3_end(p: &mut Ppu) -> u16 {
        while p.mode != 0 || p.dot <= MODE3_AT {
            p.step();
        }
        p.dot - MODE3_AT
    }

    #[test]
    fn mode3_is_172_dots_plus_scx_fine_scroll() {
        for scx in 0..8 {
            let mut p = running(scx);
            assert_eq!(mode3_end(&mut p), 172 + scx as u16, "scx {scx}");
        }
    }

    #[test]
    fn an_object_costs_its_penalty() {
        // One 8x8 object at X 8 on line 1, SCX 0: 6 dots plus 5 waiting for the first tile.
        let mut p = running(0);
        p.oam[0] = 17;
        p.oam[1] = 8;
        p.write(0xff40, 0x93);
        assert_eq!(mode3_end(&mut p), 172 + 11);
    }

    #[test]
    fn an_object_at_x_zero_costs_eleven() {
        let mut p = running(3);
        p.oam[0] = 17;
        p.oam[1] = 0;
        p.write(0xff40, 0x93);
        assert_eq!(mode3_end(&mut p), 172 + 3 + 11);
    }

    #[test]
    fn the_window_costs_six_dots() {
        let mut p = running(0);
        p.wy = 1;
        p.wx = 7 + 80;
        p.write(0xff40, 0xb1);
        assert_eq!(mode3_end(&mut p), 172 + 6);
    }

    /// A small, deterministic random number generator for the comparison below.
    struct Lcg(u64);

    impl Lcg {
        fn next(&mut self) -> u8 {
            self.0 = self
                .0
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (self.0 >> 56) as u8
        }
    }

    /// Runs two frames, making `writes` (at a dot count, a register, a value) on the way,
    /// and returns every line's pixels and the dot its mode 0 interrupt came.
    fn draw(p: &mut Ppu, writes: &[(u32, u16, u8)]) -> ([u16; WIDTH * HEIGHT], Vec<(u8, u16)>) {
        let mut hblanks = Vec::new();
        let mut next = 0;
        for t in 0..2 * FRAME_DOTS {
            while next < writes.len() && writes[next].0 == t {
                p.write(writes[next].1, writes[next].2);
                next += 1;
            }
            if p.step() & event::HBLANK != 0 {
                hblanks.push(p.position());
            }
        }
        (p.back, hblanks)
    }

    #[test]
    fn the_fast_path_matches_the_fifo() {
        let mut rng = Lcg(0x1234_5678);
        for case in 0..300 {
            let cgb = case % 3 != 0;
            let mut p = Ppu::new(cgb, cgb && case % 2 == 0);
            for b in p.vram.iter_mut() {
                *b = rng.next();
            }
            // Objects on the screen's lines, many of them near each other and the edges.
            for i in 0..40 {
                p.oam[i * 4] = 16 + (rng.next() % 150);
                p.oam[i * 4 + 1] = rng.next() % 176;
                p.oam[i * 4 + 2] = rng.next();
                p.oam[i * 4 + 3] = rng.next();
            }
            for b in p.bg_pal.iter_mut().chain(p.obj_pal.iter_mut()) {
                *b = rng.next();
            }
            p.scx = rng.next();
            p.scy = rng.next();
            p.wy = rng.next() % 160;
            p.wx = rng.next() % 176;
            p.bgp = rng.next();
            p.obp = [rng.next(), rng.next()];
            p.opri = rng.next() & 1;
            p.write(0xff40, 0x80 | (rng.next() & 0x7f));
            // Half the cases change scrolling, palettes, the window or LCDC mid-line, as raster
            // effects do: the fast path must hand over to the FIFO exactly.
            let mut writes = Vec::new();
            if case % 2 == 1 {
                let regs = [0xff42, 0xff43, 0xff47, 0xff48, 0xff4b, 0xff40];
                let mut t = 0u32;
                for _ in 0..200 {
                    t += 100 + rng.next() as u32 * 3;
                    let reg = regs[rng.next() as usize % regs.len()];
                    let v = if reg == 0xff40 {
                        0x80 | rng.next()
                    } else {
                        rng.next()
                    };
                    writes.push((t, reg, v));
                }
            }
            let mut fifo = p.clone();
            fifo.fifo_only = true;
            let (fast_px, fast_t) = draw(&mut p, &writes);
            let (fifo_px, fifo_t) = draw(&mut fifo, &writes);
            assert_eq!(fast_t, fifo_t, "case {case}: mode 3 lengths differ");
            assert!(fast_px == fifo_px, "case {case}: pixels differ");
        }
    }

    #[test]
    fn a_frame_is_70224_dots_and_raises_vblank_once() {
        let mut p = running(0);
        let mut vblanks = 0;
        for _ in 0..FRAME_DOTS {
            if p.step() & event::VBLANK != 0 {
                vblanks += 1;
            }
        }
        assert_eq!((vblanks, p.line, p.dot), (1, 1, 0));
    }
}
