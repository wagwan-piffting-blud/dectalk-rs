//! Allophone selection (FUN_1c015ee0): context rewrites over the compacted phoneme array
//! (*(ph+0x1434), flags *(ph+0x1438)), producing the final phoneme list at ph+0x302 / ph+0x570.

use super::mem::Mem;
use super::rules::feat;
use super::{index_place, index_shift, kernel_of, ph_of};

const FEATURES: u32 = 0x1c06_73fc;

#[inline]
fn at(base: u32, i: i32, scale: i32) -> u32 {
    base.wrapping_add((i * scale) as u32)
}

#[inline]
fn phone(m: &Mem, ph: u32, i: i32) -> i32 {
    m.s(at(m.r32(ph + 0x1434), i, 2))
}

#[inline]
fn set_phone(m: &mut Mem, ph: u32, i: i32, v: i32) {
    let a = at(m.r32(ph + 0x1434), i, 2);
    m.w16(a, v);
}

#[inline]
fn flags(m: &Mem, ph: u32, i: i32) -> u32 {
    m.r32(at(m.r32(ph + 0x1438), i, 4))
}

/// FUN_1c0165c0: stressed vowels after `p` up to the first boundary at least `level` strong.
fn count_stressed(m: &Mem, ph: u32, p: i32, level: u32) -> i32 {
    let mut cnt: i32 = 0;
    let n = m.s(ph + 0x143c);
    let mut j = p;
    while j < n {
        let fl = flags(m, ph, j);
        if j != p && (fl & 1) != 0 && (feat(m, phone(m, ph, j)) & 1) != 0 {
            cnt += 1;
        }
        if level <= (fl & 0x1e0) || phone(m, ph, j) == 0 {
            return cnt as i16 as i32;
        }
        j += 1;
    }
    cnt as i16 as i32
}

/// FUN_1c016640: promote the last secondary-stressed vowel before a strong boundary to primary.
fn promote_secondary(m: &mut Mem, ph: u32, p: i32) -> i32 {
    let mut last: i32 = 0;
    let mut j = p;
    while j < m.s(ph + 0x143c) {
        let fl = flags(m, ph, j);
        if j != p && (fl as u8 & 3) == 2 && (feat(m, phone(m, ph, j)) & 1) != 0 {
            last = j;
        }
        if (0xdf < (fl & 0x1e0) || phone(m, ph, j) == 0) && last != 0 {
            let a = at(m.r32(ph + 0x1438), last, 4);
            m.w32(a, (m.r32(a) & !2) | 1);
            return 1;
        }
        j += 1;
    }
    0
}

/// FUN_1c0166f0: append to the final phoneme list.
fn emit_final(m: &mut Mem, ctx: u32, i: i32, out: i32, fl: u32, a: i32, b: i32) {
    let ph = ph_of(m, ctx);
    let kernel = kernel_of(m, ctx);
    let c = m.s(ph + 0x11ae);
    index_place(m, kernel, i, c);
    if c <= i + 8 {
        m.w16(at(ph + 0x302, c, 2), out);
        m.w32(at(ph + 0x570, c, 4), fl);
        m.w16(at(m.r32(ph + 0x1428), c, 2), a);
        if m.s(ph + 0x1440) != 3 {
            m.w16(at(m.r32(ph + 0x142c), c, 2), b);
        }
        if c < 300 {
            m.w16(ph + 0x11ae, c + 1);
        }
    }
}

/// FUN_1c015ee0
pub fn allophones(m: &mut Mem, ctx: u32) {
    let ph = ph_of(m, ctx);
    let kernel = kernel_of(m, ctx);
    let mut merged: i32 = 0;
    let mut n = m.s(ph + 0x143c);
    m.w16(ph + 0x11ae, 0);
    let mut drop_next = false;
    let mut stress_state: i32 = 1;
    let mut rise_done = false;
    let mut primaries: i32 = 0;
    let mut arg2: i32 = 0;
    let mut prev: i32 = 0;
    if 5 < n {
        m.w32(ph + 0x283c, 0);
    }
    let spell = (m.r32(kernel + 0x200) & 0x100) != 0 && m.r32(ph + 0x283c) != 0;

    let mut i: i32 = 0;
    while i < n {
        let mut next: i32 = 0;
        let f = flags(m, ph, i);
        let cur = phone(m, ph, i);
        if i < n - 1 {
            next = phone(m, ph, i + 1);
        }
        if 0 < i {
            prev = m.s(at(ph + 0x300, m.s(ph + 0x11ae), 2));
        }
        let a1 = m.s(at(m.r32(ph + 0x1428), i, 2));
        m.w16(at(m.r32(ph + 0x1428), i, 2), 0);
        if m.s(ph + 0x1440) != 3 {
            arg2 = m.s(at(m.r32(ph + 0x142c), i, 2));
            m.w16(at(m.r32(ph + 0x142c), i, 2), 0);
        }
        let mut out_flags = f;
        let mut out = cur;

        if (f & 0x2000) == 0 {
            let s4 = phone(m, ph, i + 1);
            let next_vowel = (feat(m, s4) & 1) != 0;
            let mut general = true;
            if cur == 0x11 {
                general = false;
                if next_vowel
                    && (f & 0x1e0) != 0
                    && phone(m, ph, i - 1) == 0x28
                    && (flags(m, ph, i - 1) as u8 & 4) != 0
                {
                    out = 1;
                }
                if next == 0 && i == 1 && spell {
                    out = 3;
                }
            }
            if general {
                if cur == 0x25 {
                    if next == 0xf && ((flags(m, ph, i + 1) as u8 & 0x1b) == 0 || spell) {
                        let s = phone(m, ph, i + 2);
                        if (feat(m, s) & 1) != 0 || s == 0 {
                            next = 0x16;
                            set_phone(m, ph, i + 1, 0x16);
                        }
                    }
                } else if cur == 0 {
                    if s4 == 5
                        && phone(m, ph, i + 2) == 0x20
                        && phone(m, ph, i + 3) == 0x30
                        && (((flags(m, ph, i + 1) as u8 & 3) == 0 && (flags(m, ph, i + 3) as u8 & 3) == 0) || spell)
                    {
                        set_phone(m, ph, i + 1, 5);
                        next = 5;
                    }
                    if phone(m, ph, i + 1) == 4 && phone(m, ph, i + 2) == 0x2f && spell {
                        set_phone(m, ph, i + 1, 5);
                        next = 5;
                    }
                }
            }

            if (f & 7) == 0 {
                let s4 = phone(m, ph, i - 1);
                if (feat(m, s4) & 4) != 0 {
                    if cur == 0x1b {
                        out = 0x1e;
                    } else if cur == 0x1a {
                        out = 0x1d;
                        let slot = at(ph + 0x300, m.s(ph + 0x11ae), 2);
                        if s4 == 0x11 {
                            m.w16(slot, 0xf);
                            drop_next = true;
                        } else {
                            let r = match s4 {
                                1 | 2 => Some(0x13),
                                3 | 4 | 5 => Some(0x14),
                                6 | 9 => Some(0x15),
                                0xb | 10 => Some(0x16),
                                0xe | 0xd => Some(0x17),
                                _ => None,
                            };
                            if let Some(r) = r {
                                m.w16(slot, r);
                                drop_next = true;
                            }
                        }
                    }
                }
            }

            if (next == 0x10 || next == 0x19)
                && (flags(m, ph, i + 1) as u8 & 3) == 0
                && (cur == 0x2f || cur == 0x30)
            {
                out = if cur == 0x2f { 0x36 } else { 0x37 };
            } else if cur == 0x2f {
                let nf = m.s(at(FEATURES, next, 2));
                if next == 0x1b
                    || next == 0x28
                    || (0x3f < (f & 0x1e0) && ((nf & 0x400) != 0 || next == 0x1c))
                    || next == 0x24
                {
                    out = 0x30;
                    if (feat(m, prev) & 8) != 0 {
                        out = 0x34;
                    }
                } else if next == 0xd {
                    set_phone(m, ph, i + 1, 0xe);
                }
            } else if cur == 0x28 {
                if (f & 3) == 0 {
                    if prev == 0x2f || prev == 0x34 || prev == 0x30 {
                        out = 0x23;
                    }
                    if prev == 0x20 {
                        out = 0x20;
                    }
                }
            } else if cur == 0 {
                rise_done = false;
            }

            if m.s(ph + 0x1440) == 1 && (feat(m, cur) & 1) != 0 && (f & 3) != 0 && !rise_done {
                if stress_state != 2 && ((f & 1) != 0 || 0 < count_stressed(m, ph, i, 0xe0)) {
                    stress_state = 2;
                    out_flags = f | 0x200;
                }
                if (f & 1) != 0 {
                    primaries += 1;
                }
                if stress_state == 2 && (f & 1) != 0 {
                    if (f & 3) == 3 {
                        rise_done = true;
                    }
                    let c = count_stressed(m, ph, i, 0xe0);
                    let fall = (c == 0 && ((f & 0x1e0) != 0xa0 || promote_secondary(m, ph, i) == 0))
                        || (1 < primaries as i16
                            && count_stressed(m, ph, i, 0xa0) == 0
                            && 1 < count_stressed(m, ph, i, 0xe0));
                    if fall {
                        stress_state = 1;
                        primaries = 0;
                        out_flags |= 0x400;
                    }
                }
            }

            if drop_next {
                merged += 1;
                index_shift(m, kernel, i + merged, -1);
                if arg2 != 0 {
                    let c = m.s(ph + 0x11ae);
                    m.w16(at(m.r32(ph + 0x142c), c - 1, 2), arg2);
                }
                drop_next = false;
            } else {
                emit_final(m, ctx, i, out, out_flags, a1, arg2);
            }
        } else {
            emit_final(m, ctx, i, out, out_flags, a1, arg2);
        }
        n = m.s(ph + 0x143c);
        i += 1;
    }
    if m.s(ph + 0x1440) == 2 {
        m.w16(ph + 0x1440, 1);
    }
    let c = m.s(ph + 0x11ae);
    m.w16(at(ph + 0x302, c, 2), 0);
    m.w32(at(ph + 0x570, c, 4), 0);
}
