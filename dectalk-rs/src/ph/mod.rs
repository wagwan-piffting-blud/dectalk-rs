//! Phonetic stage: clause symbols -> VTM parameter frames.
//!
//! Transliterated from dectalk.dll 4.60 over the flat `Mem` model. `ctx` is the per-handle
//! context (0xe8 bytes); `kernel = ctx+8`, PH state `ph = ctx+0x18`.

pub mod allophone;
pub mod duration;
pub mod engine;
pub mod f0;
pub mod intonation;
pub mod mem;
pub mod rules;
pub mod speaker;
pub mod targets;

use mem::Mem;

pub const CTX_SIZE: usize = 0xe8;
pub const KERNEL_SIZE: usize = 0x488;
pub const PH_SIZE: usize = 0x284c;
pub const AUX_SIZE: usize = 0xc4;

const DAT_1C0699EC: u32 = 0x1c06_99ec;

#[inline]
pub fn kernel_of(m: &Mem, ctx: u32) -> u32 {
    m.r32(ctx + 8)
}

#[inline]
pub fn ph_of(m: &Mem, ctx: u32) -> u32 {
    m.r32(ctx + 0x18)
}

/// FUN_1c025200: message size in words for a VTM message kind.
fn msg_words(kind: u8) -> i32 {
    match kind {
        0 => 0x15,
        1 => 0x19,
        7 => 3,
        8 => 1,
        _ => 0,
    }
}

/// FUN_1c0251c0: allocate a VTM message; returns pointer past the kind word.
pub(crate) fn msg_alloc(m: &mut Mem, kind: u16) -> u32 {
    let n = msg_words(kind as u8);
    if n > 0 {
        let p = m.malloc((n * 2) as u32);
        // The original leaves the buffer uninitialized; zero it so unused words are deterministic.
        for k in 0..n as u32 {
            m.w16(p + k * 2, 0);
        }
        m.w16(p, kind as i32);
        return p + 2;
    }
    2
}

/// FUN_1c025260: send and free a message allocated by `msg_alloc`.
pub(crate) fn msg_send(m: &mut Mem, kernel: u32, body: u32) -> i32 {
    let base = body.wrapping_sub(2);
    if base == 0 {
        return 0;
    }
    let n = msg_words(m.r16(base) as u8);
    if n > 0 {
        let pipe = m.r32(kernel + 0xdc);
        m.pipe_write(pipe, base, n as u32);
    }
    m.free(base);
    n
}

// Index-mark queue: singly linked nodes at kernel+0x17c. Node dwords: [0] next, [6] command,
// [7] [8] args, [9] clause input position, [10] phoneme position, [11] insert adjustment.

fn index_nodes(m: &Mem, kernel: u32) -> Vec<u32> {
    let mut v = Vec::new();
    let mut n = m.r32(kernel + 0x17c);
    while n != 0 {
        v.push(n);
        n = m.r32(n);
    }
    v
}

/// FUN_1c024c10: send queued index marks whose phoneme position is <= `pos` to the VTM pipe.
pub fn index_flush(m: &mut Mem, kernel: u32, pos: u32) {
    let mut kind: i32 = 0;
    loop {
        let node = m.r32(kernel + 0x17c);
        if node == 0 || m.r32(node + 0x28) > pos {
            break;
        }
        kind = match m.r32(node + 0x18) {
            0x1f08 | 0x1f09 => 7,
            0x1f13 => 0x107,
            0x1f14 => 0x207,
            0x1f15 => 0x307,
            0x1f16 => 0x407,
            _ => kind,
        };
        let words = [kind as u16, m.r32(node + 0x1c) as u16, m.r32(node + 0x20) as u16];
        m.w32(kernel + 0x17c, m.r32(node));
        m.free(node);
        let pipe = m.r32(kernel + 0xdc);
        m.pipe_write_words(pipe, &words);
    }
}

/// FUN_1c024d10
pub fn index_adjust(m: &mut Mem, kernel: u32, pos: i32, d_clause: i32, d_adj: i32) {
    for node in index_nodes(m, kernel) {
        if m.r32(node + 0x2c).wrapping_add(pos as u32) <= m.r32(node + 0x28) {
            m.w32(node + 0x24, m.r32(node + 0x24).wrapping_add(d_clause as u32));
            m.w32(node + 0x2c, m.r32(node + 0x2c).wrapping_add(d_adj as u32));
        }
    }
}

/// FUN_1c024d70
pub fn index_shift(m: &mut Mem, kernel: u32, pos: i32, delta: i32) {
    for node in index_nodes(m, kernel) {
        if (pos as u32) <= m.r32(node + 0x28) {
            m.w32(node + 0x28, m.r32(node + 0x28).wrapping_add(delta as u32));
        }
    }
}

/// FUN_1c024dc0
pub fn index_place(m: &mut Mem, kernel: u32, clause_pos: i32, phone_pos: i32) {
    for node in index_nodes(m, kernel) {
        if m.r32(node + 0x24) == clause_pos as u32 {
            m.w32(node + 0x28, phone_pos as u32);
        }
    }
}

/// FUN_1c0168d0
pub fn clause_init(m: &mut Mem, ph: u32) {
    if m.s(ph + 0x1fba) == 0 {
        m.w16(ph + 0x1fba, 1);
        m.w8(ph + 0x1520, 1);
    }
    if m.r8(ph + 0x1520) == 1 {
        m.w16(ph + 0x1fa6, -2);
    } else {
        m.w16(ph + 0x1fa6, -1);
    }
}

/// FUN_1c0169a0
pub fn frames_init(m: &mut Mem, ph: u32) {
    m.w16(ph + 0x2ee, -1);
    m.w16(ph + 0x201c, -1);
    m.w16(ph + 0x11ac, 0);
}

/// FUN_1c0169c0: returns 1 when the clause is exhausted.
pub fn phone_advance(m: &mut Mem, ph: u32) -> i32 {
    if m.s(ph + 0x201c) < m.s(ph + 0x11ae) {
        return 0;
    }
    let mut k: i32 = 0;
    if m.s(ph + 0x1426) > -1 {
        loop {
            let o = (k * 2) as u32;
            k += 1;
            for arr in [0x1428, 0x142c, 0x1430] {
                let base = m.r32(ph + arr);
                m.w16(base.wrapping_add(o), 0);
            }
            if k as i16 > m.r16(ph + 0x1426) {
                break;
            }
        }
    }
    1
}

const TRACK_OUT: [(u32, u32); 17] = [
    (0x20, 0x2d8), (0x44, 0x2c8), (0x68, 0x2dc), (0x8c, 0x2de), (0xb0, 0x2e0), (0xd4, 0x2e2),
    (0xf8, 0x2e4), (0x11c, 0x2e6), (0x140, 0x2da), (0x164, 0x2c6), (0x188, 0x2ca), (0x1ac, 0x2cc),
    (0x1d0, 0x2ce), (0x1f4, 0x2d0), (0x218, 0x2d2), (0x23c, 0x2d4), (0x260, 0x2d6),
];

/// Binds each 0x24-byte parameter track (+0x20 slot) to its output word in the frame block.
pub fn bind_track_outputs(m: &mut Mem, ph: u32) {
    for (slot, out) in TRACK_OUT {
        m.w32(ph + slot, ph + out);
    }
}

#[inline]
pub(crate) fn wrap(v: i32) -> i32 {
    v as i16 as i32
}

/// FUN_1c016c60: advance all parameter tracks by one frame and write the frame block.
pub fn frame_tracks_interp(m: &mut Mem, ctx: u32) {
    let ph = ph_of(m, ctx);
    let aux = m.r32(ph + 0x2848);
    let owner = m.r32(ph + 0x11a8);
    if m.s(aux + 0xa6) == 0 {
        m.w16(aux + 0xa6, 1);
        bind_track_outputs(m, ph);
    }
    let now = m.s(ph + 0x2ee);

    let mut t = ph + 0x24;
    while t <= ph + 0xfc {
        let out = m.r32(t + 0x20);
        if m.i(owner + 0xd4) != 0 {
            return;
        }
        if m.s(t + 2) < now && 0 < now && -1 < m.s(t + 2) {
            let p = m.r32(t + 0x1c);
            m.w16(t + 2, m.s(p));
            let slope = m.s(p + 2);
            m.w32(t + 0x1c, p + 4);
            m.w16(t, m.s(t) + (m.s(t + 6) >> 3));
            m.w16(t + 4, slope);
            m.w16(t + 6, 0);
        }
        m.w16(t + 6, m.s(t + 6) + m.s(t + 4));
        let mut x = wrap(m.s(t + 6) + m.s(t + 8));
        if m.s(t + 8) != 0 {
            m.w16(t + 8, m.s(t + 8) - m.s(t + 0xa));
        }
        if m.s(t + 0x10) <= now {
            x = wrap(x + m.s(t + 0xc));
            m.w16(t + 0xc, m.s(t + 0xe) + m.s(t + 0xc));
        }
        if t == ph + 0x48 {
            let v = m.s(ph + 0x2ac);
            x = wrap(x + v);
            if v != 0 {
                m.w16(ph + 0x2ac, v - m.s(ph + 0x2b2));
            }
            if m.s(ph + 0x2b0) <= now {
                x = wrap(x + m.s(ph + 0x2ae));
                m.w16(ph + 0x2ae, m.s(ph + 0x2b4) + m.s(ph + 0x2ae));
            }
        }
        x = wrap((x >> 3) + m.s(t));
        m.w16(out, x);
        let hold = m.s(t + 0x12);
        if hold < 1 {
            if t == ph + 0xb4 {
                m.w16(out, (m.s(ph + 0x2b8) * x) >> 12);
            }
        } else if now < hold {
            m.w16(out, m.s(t + 0x14));
        }
        t += 0x24;
    }

    while t <= ph + 0x240 {
        let out = m.r32(t + 0x20);
        let x = wrap((m.s(t + 8) >> 3) + m.s(t));
        if m.s(t + 8) != 0 {
            m.w16(t + 8, m.s(t + 8) - m.s(t + 0xa));
        }
        if now < m.s(t + 0x10) {
            m.w16(out, x);
        } else {
            m.w16(out, (m.s(t + 0xc) >> 3) + x);
            m.w16(t + 0xc, m.s(t + 0xc) + m.s(t + 0xe));
        }
        let hold = m.s(t + 0x12);
        if 0 < hold {
            if now < hold {
                m.w16(out, m.s(t + 0x14));
            } else if t > ph + 0x144 && now == hold + 1 && 9 < m.s(out) {
                m.w16(out, m.s(out) - 10);
            }
        }
        t += 0x24;
    }

    if 6 < m.s(ph + 0x2da) {
        m.w16(ph + 0x2da, m.s(ph + 0x2da) - m.s(ph + 0x2f0));
    }
    let k = if m.s(ph + 0x11b0) == 1 { -900 } else { -0x73a };
    let y = wrap(((m.s(ph + 0x2c4) + k) * m.s(ph + 0x2be)) >> 12).max(0);
    m.w16(ph + 0x2d6, m.s(ph + 0x2d6) + (y >> 1));
    m.w16(ph + 0x2d6, m.s(ph + 0x2bc) + m.s(ph + 0x2d6));
    if m.s(ph + 0x2b6) == 1 {
        if 0x28 < m.s(ph + 0x2da) {
            if m.s(aux + 0xa8) < 0x1b {
                m.w16(aux + 0xa8, m.s(aux + 0xa8) + 2);
            }
            let y = wrap((m.s(ph + 0x2ba) * m.s(aux + 0xa8)) >> 12);
            if m.s(ph + 0x2c6) < y {
                m.w16(ph + 0x2c6, m.s(ph + 0x2c6) + y);
            }
            if m.s(aux + 0xaa) < 0x10 {
                m.w16(aux + 0xaa, m.s(aux + 0xaa) + 1);
            }
            m.w16(ph + 0x2d6, m.s(ph + 0x2d6) + wrap((m.s(ph + 0x2ba) * m.s(aux + 0xaa)) >> 12));
        }
    } else {
        m.w16(aux + 0xa8, 0);
        m.w16(aux + 0xaa, 0);
    }
    if 0x1f < m.s(ph + 0x2d6) {
        m.w16(ph + 0x2d6, 0x1f);
    }
    if m.s(ph + 0x2d6) < 0 {
        m.w16(ph + 0x2d6, 0);
    }
    if 0x12 < m.s(ph + 0x2d6) && 3 < m.s(ph + 0x2da) {
        let y = ((m.s(ph + 0x2d6) >> 2) - 4).max(0);
        m.w16(ph + 0x2da, m.s(ph + 0x2da) + y);
    }
}

/// FUN_1c016a20: finish the pending frame message (F0-side fields come from this frame) and
/// start the next one. Messages go out one frame late.
pub fn frame_emit(m: &mut Mem, ctx: u32) {
    let ph = ph_of(m, ctx);
    let kernel = kernel_of(m, ctx);
    let mut active: u32 = 0;
    let buf;
    if m.s(ph + 0x2828) == 0 {
        m.w16(ph + 0x2828, 1);
        buf = msg_alloc(m, 0);
    } else {
        let cur = m.r32(ph + 0x282c);
        m.w16(cur + 0x14, m.s(ph + 0x2da));
        active = (m.s(cur + 0x14) != 0) as u32;
        let idx = m.s(ph + 0x2d6);
        m.w16(cur + 0x10, m.s(DAT_1C0699EC.wrapping_add((idx * 2) as u32)));
        if m.s(cur + 0x10) < 0xc {
            m.w16(cur + 0x10, 0xc);
        }
        m.w16(cur + 0x12, m.s(ph + 0x2d8));
        if m.i(kernel + 0x174) != 0 {
            m.w16(ph + 0x2ee, m.s(ph + 0x11ac));
            m.w16(ph + 0x201c, m.s(ph + 0x11ae));
            m.w16(ph + 0x1fba, 0);
            return;
        }
        msg_send(m, kernel, cur);
        buf = msg_alloc(m, 0);
    }
    m.w32(ph + 0x282c, buf);
    let copy = |m: &mut Mem, dst: u32, src: u32| m.w16(buf + dst, m.s(ph + src));
    copy(m, 0x02, 0x2c8);
    copy(m, 0x1c, 0x2e2);
    copy(m, 0x16, 0x2dc);
    copy(m, 0x1e, 0x2e4);
    copy(m, 0x18, 0x2de);
    copy(m, 0x20, 0x2e6);
    copy(m, 0x1a, 0x2e0);
    for (dst, src) in [(0x04, 0x2ca), (0x06, 0x2cc), (0x08, 0x2ce), (0x0a, 0x2d0), (0x0c, 0x2d2), (0x0e, 0x2d4)] {
        copy(m, dst, src);
        if m.s(buf + dst) != 0 {
            active = 1;
        }
    }
    copy(m, 0x00, 0x2c6);
    copy(m, 0x22, 0x2e8);
    copy(m, 0x24, 0x2ea);
    m.w32(ph + 0x2010, m.r32(ph + 0x2010).wrapping_add(active));
}
