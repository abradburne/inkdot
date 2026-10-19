//! inkdot-core: the Game Boy and Game Boy Color, as a machine with no I/O.
//!
//! The core is given a ROM and button states and produces frames, sound and save data. It has
//! no dependencies and no platform code, so the DS+ frontend, the headless test runner and any
//! later port all run the same machine. [`docs/design.md`](../../docs/design.md) explains the
//! design; [`docs/provenance.md`](../../docs/provenance.md) lists the documents it follows.
#![no_std]

extern crate alloc;

pub mod apu;
pub mod cart;
pub mod compat;
pub mod cpu;
mod gameboy;
pub mod hw;
pub mod ppu;
pub mod serial;
pub mod state;
pub mod synth;
pub mod timer;

pub use cart::{CartError, Header};
pub use gameboy::{GameBoy, Model};
pub use state::StateError;
