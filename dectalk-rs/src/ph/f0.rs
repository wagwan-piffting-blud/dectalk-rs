//! F0 contour generator (FUN_1c0170c0 and helpers), run once per frame.
//! `ph` = PH state, `aux` = *(ph+0x2848). Two voice-type branches keyed on ph+0x11b0.

use super::mem::Mem;
use super::{kernel_of, ph_of, wrap};

const DAT_1C06722C: u32 = 0x1c06_722c;
const PH_FEATURES: u32 = 0x1c06_73fc;
const PH_FEATURES2: u32 = 0x1c06_7388;
const SINE: u32 = 0x1c06_996c;
const PITCH_TARGETS: u32 = 0x1c06_9a2a;

fn idx(base: u32, i: i32, scale: i32) -> u32 {
    base.wrapping_add((i * scale) as u32)
}

/// FUN_1c0188a0: set up a glide toward an absolute or table pitch target.
fn pitch_target(m: &mut Mem, ph: u32, p: u32) {
    let aux = m.r32(ph + 0x2848);
    if 1999 < m.s(p) {
        m.w16(p, m.s(p) - 2000);
    }
    let v = m.s(p);
    if v < 0x26 {
        let t = m.s(idx(PITCH_TARGETS, v, 2));
        m.w16(aux + 0x8c, 1);
        m.w16(aux + 0x8e, t);
        m.w16(aux + 0x90, (t - m.s(ph + 0x2c4)) >> 2);
    } else {
        let v = wrap(v * 10);
        m.w16(p, v);
        if v < 500 {
            m.w16(p, 500);
        } else if 0x1401 < v {
            m.w16(p, 0x1401);
        }
        let v = m.s(p);
        m.w16(aux + 0x8c, 0);
        m.w16(aux + 0x8e, v);
        let dur;
        if m.s(ph + 0x1440) == 6 {
            dur = m.s(aux + 0x66);
            if dur == 0 {
                m.w16(ph + 0x2c4, v);
            }
        } else {
            dur = m.s(idx(ph + 0xa4a, m.s(aux + 0x76), 2));
        }
        let d = wrap((m.s(aux + 0x8e) - m.s(ph + 0x2c4)) * 4);
        m.w16(aux + 0x90, d);
        if 0 < d {
            m.w16(aux + 0x90, d - 1 + dur);
        }
        if m.s(aux + 0x90) < 0 {
            m.w16(aux + 0x90, (m.s(aux + 0x90) - dur) + 1);
        }
        if dur != 0 {
            m.w16(aux + 0x90, m.s(aux + 0x90) / dur);
        }
    }
    m.w16(aux + 0x92, 0);
    m.w16(aux + 0x94, m.s(ph + 0x2c4));
}

/// FUN_1c0189d0: per-phoneme glottalization marker tracking.
fn glottal_track(m: &mut Mem, ph: u32) {
    let aux = m.r32(ph + 0x2848);
    let t = m.s(aux + 0x4c);
    let dur = m.s(aux + 0x56);
    if t < dur {
        if t != 8 && t != dur - 1 {
            return;
        }
        m.w16(aux + 0x50, m.s(aux + 0x52));
        return;
    }
    m.w16(aux + 0x76, m.s(aux + 0x76) + 1);
    m.w16(aux + 0x4c, t - dur);
    let mut g = m.s(aux + 0x50);
    let i = m.s(aux + 0x76);
    m.w16(aux + 0x56, m.s(idx(ph + 0xa48, i, 2)));
    if g == 0 {
        m.w16(aux + 0x50, -200);
        g = -200;
    }
    if -1 < g {
        m.w16(aux + 0x50, 0);
    }
    m.w16(aux + 0x52, -200);
    let next = m.s(idx(ph + 0x304, i, 2));
    let flags_lo = m.r8(idx(ph + 0x574, i, 4));
    let stress = m.r32(idx(ph + 0x570, i, 4)) & 0x1e0;
    let mut mark = false;
    if (m.r8(idx(PH_FEATURES, next, 2)) & 4) != 0 && (flags_lo & 0x10) == 0 && 0x5f < stress {
        let cur = m.s(idx(ph + 0x302, i, 2));
        let f = m.r8(idx(PH_FEATURES, cur, 2));
        if (f & 1) == 0 {
            mark = !((f & 0x40) != 0 || (flags_lo & 1) == 0);
        } else {
            mark = !(((cur != next) || (flags_lo & 1) == 0) && stress < 0xa0);
        }
    }
    if mark {
        m.w16(aux + 0x52, m.s(aux + 0x56));
    }
    if (m.r8(idx(PH_FEATURES2, m.s(idx(ph + 0x304, i, 2)), 2)) & 0x20) != 0 {
        m.w16(aux + 0x52, m.s(aux + 0x56));
    }
    if (m.r8(idx(PH_FEATURES2, m.s(idx(ph + 0x302, i, 2)), 2)) & 0x20) != 0 {
        m.w16(aux + 0x52, m.s(aux + 0x56));
    }
}

/// FUN_1c018b00: two cascaded one-pole smoothers on the phrase contour -> ph+0x2c4.
fn smooth_phrase(m: &mut Mem, ph: u32, input: i32) {
    let aux = m.r32(ph + 0x2848);
    let g_in = m.s(aux + 0x64);
    let g_fb = m.s(aux + 0x62);
    m.w16(ph + 0x11c4, g_fb);
    let y1 = m.s(aux + 0x58);
    m.w16(ph + 0x11c6, y1);
    let s1 = wrap(((y1 * g_fb) >> 14) as i16 as i32 + (((input as i16 as i32) * g_in) >> 14) as i16 as i32);
    m.w16(aux + 0x58, s1);
    let g_in2 = m.s(aux + 0x60);
    m.w16(ph + 0x11c4, g_fb);
    let y2 = m.s(aux + 0x5a);
    m.w16(ph + 0x11c6, y2);
    let s2 = wrap(((y2 * g_fb) >> 14) as i16 as i32 + ((s1 * g_in2) >> 14) as i16 as i32);
    m.w16(aux + 0x5a, s2);
    m.w16(ph + 0x2c4, s2 >> 3);
}

/// FUN_1c018bc0: two cascaded smoothers on the accent impulse -> ph+0x2c2.
fn smooth_accent(m: &mut Mem, ph: u32) {
    let aux = m.r32(ph + 0x2848);
    let g_in = m.s(aux + 0xa4);
    let x = m.s(aux + 0x6a);
    let g_fb = m.s(aux + 0xa2);
    m.w16(ph + 0x11c4, g_fb);
    let y1 = m.s(aux + 0x98);
    m.w16(ph + 0x11c6, y1);
    let s1 = wrap(((y1 * g_fb) >> 14) as i16 as i32 + ((x * g_in) >> 14) as i16 as i32);
    m.w16(aux + 0x98, s1);
    let g_in2 = m.s(aux + 0xa0);
    let x2 = wrap(m.s(aux + 0x6c) * 8 + s1);
    m.w16(aux + 0x6c, m.s(aux + 0x6c) - 1);
    m.w16(ph + 0x11c4, g_fb);
    let y2 = m.s(aux + 0x9a);
    m.w16(ph + 0x11c6, y2);
    let s2 = wrap(((y2 * g_fb) >> 14) as i16 as i32 + ((x2 * g_in2) >> 14) as i16 as i32);
    m.w16(aux + 0x9a, s2);
    m.w16(ph + 0x2c2, s2 >> 3);
}

/// FUN_1c018cb0: linear glide toward the pitch target (singing/tone mode).
fn glide(m: &mut Mem, ph: u32) {
    let aux = m.r32(ph + 0x2848);
    m.w16(aux + 0x92, m.s(aux + 0x92) + m.s(aux + 0x90));
    let v = wrap((m.s(aux + 0x92) >> 2) + m.s(aux + 0x94));
    m.w16(ph + 0x2c4, v);
    let target = m.s(aux + 0x8e);
    let reached = if m.s(aux + 0x90) < 0 { target > v } else { v > target };
    if reached {
        m.w16(ph + 0x2c4, target);
        m.w16(aux + 0x94, target);
        m.w16(aux + 0x92, 0);
        m.w16(aux + 0x90, 0);
    }
    m.w16(ph + 0x1faa, m.s(ph + 0x2c4));
    if m.s(aux + 0x8c) == 1 {
        m.w16(aux + 0x84, m.s(aux + 0x84) + 0xa5);
        if 0x1000 < m.s(aux + 0x84) {
            m.w16(aux + 0x84, m.s(aux + 0x84) - 0x1000);
        }
        let s = m.s(idx(SINE, m.s(aux + 0x84) >> 6, 2));
        m.w16(ph + 0x1faa, m.s(ph + 0x1faa) + (s >> 3));
    }
}

const DECLINATION_A: [i16; 17] = [
    0x4a3, 0x48d, 0x473, 0x467, 0x459, 0x455, 0x447, 0x43a, 0x437, 0x429, 0x41d, 0x3ac, 0x3aa,
    0x39c, 0x3f2, 0x3f6, 0x3fa,
];
const DECLINATION_B: [i16; 17] = [
    0x485, 0x479, 0x473, 0x467, 0x459, 0x455, 0x447, 0x43a, 0x437, 0x429, 0x41d, 0x410, 0x40e,
    0x400, 0x3f2, 0x3d4, 0x370,
];
const DECLINATION_C: [i16; 17] = [
    0x488, 0x47e, 0x47e, 0x480, 0x46c, 0x460, 0x44c, 0x450, 0x44c, 0x438, 0x42e, 0x424, 0x41a,
    0x410, 0x406, 1000, 1000,
];

fn select_declination(m: &mut Mem, ph: u32, limit: i32) {
    let t = if m.s(ph + 0x1c82) < limit { ph + 0x1cae } else { ph + 0x1c8c };
    m.w32(ph + 0x1c88, t);
}

/// FUN_1c0170c0
pub fn f0_frame(m: &mut Mem, ctx: u32) {
    let ph = ph_of(m, ctx);
    let kernel = kernel_of(m, ctx);
    let aux = m.r32(ph + 0x2848);
    let male = m.s(ph + 0x11b0) == 1;

    if m.s(ph + 0x1fa6) < -1 {
        m.w16(aux + 0x44, 0);
        m.w16(ph + 0x1fb4, 0);
        m.w16(aux + 0x3a, 0);
        for (k, v) in DECLINATION_A.iter().enumerate() {
            m.w16(ph + 0x1cd0 + k as u32 * 2, *v as i32);
        }
        for (k, v) in DECLINATION_B.iter().enumerate() {
            m.w16(ph + 0x1c8c + k as u32 * 2, *v as i32);
        }
        for (k, v) in DECLINATION_C.iter().enumerate() {
            m.w16(ph + 0x1cae + k as u32 * 2, *v as i32);
        }
        select_declination(m, ph, 4);
        let decl = m.r32(ph + 0x1c88);
        m.w16(aux + 0x86, m.s(decl));
        m.w16(aux + 0x8a, m.s(decl + 0x20));
        m.w16(aux + 0x6e, m.s(aux + 0x86));
        m.w16(aux + 0x7a, m.s(aux + 0x8a));
        for o in [0x48, 0x36, 0x32, 0x9c, 0x80, 0x82, 0x84] {
            m.w16(aux + o, 0);
        }
        m.w16(aux + 0x50, -200);
        let base8 = if male { wrap(m.s(aux + 0x86) << 3) } else { 0 };
        m.w16(aux + 0x58, base8);
        m.w16(aux + 0x5a, base8);
        m.w32(0x1c06_effc, 0);
        m.w16(aux + 0x98, 0);
        m.w16(aux + 0x9a, 0);
        m.w16(ph + 0x2c4, m.s(aux + 0x86));
        m.w16(aux + 0x5c, 0);
        m.w16(aux + 0x5e, 0);
        m.w16(aux + 0x9e, 0);
        let q = m.s(ph + 0x1524);
        m.w16(aux + 0xa2, 0x3448);
        m.w16(aux + 0x60, q);
        m.w16(aux + 0x62, 0x4000 - q);
        m.w16(aux + 0x64, m.s(aux + 0x60) << 3);
        m.w16(aux + 0xa0, 3000);
        m.w16(aux + 0xa4, 24000);
        m.w16(aux + 0x8e, m.s(aux + 0x86));
        m.w16(aux + 0x90, 0);
        m.w16(aux + 0x92, 0);
        m.w16(aux + 0x94, m.s(ph + 0x2c4));
        m.w16(aux + 0x8c, 0);
        m.w16(ph + 0x1fa6, -1);
    }

    if m.s(ph + 0x1fa6) == -1 {
        // The original first picks a table by ph+0x2fc, then unconditionally overwrites it.
        select_declination(m, ph, 3);
        m.w16(aux + 0x72, m.s(aux + 0x86));
        m.w16(aux + 0x6e, m.s(aux + 0x86));
        m.w16(aux + 0x7a, m.s(aux + 0x8a));
        for o in [0x48, 0x36, 0x32, 0x9c] {
            m.w16(aux + o, 0);
        }
        m.w16(ph + 0x2844, 0);
        if m.r8(ph + 0x143e) != 0 {
            m.w16(aux + 0x6e, m.s(aux + 0x6e) + 0x78);
            m.w16(aux + 0x7a, m.s(aux + 0x7a) + 0x46);
            m.w8(ph + 0x143e, 0);
        }
        let first = m.s(ph + 0x1d36);
        m.w16(aux + 0x74, -1);
        m.w16(aux + 0x66, first);
        m.w16(aux + 0x76, -1);
        m.w16(ph + 0x1fa6, 0);
        m.w16(aux + 0x4a, 0);
        m.w16(aux + 0x46, 0);
        let base8 = if male { wrap(m.s(aux + 0x86) << 3) } else { 0 };
        m.w16(aux + 0x58, base8);
        m.w16(aux + 0x4c, 0);
        m.w16(aux + 0x5a, base8);
        m.w32(0x1c06_effc, 0);
        m.w16(aux + 0x98, 0);
        m.w16(aux + 0x9a, 0);
        m.w16(ph + 0x2c4, m.s(aux + 0x86));
        select_declination(m, ph, 3);
        m.w16(aux + 0x4e, -4);
        m.w16(aux + 0x54, 0);
        m.w16(aux + 0x56, 0);
        m.w16(ph + 0x1c7e, 0);
        m.w16(ph + 0x1c7c, 0);
        m.w16(ph + 0x1fb4, 0);
        for o in [0x3a, 0x3e, 0x3c, 0x5c] {
            m.w16(aux + o, 0);
        }
    }

    // Consume pitch commands whose start time has been reached.
    while m.s(aux + 0x66) <= m.s(aux + 0x46) {
        let cur = m.s(ph + 0x1fa6);
        if m.s(ph + 0x1fa8) <= cur {
            break;
        }
        let p = aux + 0x38;
        m.w16(p, m.s(idx(ph + 0x1536, cur, 2)));
        let kind = m.s(idx(ph + 0x17a2, cur, 2));
        m.w16(aux + 0x46, m.s(aux + 0x46) - m.s(aux + 0x66));
        m.w16(aux + 0x40, kind);
        m.w16(aux + 0x42, m.s(idx(ph + 0x1a0e, cur, 2)));
        if m.s(ph + 0x1440) == 6 {
            m.w16(aux + 0x66, m.s(idx(ph + 0x1d36, cur, 2)));
            m.w16(ph + 0x1fa6, cur + 1);
            pitch_target(m, ph, p);
            continue;
        }
        m.w16(ph + 0x1fa6, cur + 1);
        m.w16(aux + 0x66, m.s(idx(ph + 0x1d36, cur + 1, 2)));
        let v = m.s(p);
        let rate_adj = (m.s(ph + 0x1524) - 0x514) >> 8;
        match m.s(aux + 0x40) {
            0 => pitch_target(m, ph, p),
            2 => {
                m.w16(aux + 0x5c, m.s(aux + 0x5c) + v);
                let clear = if v < 0 { 0 < m.s(aux + 0x5e) } else { m.s(aux + 0x5e) < 0 };
                if clear {
                    m.w16(aux + 0x5e, 0);
                    m.w16(aux + 0x9e, 0);
                }
            }
            3 => {
                m.w16(aux + 0x5c, 0);
                m.w16(ph + 0x1fb4, 0);
                m.w16(aux + 0x3a, 0);
                m.w16(aux + 0x3e, 0);
                m.w16(aux + 0x3c, 0);
            }
            k @ (1 | 4) => {
                if k == 4 {
                    m.w16(ph + 0x1fb4, 1);
                }
                m.w16(aux + 0x5e, v);
                m.w16(aux + 0x9e, v >> 2);
                m.w16(aux + 0x78, 0x14 - rate_adj);
            }
            5 => {
                m.w16(aux + 0x3a, v);
                m.w16(aux + 0x3c, v / m.s(aux + 0x42));
            }
            6 if !male => {
                m.w16(aux + 0x5e, v);
                m.w16(aux + 0x9e, 0);
                m.w16(aux + 0x78, 5 - rate_adj);
            }
            _ => {}
        }
    }

    let (lead, cap, step) = if male { (0xc, 900, 100) } else { (0xe, 0x640, 0xb4) };
    if m.s(ph + 0x1fa4) - lead < m.s(aux + 0x48) && m.s(ph + 0x1fb4) != 0 && 3 < m.s(ph + 0x1c82) {
        if m.s(ph + 0x1fae) < cap {
            m.w16(ph + 0x1fae, m.s(ph + 0x1fae) + step);
        }
    } else {
        m.w16(ph + 0x1fae, 0);
    }

    if m.s(aux + 0x36) <= m.s(aux + 0x48) * 0x10 {
        let decl = m.r32(ph + 0x1c88);
        m.w16(aux + 0x34, m.s(aux + 0x72) - m.s(idx(decl + 2, m.s(aux + 0x32), 2)));
        m.w16(aux + 0x36, m.s(aux + 0x36) + m.s(ph + 0x1fa4));
        if m.s(aux + 0x32) < 0x10 {
            m.w16(aux + 0x32, m.s(aux + 0x32) + 1);
        }
        if m.s(ph + 0x1fa4) == 0 {
            m.w16(ph + 0x1fa4, 1);
        }
    }
    let slope = wrap(m.s(aux + 0x34) << 4);
    m.w16(aux + 0x9c, slope);
    m.w16(aux + 0x72, m.s(aux + 0x72) - slope / m.s(ph + 0x1fa4));

    if male {
        if m.s(aux + 0x96) < 0x1f5 {
            m.w16(aux + 0x96, m.s(aux + 0x72));
        } else {
            let e = wrap((m.s(ph + 0x1522) * m.s(ph + 0x1fae)) >> 12);
            m.w16(ph + 0x1fb2, e);
            m.w16(aux + 0x96, m.s(aux + 0x72) - e);
        }
    } else {
        let e = wrap((m.s(ph + 0x1522) * m.s(ph + 0x1fae)) >> 12);
        m.w16(ph + 0x1fb2, e);
        if m.s(aux + 0x96) < 0x1f5 {
            m.w16(aux + 0x96, m.s(aux + 0x72));
        } else {
            m.w16(aux + 0x96, m.s(aux + 0x72) - e);
        }
    }

    m.w16(aux + 0x48, m.s(aux + 0x48) + 1);
    m.w16(aux + 0x78, m.s(aux + 0x78) - 1);
    if m.s(aux + 0x78) < 0 {
        m.w16(aux + 0x5e, 0);
        m.w16(aux + 0x9e, 0);
    }

    if m.s(aux + 0x54) + m.s(aux + 0x4e) <= m.s(aux + 0x4a) && m.s(aux + 0x74) < m.s(ph + 0x11ae) - 1 {
        let n = wrap(m.s(aux + 0x74) + 1);
        m.w16(aux + 0x4a, m.s(aux + 0x4a) - m.s(aux + 0x54));
        m.w16(aux + 0x74, n);
        m.w16(aux + 0x54, m.s(idx(ph + 0xa48, n, 2)));
        m.w16(aux + 0x44, m.s(idx(ph + 0x302, n, 2)));
        if n < m.s(ph + 0x11ae) {
            m.w16(aux + 0x68, m.s(idx(ph + 0x304, n, 2)));
        }
        let phone = m.s(aux + 0x44);
        let mut amp = m.s(idx(DAT_1C06722C, phone, 2));
        let stressed = (m.r8(idx(ph + 0x570, n, 4)) & 3) != 0;
        if !stressed {
            amp >>= 1;
        }
        m.w16(aux + 0x4e, -4);
        if (m.r8(idx(PH_FEATURES, m.s(aux + 0x68), 2)) & 2) == 0 {
            m.w16(aux + 0x4e, 0);
        }
        let f = m.r8(idx(PH_FEATURES, phone, 2));
        if (f & 2) == 0 {
            m.w16(aux + 0x6c, amp);
            m.w16(aux + 0x6a, 0);
            m.w16(aux + 0x4e, 1);
            if (f & 0x40) != 0 {
                m.w16(aux + 0x4e, 5);
                if !stressed {
                    m.w16(aux + 0x4e, 3);
                }
            }
        } else {
            m.w16(aux + 0x6a, amp);
            m.w16(aux + 0x6c, 0);
        }
    }

    glottal_track(m, ph);

    if m.s(ph + 0x1440) < 4 {
        let d = m.s(aux + 0x3c);
        if male {
            m.w16(aux + 0x3e, m.s(aux + 0x3e) + d);
        }
        m.w16(aux + 0x3a, m.s(aux + 0x3a) - d);
        let rem = m.s(aux + 0x3a);
        let done = if d < 1 { d < 0 && d <= rem } else { rem <= d };
        if done {
            m.w16(aux + 0x3c, 0);
        }
        let arg = if male {
            let acc = m.s(aux + 0x5e);
            let phr = m.s(aux + 0x5c);
            let e = m.s(aux + 0x96);
            if acc != 0 {
                m.w16(aux + 0x5e, m.s(aux + 0x9e) + acc);
            }
            acc * 2 + phr + e
        } else {
            let phr = m.s(aux + 0x5c);
            m.w16(aux + 0x3e, m.s(aux + 0x3e) + m.s(aux + 0x3c));
            let acc = m.s(aux + 0x5e);
            if acc != 0 {
                m.w16(aux + 0x5e, m.s(aux + 0x9e) + acc);
            }
            phr + acc
        };
        let imp = m.s(aux + 0x6a);
        m.w16(aux + 0x9e, m.s(aux + 0x9e) >> 1);
        m.w16(ph + 0x11c4, imp);
        m.w16(ph + 0x11c6, 0x3ec0);
        m.w16(aux + 0x6a, (imp * 0x3ec0) >> 14);
        smooth_accent(m, ph);
        smooth_phrase(m, ph, arg);
        let accent = if male { m.s(ph + 0x2c2) } else { 0 };
        m.w16(ph + 0x1faa, m.s(ph + 0x2c4) + accent + m.s(aux + 0x3e));
    } else {
        glide(m, ph);
    }

    let jit = (m.s(aux + 0x4c) - m.s(aux + 0x50)).abs();
    if jit < 8 {
        m.w16(ph + 0x1faa, m.s(ph + 0x1faa) + jit * 0x46 - 0x226);
    }
    m.w16(ph + 0x2f0, if jit < 6 { 6 - jit } else { 0 });

    if m.s(ph + 0x1440) < 4 {
        m.w16(aux + 0x7e, m.s(aux + 0x7e) + 0x83);
        if 0x1000 < m.s(aux + 0x7e) {
            m.w16(aux + 0x7e, m.s(aux + 0x7e) - 0x1000);
        }
        m.w16(aux + 0x7c, m.s(aux + 0x7c) + 0x4f);
        if 0x1000 < m.s(aux + 0x7c) {
            m.w16(aux + 0x7c, m.s(aux + 0x7c) - 0x1000);
        }
        let a = m.s(idx(SINE, m.s(aux + 0x7e) >> 6, 2));
        let b = m.s(idx(SINE, m.s(aux + 0x7c) >> 6, 2));
        m.w16(ph + 0x11c6, 500);
        m.w16(ph + 0x11c4, a - b);
        m.w16(ph + 0x1faa, m.s(ph + 0x1faa) + ((wrap(a - b) * 500) >> 14));
    }

    if m.s(ph + 0x1440) < 4 {
        if male {
            let v = wrap(((m.s(ph + 0x1faa) - 0x4b0) * m.s(ph + 0x152e)) >> 12);
            m.w16(ph + 0x1faa, v + m.s(ph + 0x152c));
        } else {
            let v = wrap((m.s(ph + 0x152e) * m.s(ph + 0x1faa)) >> 12);
            m.w16(ph + 0x1faa, v);
            let e = wrap(((m.s(aux + 0x96) - 1000) * 0x1004) >> 12);
            m.w16(ph + 0x1faa, v - 200 + e + m.s(ph + 0x152c));
        }
    }
    if !male {
        if 600 < m.s(ph + 0x1fb2) {
            m.w16(ph + 0x1faa, m.s(ph + 0x1faa) >> 1);
        }
        if m.s(ph + 0x1444) == 1 {
            m.w16(ph + 0x1444, 0);
            let v = m.s(ph + 0x1faa);
            m.w16(ph + 0x1faa, ((m.s(ph + 0x1fac) - v) >> 1) as i16 as i32 + v);
        } else {
            m.w16(ph + 0x1444, 1);
        }
    }

    let f0 = m.s(ph + 0x1faa);
    m.w16(ph + 0x1fac, f0);
    if f0 >= 0x1402 {
        m.w16(ph + 0x1faa, 0x1401);
    } else if f0 < 500 {
        m.w16(ph + 0x1faa, 500);
    }
    if m.s(ph + 0x1440) == 4 {
        m.w16(ph + 0x1faa, (m.s(ph + 0x1faa) * 0x105e) >> 12);
    }
    m.w16(ph + 0x11c4, 400);
    m.w16(ph + 0x11c8, m.s(ph + 0x1faa));
    m.w16(ph + 0x11c6, 0x44c);
    m.w16(ph + 0x2d8, 440000 / m.s(ph + 0x1faa));

    let dbg = (m.r8(kernel + 0x204) & 0x20) != 0
        || ((m.r16(kernel + 0x478) as u16 & 0x2000) != 0 && (m.r16(kernel + 0x478) as u16 & 8) != 0);
    if dbg && m.s(aux + 0x74) != m.s(aux + 0x70) {
        unimplemented!("F0 debug trace output");
    }

    m.w16(aux + 0x46, m.s(aux + 0x46) + 1);
    m.w16(aux + 0x4a, m.s(aux + 0x4a) + 1);
    m.w16(aux + 0x4c, m.s(aux + 0x4c) + 1);
    let cur = m.s(aux + 0x74);
    m.w16(ph + 0x2e8, m.s(idx(ph + 0x302, cur, 2)));
    m.w16(ph + 0x2ea, m.s(idx(ph + 0xa48, cur, 2)));
}
