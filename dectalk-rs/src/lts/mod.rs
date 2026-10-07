//! LTS thread (FUN_1c00fee0): text words -> dictionary / letter-to-sound -> PH symbol stream.
//!
//! Transliterated against the flat `Mem` model. Every original function FUN_1c0XXXXX is
//! `f_1c0XXXXX(m, p1..pN) -> u32` with raw 32-bit cdecl arguments and EAX as the return value.
//! ctx = the LTS thread's argument; kernel = ctx+8; lts state (0x112c bytes) = ctx+0x10.

pub mod dict;
pub mod engine;
pub mod io;
pub mod parse1;
pub mod parse2;
pub mod pre;
pub mod spell;

pub use crate::ph::mem::Mem;
pub use dict::*;
pub use io::*;
pub use parse1::*;
pub use parse2::*;
pub use pre::*;
pub use spell::*;

pub const LTS_SIZE: usize = 0x112c;

/// FUN_1c006700's in-memory dictionary block at `table - 4`: [u32 size][count x u32 absolute
/// entry pointers][entry blob]. Returns (table address, entry count).
pub fn map_dict(m: &mut Mem, name: &'static str, file: &[u8], table: u32) -> (u32, u32) {
    let rd = |o: usize| u32::from_le_bytes(file[o..o + 4].try_into().unwrap());
    let (count, blob_len) = (rd(0), rd(4));
    let size = blob_len + 4 + count * 4;
    let blob = table + count * 4;
    let mut block = Vec::with_capacity(size as usize);
    block.extend_from_slice(&size.to_le_bytes());
    for k in 0..count as usize {
        block.extend_from_slice(&(blob + rd(8 + k * 4)).to_le_bytes());
    }
    let start = 8 + count as usize * 4;
    block.extend_from_slice(&file[start..start + blob_len as usize]);
    m.map(name, table - 4, &block);
    (table, count)
}

/// Sign-extend the low 16 bits (a `movsx` of a word).
#[inline]
pub fn sx16(v: u32) -> u32 {
    v as u16 as i16 as i32 as u32
}

/// Sign-extend the low 8 bits.
#[inline]
pub fn sx8(v: u32) -> u32 {
    v as u8 as i8 as i32 as u32
}
