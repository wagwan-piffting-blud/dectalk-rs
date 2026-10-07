//! Voice definitions: 34-word speaker records at ph+0x1446 (current), ph+0x14dc (user voice),
//! and the VTM speaker message derived from them.

use super::mem::Mem;
use super::{kernel_of, msg_alloc, msg_send, ph_of, wrap};

pub const VOICE_WORDS: u32 = 0x22;
const DV_LIMITS: u32 = 0x1c06_a70c;

const LOCUS_MALE: u32 = 0x1c06_8cd4;
const LISTS_MALE: u32 = 0x1c06_7b10;
const TARGETS_MALE: u32 = 0x1c06_74d0;
const LOCUS_FEMALE: u32 = 0x1c06_9308;
const LISTS_FEMALE: u32 = 0x1c06_7e94;
const TARGETS_FEMALE: u32 = 0x1c06_77f0;
const CONTEXT_TARGETS: u32 = 0x1c06_8294;

/// FUN_1c016910: clear phoneme arrays and bind the clause working-array pointers.
pub fn init_arrays(m: &mut Mem, ph: u32) {
    for k in 0..0x136u32 {
        m.w16(ph + 0x302 + k * 2 + 0x1234, 0);
        m.w16(ph + 0x302 + k * 2, 0);
        m.w32(ph + 0x570 + k * 4, 0);
        m.w16(ph + 0x302 + k * 2 + 0x746, 0);
        m.w16(ph + 0x302 + k * 2 + 0x1a34, 0);
    }
    m.w32(ph + 0x1434, ph + 0x312);
    m.w32(ph + 0x1438, ph + 0x590);
    m.w32(ph + 0x1428, ph + 0xa58);
    m.w16(ph + 0x2ac, 0);
    m.w16(ph + 0x2ae, 0);
    m.w32(ph + 0x142c, ph + 0x1546);
    m.w32(ph + 0x1430, ph + 0x1d46);
}

/// FUN_1c0204b0: load built-in voice `n` (tables differ below ~8.7 kHz).
pub fn select_voice(m: &mut Mem, ctx: u32, n: i32) {
    let kernel = kernel_of(m, ctx);
    let ph = ph_of(m, ctx);
    let slot = if m.r32(kernel + 0x22c) < 0x223b { 0x14b4 } else { 0x148c };
    let src = m.r32(ph + slot + (n as u32) * 4);
    for k in 0..VOICE_WORDS {
        m.w16(ph + 0x1446 + k * 2, m.s(src + k * 2));
    }
    m.w16(ph + 0x1488, n);
    m.w8(ph + 0x1520, 1);
    m.w16(kernel + 0x220, n);
}

/// FUN_1c020470: set one voice parameter, clamped to its legal range.
pub fn set_voice_param(m: &mut Mem, ph: u32, idx: i32, val: i32) {
    if !(0..0x20).contains(&idx) {
        return;
    }
    let lo = m.s(DV_LIMITS + idx as u32 * 4);
    let hi = m.s(DV_LIMITS + 2 + idx as u32 * 4);
    let v = if val < lo {
        lo
    } else if hi < val {
        hi
    } else {
        val
    };
    m.w16(ph + 0x1446 + idx as u32 * 2, v);
    m.w8(ph + 0x1520, 1);
}

/// FUN_1c020520: save the current voice as the user-defined voice.
pub fn save_user_voice(m: &mut Mem, ph: u32) {
    for k in 0..VOICE_WORDS {
        m.w16(ph + 0x14dc + k * 2, m.s(ph + 0x1446 + k * 2));
    }
}

/// FUN_1c020540: derive PH voice constants and send the VTM speaker message.
pub fn apply_voice(m: &mut Mem, ctx: u32) {
    let kernel = kernel_of(m, ctx);
    let ph = ph_of(m, ctx);
    let msg = msg_alloc(m, 1);
    let v = |m: &Mem, o: u32| m.s(ph + o);
    m.w16(ph + 0x11b0, v(m, 0x1446));
    if v(m, 0x1446) == 1 {
        m.w32(ph + 0x11b4, LOCUS_MALE);
        m.w32(ph + 0x11b8, LISTS_MALE);
        m.w32(ph + 0x11bc, TARGETS_MALE);
    } else {
        m.w32(ph + 0x11b4, LOCUS_FEMALE);
        m.w32(ph + 0x11b8, LISTS_FEMALE);
        m.w32(ph + 0x11bc, TARGETS_FEMALE);
    }
    m.w32(ph + 0x11c0, CONTEXT_TARGETS);
    m.w16(ph + 0x2be, v(m, 0x1478));
    m.w16(ph + 0x1522, v(m, 0x144a) * 0x29);
    m.w16(ph + 0x1524, (v(m, 0x147e) + 100) * 0xf);
    m.w16(ph + 0x1526, v(m, 0x1480) * 10);
    m.w16(ph + 0x1528, v(m, 0x1482));
    m.w16(ph + 0x152c, v(m, 0x144c) * 10);
    m.w16(ph + 0x152e, v(m, 0x144e) * 0x29);
    m.w16(ph + 0x2ba, v(m, 0x147c) * 0x29);
    m.w16(ph + 0x152a, v(m, 0x147a) * 10);
    m.w16(ph + 0x2bc, (v(m, 0x1448) * 0x19) / 100);
    let mut sm = v(m, 0x1484);
    m.w16(ph + 0x11a6, sm);
    let br = v(m, 0x1450);
    m.w16(ph + 0x2b8, wrap((br * br) >> 1) + 0x1000);
    if sm != 0 {
        sm = if sm < 0x60 { (0x5f - sm) * 0x1e } else { sm - 0x5f };
        m.w16(ph + 0x11a6, sm);
    }

    let w = |m: &mut Mem, i: u32, x: i32| m.w16(msg + i * 2, x);
    let scale = wrap((200 - v(m, 0x1458)) * 0x29);
    w(m, 0xf, scale);
    let f4 = v(m, 0x145a);
    w(m, 0, if f4 == 6000 { 6000 } else { (f4 * scale) >> 12 });
    w(m, 1, v(m, 0x145c));
    if 0x1356 < m.s(msg) {
        w(m, 0, 6000);
        w(m, 1, 6000);
    }
    let f5 = v(m, 0x145e);
    let s15 = m.s(msg + 0x1e);
    w(m, 2, if f5 == 6000 { 6000 } else { (f5 * s15) >> 12 });
    w(m, 3, v(m, 0x1460));
    if 0x1356 < m.s(msg + 4) {
        w(m, 2, 6000);
        w(m, 3, 6000);
    }
    for (dst, src) in [(4, 0x1462), (5, 0x1464), (6, 0x1456), (7, 0x146e), (8, 0x1470), (9, 0x1472), (10, 0x1474), (11, 0x1476)] {
        let x = v(m, src);
        w(m, dst, x);
    }
    w(m, 0xc, (0x7d - v(m, 0x1452)) * 0xa0);
    w(m, 0xd, v(m, 0x1454) << 2);
    w(m, 0xe, v(m, 0x1450) + 9);
    for (dst, src) in [(0x10, 0x1466), (0x11, 0x146c), (0x12, 0x146a), (0x13, 0x1468), (0x15, 0x1486), (0x16, 0x1488)] {
        let x = v(m, src);
        w(m, dst, x);
    }
    msg_send(m, kernel, msg);
}
