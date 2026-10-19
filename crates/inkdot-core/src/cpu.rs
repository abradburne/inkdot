//! The SM83, the Game Boy's CPU.
//!
//! The CPU is stepped one instruction at a time, and every M-cycle of an instruction is a call on
//! the [`Bus`]: a read, a write, or an internal cycle. The bus advances the rest of the machine by
//! one M-cycle on each call, so the order and number of bus calls is the instruction's timing,
//! which is what the per-instruction test vectors check.
//!
//! Each instruction starts with the fetch of its opcode at PC. On hardware that fetch overlaps the
//! last cycle of the previous instruction; counting it at the start instead gives the same
//! number of cycles between any two events, and interrupts are checked just before it.
//!
//! Sources: Pan Docs "CPU Instruction Set", "Interrupts", "halt"; Game Boy: Complete Technical
//! Reference, "Sharp SM83 instruction set" (cycle-by-cycle bus activity).

/// What the CPU needs from the rest of the machine. Each method other than [`Bus::interrupts`]
/// and [`Bus::acknowledge`] takes one M-cycle.
pub trait Bus {
    /// Reads a byte.
    fn read(&mut self, addr: u16) -> u8;
    /// Writes a byte.
    fn write(&mut self, addr: u16, value: u8);
    /// A cycle with no memory access. `addr` is what the CPU puts on the address bus meanwhile,
    /// which matters to the DMG's OAM corruption bug (16-bit increments and decrements).
    fn idle(&mut self, addr: u16);
    /// The interrupts both requested and enabled (`IF & IE`, bits 0-4).
    fn interrupts(&self) -> u8;
    /// Clears the request for interrupt `bit` (0-4) as its handler is called.
    fn acknowledge(&mut self, bit: u8);
    /// The STOP instruction was executed. Returns whether the CPU stops (`false` when STOP did
    /// a CGB speed switch instead).
    fn stop(&mut self) -> bool {
        true
    }
    /// While stopped: whether to wake (a button pressed). Called once per M-cycle.
    fn stop_wake(&mut self) -> bool {
        true
    }
}

const Z: u8 = 0x80;
const N: u8 = 0x40;
const H: u8 = 0x20;
const C: u8 = 0x10;

/// The CPU's run state between instructions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Executing instructions.
    Running,
    /// After HALT: waiting for an interrupt to be requested.
    Halted,
    /// After STOP: waiting for a button.
    Stopped,
    /// An invalid opcode was executed; only a reset leaves this.
    Locked,
}

/// The CPU's registers and internal state.
#[derive(Clone, Debug, PartialEq, Eq)]
#[allow(missing_docs)]
pub struct Cpu {
    pub a: u8,
    pub f: u8,
    pub b: u8,
    pub c: u8,
    pub d: u8,
    pub e: u8,
    pub h: u8,
    pub l: u8,
    pub sp: u16,
    pub pc: u16,
    /// The interrupt master enable flag.
    pub ime: bool,
    /// EI was executed: IME becomes set after the next instruction.
    pub ei_pending: bool,
    /// The HALT bug: the next opcode fetch doesn't advance PC.
    pub halt_bug: bool,
    pub mode: Mode,
}

impl Default for Cpu {
    fn default() -> Cpu {
        Cpu {
            a: 0,
            f: 0,
            b: 0,
            c: 0,
            d: 0,
            e: 0,
            h: 0,
            l: 0,
            sp: 0,
            pc: 0,
            ime: false,
            ei_pending: false,
            halt_bug: false,
            mode: Mode::Running,
        }
    }
}

impl Cpu {
    /// BC as a 16-bit register.
    pub fn bc(&self) -> u16 {
        u16::from_be_bytes([self.b, self.c])
    }
    /// DE as a 16-bit register.
    pub fn de(&self) -> u16 {
        u16::from_be_bytes([self.d, self.e])
    }
    /// HL as a 16-bit register.
    pub fn hl(&self) -> u16 {
        u16::from_be_bytes([self.h, self.l])
    }
    /// AF as a 16-bit register.
    pub fn af(&self) -> u16 {
        u16::from_be_bytes([self.a, self.f])
    }
    fn set_bc(&mut self, v: u16) {
        [self.b, self.c] = v.to_be_bytes();
    }
    fn set_de(&mut self, v: u16) {
        [self.d, self.e] = v.to_be_bytes();
    }
    fn set_hl(&mut self, v: u16) {
        [self.h, self.l] = v.to_be_bytes();
    }
    fn set_af(&mut self, v: u16) {
        // The low four bits of F don't exist.
        [self.a, self.f] = (v & 0xfff0).to_be_bytes();
    }

    fn flag(&self, f: u8) -> bool {
        self.f & f != 0
    }

    fn set_flags(&mut self, z: bool, n: bool, h: bool, c: bool) {
        self.f = (z as u8) << 7 | (n as u8) << 6 | (h as u8) << 5 | (c as u8) << 4;
    }

    /// Runs one instruction, one interrupt dispatch, or one cycle of HALT or STOP.
    pub fn step(&mut self, bus: &mut impl Bus) {
        match self.mode {
            Mode::Running => {}
            Mode::Halted => {
                bus.idle(self.pc);
                if bus.interrupts() != 0 {
                    self.mode = Mode::Running;
                }
                return;
            }
            Mode::Stopped => {
                bus.idle(self.pc);
                if bus.stop_wake() {
                    self.mode = Mode::Running;
                }
                return;
            }
            Mode::Locked => {
                bus.idle(self.pc);
                return;
            }
        }
        if self.ime && bus.interrupts() != 0 {
            self.dispatch(bus);
            return;
        }
        let enable_after = self.ei_pending;
        let op = self.fetch(bus);
        self.execute(op, bus);
        if enable_after && self.ei_pending {
            self.ime = true;
            self.ei_pending = false;
        }
    }

    /// Calls the handler of the highest-priority pending interrupt: two internal cycles, PC
    /// pushed, PC set. Which handler is decided after the high byte of PC is pushed, so a push
    /// that overwrites IE ($FFFF) can cancel the dispatch, which then jumps to $0000
    /// (Mooneye's `ie_push`).
    fn dispatch(&mut self, bus: &mut impl Bus) {
        self.ime = false;
        // A HALT bug pending here means the HALT is returned to (Pan Docs "halt bug").
        if self.halt_bug {
            self.halt_bug = false;
            self.pc = self.pc.wrapping_sub(1);
        }
        bus.idle(self.pc);
        bus.idle(self.pc);
        let [hi, lo] = self.pc.to_be_bytes();
        self.sp = self.sp.wrapping_sub(1);
        bus.write(self.sp, hi);
        let pending = bus.interrupts();
        self.sp = self.sp.wrapping_sub(1);
        bus.write(self.sp, lo);
        if pending == 0 {
            self.pc = 0;
        } else {
            let bit = pending.trailing_zeros() as u8;
            bus.acknowledge(bit);
            self.pc = 0x40 + 8 * bit as u16;
        }
        bus.idle(self.pc);
    }

    fn fetch(&mut self, bus: &mut impl Bus) -> u8 {
        let v = bus.read(self.pc);
        if self.halt_bug {
            self.halt_bug = false;
        } else {
            self.pc = self.pc.wrapping_add(1);
        }
        v
    }

    fn fetch16(&mut self, bus: &mut impl Bus) -> u16 {
        let lo = self.fetch(bus);
        let hi = self.fetch(bus);
        u16::from_le_bytes([lo, hi])
    }

    /// An 8-bit operand by its 3-bit encoding: B C D E H L (HL) A.
    fn r8(&mut self, i: u8, bus: &mut impl Bus) -> u8 {
        match i {
            0 => self.b,
            1 => self.c,
            2 => self.d,
            3 => self.e,
            4 => self.h,
            5 => self.l,
            6 => bus.read(self.hl()),
            _ => self.a,
        }
    }

    fn set_r8(&mut self, i: u8, v: u8, bus: &mut impl Bus) {
        match i {
            0 => self.b = v,
            1 => self.c = v,
            2 => self.d = v,
            3 => self.e = v,
            4 => self.h = v,
            5 => self.l = v,
            6 => bus.write(self.hl(), v),
            _ => self.a = v,
        }
    }

    /// A 16-bit register by its 2-bit encoding: BC DE HL SP.
    fn r16(&self, i: u8) -> u16 {
        match i {
            0 => self.bc(),
            1 => self.de(),
            2 => self.hl(),
            _ => self.sp,
        }
    }

    fn set_r16(&mut self, i: u8, v: u16) {
        match i {
            0 => self.set_bc(v),
            1 => self.set_de(v),
            2 => self.set_hl(v),
            _ => self.sp = v,
        }
    }

    /// A condition by its 2-bit encoding: NZ Z NC C.
    fn cond(&self, i: u8) -> bool {
        match i {
            0 => !self.flag(Z),
            1 => self.flag(Z),
            2 => !self.flag(C),
            _ => self.flag(C),
        }
    }

    fn push(&mut self, v: u16, bus: &mut impl Bus) {
        let [hi, lo] = v.to_be_bytes();
        bus.idle(self.sp);
        self.sp = self.sp.wrapping_sub(1);
        bus.write(self.sp, hi);
        self.sp = self.sp.wrapping_sub(1);
        bus.write(self.sp, lo);
    }

    fn pop(&mut self, bus: &mut impl Bus) -> u16 {
        let lo = bus.read(self.sp);
        self.sp = self.sp.wrapping_add(1);
        let hi = bus.read(self.sp);
        self.sp = self.sp.wrapping_add(1);
        u16::from_le_bytes([lo, hi])
    }

    /// The eight arithmetic and logic operations on A, by their 3-bit encoding.
    fn alu(&mut self, op: u8, v: u8) {
        let a = self.a;
        match op {
            0 | 1 => {
                let carry = (op == 1 && self.flag(C)) as u8;
                let r = a as u16 + v as u16 + carry as u16;
                let h = (a & 0xf) + (v & 0xf) + carry > 0xf;
                self.a = r as u8;
                self.set_flags(self.a == 0, false, h, r > 0xff);
            }
            2 | 3 | 7 => {
                let carry = (op == 3 && self.flag(C)) as u8;
                let r = (a as i16) - (v as i16) - carry as i16;
                let h = ((a & 0xf) as i16) - ((v & 0xf) as i16) - (carry as i16) < 0;
                self.set_flags(r as u8 == 0, true, h, r < 0);
                if op != 7 {
                    self.a = r as u8;
                }
            }
            4 => {
                self.a = a & v;
                self.set_flags(self.a == 0, false, true, false);
            }
            5 => {
                self.a = a ^ v;
                self.set_flags(self.a == 0, false, false, false);
            }
            _ => {
                self.a = a | v;
                self.set_flags(self.a == 0, false, false, false);
            }
        }
    }

    /// The CB-prefixed rotates and shifts, by their 3-bit encoding: RLC RRC RL RR SLA SRA SWAP
    /// SRL. Sets all four flags.
    fn shift(&mut self, op: u8, v: u8) -> u8 {
        let carry_in = self.flag(C) as u8;
        let (r, c) = match op {
            0 => (v.rotate_left(1), v & 0x80 != 0),
            1 => (v.rotate_right(1), v & 1 != 0),
            2 => (v << 1 | carry_in, v & 0x80 != 0),
            3 => (v >> 1 | carry_in << 7, v & 1 != 0),
            4 => (v << 1, v & 0x80 != 0),
            5 => (v >> 1 | (v & 0x80), v & 1 != 0),
            6 => (v.rotate_left(4), false),
            _ => (v >> 1, v & 1 != 0),
        };
        self.set_flags(r == 0, false, false, c);
        r
    }

    /// SP plus a signed byte, as ADD SP,e and LD HL,SP+e compute it: the flags come from the
    /// unsigned addition of the low bytes.
    fn sp_plus(&mut self, e: u8) -> u16 {
        let sp = self.sp;
        let h = (sp & 0xf) + (e as u16 & 0xf) > 0xf;
        let c = (sp & 0xff) + e as u16 > 0xff;
        self.set_flags(false, false, h, c);
        sp.wrapping_add(e as i8 as u16)
    }

    fn execute(&mut self, op: u8, bus: &mut impl Bus) {
        let x = op >> 6;
        let y = (op >> 3) & 7;
        let z = op & 7;
        match op {
            0x00 => {}
            // LD r16, imm16
            0x01 | 0x11 | 0x21 | 0x31 => {
                let v = self.fetch16(bus);
                self.set_r16(y >> 1, v);
            }
            // LD [r16mem], A and LD A, [r16mem]
            0x02 | 0x12 | 0x22 | 0x32 | 0x0a | 0x1a | 0x2a | 0x3a => {
                let addr = match y >> 1 {
                    0 => self.bc(),
                    1 => self.de(),
                    _ => self.hl(),
                };
                match y >> 1 {
                    2 => self.set_hl(addr.wrapping_add(1)),
                    3 => self.set_hl(addr.wrapping_sub(1)),
                    _ => {}
                }
                if y & 1 == 0 {
                    bus.write(addr, self.a);
                } else {
                    self.a = bus.read(addr);
                }
            }
            // LD [imm16], SP
            0x08 => {
                let addr = self.fetch16(bus);
                let [hi, lo] = self.sp.to_be_bytes();
                bus.write(addr, lo);
                bus.write(addr.wrapping_add(1), hi);
            }
            // INC r16 and DEC r16
            0x03 | 0x13 | 0x23 | 0x33 | 0x0b | 0x1b | 0x2b | 0x3b => {
                let r = self.r16(y >> 1);
                bus.idle(r);
                let v = if y & 1 == 0 {
                    r.wrapping_add(1)
                } else {
                    r.wrapping_sub(1)
                };
                self.set_r16(y >> 1, v);
            }
            // ADD HL, r16
            0x09 | 0x19 | 0x29 | 0x39 => {
                let hl = self.hl();
                let v = self.r16(y >> 1);
                let r = hl as u32 + v as u32;
                let h = (hl & 0xfff) + (v & 0xfff) > 0xfff;
                self.set_flags(self.flag(Z), false, h, r > 0xffff);
                bus.idle(hl);
                self.set_hl(r as u16);
            }
            // INC r8 and DEC r8
            _ if x == 0 && (z == 4 || z == 5) => {
                let v = self.r8(y, bus);
                let (r, h) = if z == 4 {
                    (v.wrapping_add(1), v & 0xf == 0xf)
                } else {
                    (v.wrapping_sub(1), v & 0xf == 0)
                };
                self.set_flags(r == 0, z == 5, h, self.flag(C));
                self.set_r8(y, r, bus);
            }
            // LD r8, imm8
            _ if x == 0 && z == 6 => {
                let v = self.fetch(bus);
                self.set_r8(y, v, bus);
            }
            0x07 | 0x0f | 0x17 | 0x1f => {
                let r = self.shift(y, self.a);
                self.a = r;
                self.f &= !Z;
            }
            // DAA: adjusts A to binary-coded decimal after an addition or subtraction.
            0x27 => {
                let mut a = self.a;
                let mut c = self.flag(C);
                if !self.flag(N) {
                    if c || a > 0x99 {
                        a = a.wrapping_add(0x60);
                        c = true;
                    }
                    if self.flag(H) || a & 0xf > 9 {
                        a = a.wrapping_add(6);
                    }
                } else {
                    if c {
                        a = a.wrapping_sub(0x60);
                    }
                    if self.flag(H) {
                        a = a.wrapping_sub(6);
                    }
                }
                self.a = a;
                self.set_flags(a == 0, self.flag(N), false, c);
            }
            0x2f => {
                self.a = !self.a;
                self.f |= N | H;
            }
            0x37 => self.f = (self.f & Z) | C,
            0x3f => self.f = (self.f & (Z | C)) ^ C,
            // JR imm8 and JR cond, imm8
            0x18 | 0x20 | 0x28 | 0x30 | 0x38 => {
                let e = self.fetch(bus) as i8;
                if op == 0x18 || self.cond(y - 4) {
                    bus.idle(self.pc);
                    self.pc = self.pc.wrapping_add(e as u16);
                }
            }
            0x10 => {
                if bus.stop() {
                    self.mode = Mode::Stopped;
                }
            }
            0x76 => {
                if self.ime || bus.interrupts() == 0 {
                    self.mode = Mode::Halted;
                } else {
                    self.halt_bug = true;
                }
            }
            // LD r8, r8
            0x40..=0x7f => {
                let v = self.r8(z, bus);
                self.set_r8(y, v, bus);
            }
            // ALU A, r8
            0x80..=0xbf => {
                let v = self.r8(z, bus);
                self.alu(y, v);
            }
            // ALU A, imm8
            _ if x == 3 && z == 6 => {
                let v = self.fetch(bus);
                self.alu(y, v);
            }
            // RET cond
            0xc0 | 0xc8 | 0xd0 | 0xd8 => {
                bus.idle(self.sp);
                if self.cond(y) {
                    self.pc = self.pop(bus);
                    bus.idle(self.pc);
                }
            }
            // RET and RETI
            0xc9 | 0xd9 => {
                self.pc = self.pop(bus);
                bus.idle(self.pc);
                if op == 0xd9 {
                    self.ime = true;
                }
            }
            // JP cond, imm16 and JP imm16
            0xc2 | 0xca | 0xd2 | 0xda | 0xc3 => {
                let addr = self.fetch16(bus);
                if op == 0xc3 || self.cond(y) {
                    bus.idle(addr);
                    self.pc = addr;
                }
            }
            0xe9 => self.pc = self.hl(),
            // CALL cond, imm16 and CALL imm16
            0xc4 | 0xcc | 0xd4 | 0xdc | 0xcd => {
                let addr = self.fetch16(bus);
                if op == 0xcd || self.cond(y) {
                    self.push(self.pc, bus);
                    self.pc = addr;
                }
            }
            // RST
            _ if x == 3 && z == 7 => {
                self.push(self.pc, bus);
                self.pc = (y as u16) * 8;
            }
            // POP r16stk
            0xc1 | 0xd1 | 0xe1 | 0xf1 => {
                let v = self.pop(bus);
                match y >> 1 {
                    0 => self.set_bc(v),
                    1 => self.set_de(v),
                    2 => self.set_hl(v),
                    _ => self.set_af(v),
                }
            }
            // PUSH r16stk
            0xc5 | 0xd5 | 0xe5 | 0xf5 => {
                let v = match y >> 1 {
                    0 => self.bc(),
                    1 => self.de(),
                    2 => self.hl(),
                    _ => self.af(),
                };
                self.push(v, bus);
            }
            0xcb => {
                let op = self.fetch(bus);
                let (kind, bit, reg) = (op >> 6, (op >> 3) & 7, op & 7);
                let v = self.r8(reg, bus);
                match kind {
                    0 => {
                        let r = self.shift(bit, v);
                        self.set_r8(reg, r, bus);
                    }
                    1 => {
                        let z = v & (1 << bit) == 0;
                        self.f = (self.f & C) | H | if z { Z } else { 0 };
                    }
                    2 => self.set_r8(reg, v & !(1 << bit), bus),
                    _ => self.set_r8(reg, v | (1 << bit), bus),
                }
            }
            0xe0 => {
                let n = self.fetch(bus);
                bus.write(0xff00 | n as u16, self.a);
            }
            0xf0 => {
                let n = self.fetch(bus);
                self.a = bus.read(0xff00 | n as u16);
            }
            0xe2 => bus.write(0xff00 | self.c as u16, self.a),
            0xf2 => self.a = bus.read(0xff00 | self.c as u16),
            0xea => {
                let addr = self.fetch16(bus);
                bus.write(addr, self.a);
            }
            0xfa => {
                let addr = self.fetch16(bus);
                self.a = bus.read(addr);
            }
            // ADD SP, imm8
            0xe8 => {
                let e = self.fetch(bus);
                let r = self.sp_plus(e);
                bus.idle(self.sp);
                bus.idle(self.sp);
                self.sp = r;
            }
            // LD HL, SP + imm8
            0xf8 => {
                let e = self.fetch(bus);
                let r = self.sp_plus(e);
                bus.idle(self.sp);
                self.set_hl(r);
            }
            0xf9 => {
                bus.idle(self.hl());
                self.sp = self.hl();
            }
            0xf3 => {
                self.ime = false;
                self.ei_pending = false;
            }
            0xfb => self.ei_pending = true,
            // $D3 $DB $DD $E3 $E4 $EB $EC $ED $F4 $FC $FD: no instruction; the CPU locks up.
            _ => self.mode = Mode::Locked,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::{vec, vec::Vec};

    /// 64 KiB of flat memory that counts cycles.
    struct Flat {
        mem: Vec<u8>,
        cycles: u32,
        pending: u8,
    }

    impl Flat {
        fn new(program: &[u8]) -> Flat {
            let mut mem = vec![0; 0x10000];
            mem[0x100..0x100 + program.len()].copy_from_slice(program);
            Flat {
                mem,
                cycles: 0,
                pending: 0,
            }
        }
    }

    impl Bus for Flat {
        fn read(&mut self, addr: u16) -> u8 {
            self.cycles += 1;
            self.mem[addr as usize]
        }
        fn write(&mut self, addr: u16, value: u8) {
            self.cycles += 1;
            self.mem[addr as usize] = value;
        }
        fn idle(&mut self, _: u16) {
            self.cycles += 1;
        }
        fn interrupts(&self) -> u8 {
            self.pending
        }
        fn acknowledge(&mut self, bit: u8) {
            self.pending &= !(1 << bit);
        }
    }

    fn cpu() -> Cpu {
        Cpu {
            pc: 0x100,
            sp: 0xfffe,
            ..Cpu::default()
        }
    }

    #[test]
    fn ei_takes_effect_after_the_next_instruction() {
        // EI, NOP, NOP with an interrupt pending: it is taken after the first NOP.
        let mut bus = Flat::new(&[0xfb, 0x00, 0x00]);
        bus.pending = 1;
        let mut cpu = cpu();
        cpu.step(&mut bus);
        cpu.step(&mut bus);
        assert_eq!(cpu.pc, 0x102);
        cpu.step(&mut bus);
        assert_eq!(cpu.pc, 0x40);
        assert!(!cpu.ime);
    }

    #[test]
    fn di_right_after_ei_allows_no_interrupt() {
        let mut bus = Flat::new(&[0xfb, 0xf3, 0x00]);
        bus.pending = 1;
        let mut cpu = cpu();
        for _ in 0..3 {
            cpu.step(&mut bus);
        }
        assert_eq!(cpu.pc, 0x103);
    }

    #[test]
    fn interrupt_dispatch_takes_five_cycles() {
        let mut bus = Flat::new(&[0x00]);
        bus.pending = 0b100;
        let mut cpu = Cpu { ime: true, ..cpu() };
        cpu.step(&mut bus);
        assert_eq!((cpu.pc, bus.cycles), (0x50, 5));
        assert_eq!(&bus.mem[0xfffc..0xfffe], &[0x00, 0x01]);
    }

    #[test]
    fn halt_bug_reads_the_next_byte_twice() {
        // HALT with IME=0 and an interrupt pending, then INC A: A is incremented twice.
        let mut bus = Flat::new(&[0x76, 0x3c, 0x00]);
        bus.pending = 1;
        let mut cpu = cpu();
        for _ in 0..3 {
            cpu.step(&mut bus);
        }
        assert_eq!((cpu.a, cpu.pc), (2, 0x102));
    }

    #[test]
    fn halt_waits_for_an_interrupt() {
        let mut bus = Flat::new(&[0x76, 0x3c]);
        let mut cpu = cpu();
        cpu.step(&mut bus);
        assert_eq!(cpu.mode, Mode::Halted);
        cpu.step(&mut bus);
        assert_eq!(cpu.mode, Mode::Halted);
        bus.pending = 1;
        cpu.step(&mut bus);
        cpu.step(&mut bus);
        assert_eq!((cpu.mode, cpu.a), (Mode::Running, 1));
    }

    #[test]
    fn invalid_opcode_locks() {
        let mut bus = Flat::new(&[0xd3, 0x3c]);
        let mut cpu = cpu();
        cpu.step(&mut bus);
        cpu.step(&mut bus);
        assert_eq!((cpu.mode, cpu.a), (Mode::Locked, 0));
    }
}
