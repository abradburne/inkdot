//! The timer: DIV, TIMA, TMA, TAC.
//!
//! DIV is the top byte of a 16-bit system counter that counts T-cycles (4 per M-cycle, at the
//! CPU's speed). TIMA counts falling edges of one counter bit selected by TAC, ANDed with TAC's
//! enable bit, so anything that drops that signal (resetting the counter, changing TAC) can
//! tick TIMA early. An overflow leaves TIMA at 0 for one M-cycle before TMA is loaded and the
//! interrupt requested.
//!
//! Sources: Pan Docs "Timer and Divider Registers", "Timer obscure behaviour"; Mooneye's
//! `acceptance/timer` tests.

/// The timer's state.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Timer {
    /// The system counter, in T-cycles. DIV is bits 8-15.
    pub counter: u16,
    tima: u8,
    tma: u8,
    tac: u8,
    /// TIMA overflowed in the last M-cycle: TMA is loaded and the interrupt requested in this
    /// one.
    reload_next: bool,
    /// This M-cycle is the one in which TMA was loaded: writes to TIMA are lost, writes to TMA
    /// go to TIMA as well.
    reloading: bool,
}

impl Timer {
    /// The counter bit TAC selects, as a mask.
    fn bit(&self) -> u16 {
        match self.tac & 3 {
            0 => 1 << 9,
            1 => 1 << 3,
            2 => 1 << 5,
            _ => 1 << 7,
        }
    }

    fn signal(&self) -> bool {
        self.tac & 4 != 0 && self.counter & self.bit() != 0
    }

    fn increment(&mut self) {
        let (v, overflow) = self.tima.overflowing_add(1);
        self.tima = v;
        self.reload_next |= overflow;
    }

    /// One M-cycle. Returns whether the timer interrupt is requested.
    pub fn tick(&mut self) -> bool {
        self.reloading = false;
        let mut interrupt = false;
        if self.reload_next {
            self.reload_next = false;
            self.tima = self.tma;
            self.reloading = true;
            interrupt = true;
        }
        let before = self.signal();
        self.counter = self.counter.wrapping_add(4);
        if before && !self.signal() {
            self.increment();
        }
        interrupt
    }

    /// Reads $FF04-$FF07.
    pub fn read(&self, addr: u16) -> u8 {
        match addr {
            0xff04 => (self.counter >> 8) as u8,
            0xff05 => self.tima,
            0xff06 => self.tma,
            _ => self.tac | 0xf8,
        }
    }

    /// Writes $FF04-$FF07.
    pub fn write(&mut self, addr: u16, v: u8) {
        match addr {
            0xff04 => {
                let before = self.signal();
                self.counter = 0;
                if before {
                    self.increment();
                }
            }
            0xff05 => {
                if !self.reloading {
                    self.tima = v;
                    // A write in the cycle after an overflow cancels the reload.
                    self.reload_next = false;
                }
            }
            0xff06 => {
                self.tma = v;
                if self.reloading {
                    self.tima = v;
                }
            }
            _ => {
                let before = self.signal();
                self.tac = v & 7;
                if before && !self.signal() {
                    self.increment();
                }
            }
        }
    }

    /// Resets the system counter, as STOP and the CGB speed switch do.
    pub fn reset_counter(&mut self) {
        self.counter = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tima_counts_at_the_selected_rate() {
        let mut t = Timer::default();
        t.write(0xff07, 0b101); // every 4 M-cycles
        for _ in 0..16 {
            t.tick();
        }
        assert_eq!(t.read(0xff05), 4);
    }

    #[test]
    fn overflow_reloads_one_cycle_late() {
        let mut t = Timer::default();
        t.write(0xff06, 0x23);
        t.write(0xff05, 0xff);
        t.write(0xff07, 0b101);
        let mut cycles = 0;
        while t.read(0xff05) == 0xff {
            assert!(!t.tick());
            cycles += 1;
        }
        assert_eq!((cycles, t.read(0xff05)), (4, 0));
        assert!(t.tick());
        assert_eq!(t.read(0xff05), 0x23);
    }

    #[test]
    fn writing_div_can_tick_tima() {
        let mut t = Timer::default();
        t.write(0xff07, 0b101);
        t.tick();
        t.tick(); // counter = 8: bit 3 set
        t.write(0xff04, 0);
        assert_eq!(t.read(0xff05), 1);
    }
}
