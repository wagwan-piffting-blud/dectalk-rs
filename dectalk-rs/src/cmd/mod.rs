//! CMD thread (FUN_1c003460): raw text bytes from the API -> `[:commands]`, text normalization
//! and parsing -> the CMD->LTS word stream (plus tone messages straight to the VTM).
//!
//! Transliterated against the flat `Mem` model. Every original function FUN_1c0XXXXX is
//! `f_1c0XXXXX(m, p1..pN) -> u32` with raw 32-bit cdecl arguments and EAX as the return value.
//! ctx = the CMD thread's argument; kernel = ctx+8; cmd state (0x3244 bytes) = ctx+0xc.

pub mod command;
pub mod engine;
pub mod handlers;
pub mod main;
pub mod parse_a;
pub mod parse_b;
pub mod parse_c;
pub mod parse_d;
pub mod text;

pub use crate::lts::{sx16, sx8};
pub use crate::ph::mem::Mem;
pub use command::*;
pub use handlers::*;
pub use main::*;
pub use parse_a::*;
pub use parse_b::*;
pub use parse_c::*;
pub use parse_d::*;
pub use text::*;

pub const CMD_SIZE: usize = 0x3244;
