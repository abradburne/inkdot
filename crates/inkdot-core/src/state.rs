//! Save states: the whole machine as bytes, and back.
//!
//! Each part of the machine has one `visit` function that names each of its fields in order;
//! the same function saves (with a [`Writer`]) and loads (with a [`Reader`]), so the two can't
//! drift apart. A state is a header, then one section per part:
//!
//! ```text
//! "INKDOT\0\0"  u16 version  u8 cgb  u8 cgb_mode  u16 global checksum  [u8; 16] title
//! sections: [u8; 4] tag, u32 length, payload...   ("CPU ", "BUS ", "TIMR", "SERL",
//!                                                  "PPU ", "APU ", "CART")
//! ```
//!
//! All numbers are little-endian. A section with a tag this build doesn't know is skipped, a
//! missing one is an error; a change in what a section's fields mean bumps [`VERSION`], and
//! loading an older version needs a converter (there is only version 1 so far).

use alloc::vec::Vec;

/// The current format's version.
pub const VERSION: u16 = 1;
const MAGIC: &[u8; 8] = b"INKDOT\0\0";

/// Why a state can't be loaded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StateError {
    /// Not a state at all, or cut short.
    Corrupt,
    /// A state from a newer format than this build reads.
    NewerVersion(u16),
    /// The state is of another game, or of the same game on the other model.
    OtherGame,
    /// A section the machine needs is missing.
    MissingSection([u8; 4]),
}

impl core::fmt::Display for StateError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            StateError::Corrupt => write!(f, "not a save state, or a damaged one"),
            StateError::NewerVersion(v) => write!(f, "made by a newer version (format {v})"),
            StateError::OtherGame => write!(f, "a save state of another game or console"),
            StateError::MissingSection(t) => {
                write!(
                    f,
                    "save state lacks its {} part",
                    core::str::from_utf8(t).unwrap_or("?")
                )
            }
        }
    }
}

/// Saves or loads fields, in the order a `visit` function names them.
pub(crate) trait Visitor {
    fn bytes(&mut self, x: &mut [u8]);

    fn u8(&mut self, x: &mut u8) {
        self.bytes(core::slice::from_mut(x));
    }

    fn u16(&mut self, x: &mut u16) {
        let mut b = x.to_le_bytes();
        self.bytes(&mut b);
        *x = u16::from_le_bytes(b);
    }

    fn u32(&mut self, x: &mut u32) {
        let mut b = x.to_le_bytes();
        self.bytes(&mut b);
        *x = u32::from_le_bytes(b);
    }

    fn u64(&mut self, x: &mut u64) {
        let mut b = x.to_le_bytes();
        self.bytes(&mut b);
        *x = u64::from_le_bytes(b);
    }

    fn bool(&mut self, x: &mut bool) {
        let mut b = *x as u8;
        self.u8(&mut b);
        *x = b != 0;
    }

    fn i32(&mut self, x: &mut i32) {
        let mut u = *x as u32;
        self.u32(&mut u);
        *x = u as i32;
    }

    fn u16s(&mut self, x: &mut [u16]) {
        for v in x {
            self.u16(v);
        }
    }

    fn opt_u8(&mut self, x: &mut Option<u8>) {
        let mut some = x.is_some();
        let mut v = x.unwrap_or(0);
        self.bool(&mut some);
        self.u8(&mut v);
        *x = some.then_some(v);
    }

    fn opt_i32(&mut self, x: &mut Option<i32>) {
        let mut some = x.is_some();
        let mut v = x.unwrap_or(0);
        self.bool(&mut some);
        self.i32(&mut v);
        *x = some.then_some(v);
    }
}

/// Appends fields to a buffer.
pub(crate) struct Writer(pub Vec<u8>);

impl Visitor for Writer {
    fn bytes(&mut self, x: &mut [u8]) {
        self.0.extend_from_slice(x);
    }
}

/// Reads fields from a buffer; running out sets `short` and reads zeros.
pub(crate) struct Reader<'a> {
    data: &'a [u8],
    pub short: bool,
}

impl<'a> Reader<'a> {
    pub fn new(data: &'a [u8]) -> Reader<'a> {
        Reader { data, short: false }
    }
}

impl Visitor for Reader<'_> {
    fn bytes(&mut self, x: &mut [u8]) {
        if self.data.len() < x.len() {
            self.short = true;
            x.fill(0);
            self.data = &[];
            return;
        }
        let (head, rest) = self.data.split_at(x.len());
        x.copy_from_slice(head);
        self.data = rest;
    }
}

/// What identifies the game and console a state belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Identity {
    pub cgb: bool,
    pub cgb_mode: bool,
    pub checksum: u16,
    pub title: [u8; 16],
}

/// Whether a state was made on a CGB (`None`: not a state).
pub fn made_on_cgb(data: &[u8]) -> Option<bool> {
    (data.len() > 11 && &data[..8] == MAGIC).then(|| data[10] != 0)
}

/// Builds a state from its identity and sections.
pub(crate) fn encode(id: Identity, sections: &[(&[u8; 4], Vec<u8>)]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&VERSION.to_le_bytes());
    out.push(id.cgb as u8);
    out.push(id.cgb_mode as u8);
    out.extend_from_slice(&id.checksum.to_le_bytes());
    out.extend_from_slice(&id.title);
    for (tag, payload) in sections {
        out.extend_from_slice(*tag);
        out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        out.extend_from_slice(payload);
    }
    out
}

/// A section's tag and payload.
pub(crate) type Section<'a> = ([u8; 4], &'a [u8]);

/// Splits a state into its sections, after checking that it belongs to `id`.
pub(crate) fn decode(data: &[u8], id: Identity) -> Result<Vec<Section<'_>>, StateError> {
    const HEADER: usize = 8 + 2 + 1 + 1 + 2 + 16;
    if data.len() < HEADER || &data[..8] != MAGIC {
        return Err(StateError::Corrupt);
    }
    let version = u16::from_le_bytes([data[8], data[9]]);
    if version > VERSION {
        return Err(StateError::NewerVersion(version));
    }
    let found = Identity {
        cgb: data[10] != 0,
        cgb_mode: data[11] != 0,
        checksum: u16::from_le_bytes([data[12], data[13]]),
        title: data[14..30].try_into().map_err(|_| StateError::Corrupt)?,
    };
    if found != id {
        return Err(StateError::OtherGame);
    }
    let mut sections = Vec::new();
    let mut rest = &data[HEADER..];
    while !rest.is_empty() {
        if rest.len() < 8 {
            return Err(StateError::Corrupt);
        }
        let tag: [u8; 4] = rest[..4].try_into().map_err(|_| StateError::Corrupt)?;
        let len = u32::from_le_bytes([rest[4], rest[5], rest[6], rest[7]]) as usize;
        if rest.len() < 8 + len {
            return Err(StateError::Corrupt);
        }
        sections.push((tag, &rest[8..8 + len]));
        rest = &rest[8 + len..];
    }
    Ok(sections)
}

/// Adds a section of the frontend's own to a state made by `GameBoy::save_state`: the core
/// skips a tag it doesn't know when loading, so the state still loads, here and in a build
/// that has never heard of the section. `tag` must not be one of the machine's.
pub fn add_section(state: &mut Vec<u8>, tag: &[u8; 4], payload: &[u8]) {
    state.extend_from_slice(tag);
    state.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    state.extend_from_slice(payload);
}

/// The payload of the first section tagged `tag` in a state, whatever game it is of; `None`
/// when the state has no such section, or isn't a state.
pub fn section<'a>(state: &'a [u8], tag: &[u8; 4]) -> Option<&'a [u8]> {
    const HEADER: usize = 8 + 2 + 1 + 1 + 2 + 16;
    if state.len() < HEADER || &state[..8] != MAGIC {
        return None;
    }
    let mut rest = &state[HEADER..];
    while rest.len() >= 8 {
        let len = u32::from_le_bytes([rest[4], rest[5], rest[6], rest[7]]) as usize;
        let payload = rest.get(8..8 + len)?;
        if &rest[..4] == tag {
            return Some(payload);
        }
        rest = &rest[8 + len..];
    }
    None
}
