//! Intonation planning (FUN_1c018d80): emits timed pitch commands for the F0 generator.
//! Ported from the disassembly; the decompiler merged several stack slots here.
//!
//! Commands (FUN_1c019c00) go to ph+0x1536 value, +0x17a2 kind, +0x1a0e arg, +0x1d36 start
//! offset in frames relative to the previous command; ph+0x1fa8 is the count.

use super::duration::to_frames;
use super::mem::Mem;
use super::{ph_of, wrap};

const FEATURES: u32 = 0x1c06_73fc;
const STRESS_RISE: u32 = 0x1c06_995c;
const STRESS_RISE_EMPH: u32 = 0x1c06_9962;
const STRESS_POSITION: u32 = 0x1c06_994c;
const GLOTTAL_AFTER: u32 = 0x1c06_72a0;

#[inline]
fn at(base: u32, i: i32, scale: i32) -> u32 {
    base.wrapping_add((i * scale) as u32)
}

#[inline]
fn feat(m: &Mem, v: i32) -> i32 {
    m.r16(at(FEATURES, v, 2)) as u16 as i32
}

/// FUN_1c019c00. `acc` is frames since the previous command (16-bit, as in the original).
fn add_cmd(m: &mut Mem, ph: u32, kind: i32, value: i32, delay: i32, arg: i32, acc: &mut i32) {
    let s1 = *acc as i16 as i32;
    let mut d = delay as i16 as i32;
    if s1 + d < 0 {
        d = -s1;
    }
    let k = m.s(ph + 0x1fa8);
    m.w16(at(ph + 0x1d36, k, 2), s1 + d);
    m.w16(at(ph + 0x1536, k, 2), value);
    m.w16(at(ph + 0x17a2, k, 2), kind);
    m.w16(at(ph + 0x1a0e, k, 2), arg);
    *acc = wrap(-d);
    if k < 299 {
        m.w16(ph + 0x1fa8, k + 1);
    }
}

#[inline]
fn dur(m: &Mem, ph: u32, i: i32) -> i32 {
    m.s(at(ph + 0xa48, i, 2))
}

#[inline]
fn flags(m: &Mem, ph: u32, i: i32) -> u32 {
    m.r32(at(ph + 0x570, i, 4))
}

/// Explicit pitch argument for mode 3 (user-specified pitches); `bias` differs per call site.
fn explicit_pitch(m: &mut Mem, ph: u32, pidx: &mut i32, used: &mut i32, bias: i32, fallback: i32) -> (i32, i32) {
    let idx = *pidx as i16 as i32;
    let mut v = wrap(m.s(at(m.r32(ph + 0x142c), idx, 2)).wrapping_mul(10) - bias);
    if 1999 < v || v < 1 || *used as i16 == 1 {
        v = fallback;
        *used = 1;
    }
    let delay = to_frames(m.s(at(m.r32(ph + 0x1430), idx, 2)));
    *pidx += 1;
    (v, delay)
}

/// FUN_1c018d80
pub fn intonation(m: &mut Mem, ctx: u32) {
    let ph = ph_of(m, ctx);
    let aux = m.r32(ph + 0x2848);
    m.w16(ph + 0x2842, 0);
    m.w16(aux + 0xac, 0);
    m.w16(aux + 0xae, 0);
    m.w16(aux + 0xb0, 0);
    let mut used: i32 = 0;
    let mut acc: i32 = 0;
    let n0 = m.s(ph + 0x11ae);
    let mut glottal_rise: i32 = 0;
    m.w16(ph + 0x2836, 0);
    m.w16(ph + 0x2838, 0);
    m.w16(ph + 0x1fa8, 0);
    let mut prev: i32 = 0;
    let mut prev_fl: i32 = 0;
    let mut pidx: i32 = 0;
    let mut i: i32 = 0;
    let mut next_or_lvl: i32 = 0;
    let mut lvl_a: i32 = 0;
    let mut lvl_b: i32 = 0;
    // Uninitialized in the original: left over from FUN_1c01f620's frame, which holds the
    // feature word of the clause's last phoneme at the same stack address.
    let mut rise_count: i32 = if n0 > 0 { feat(m, m.s(at(ph + 0x302, n0 - 1, 2))) } else { 0 };
    if n0 < 1 {
        m.w16(aux + 0xb2, m.s(ph + 0x2fc));
        return;
    }

    loop {
        let n = m.s(ph + 0x11ae);
        if 0 < i {
            prev = m.s(at(ph + 0x300, i, 2));
            prev_fl = m.r16(at(ph + 0x56c, i, 4)) as u16 as i32;
        }
        let fl = m.r16(at(ph + 0x570, i, 4)) as u16 as i32;
        let cur = m.s(at(ph + 0x302, i, 2));
        let st = fl & 3;
        let cf = feat(m, cur);
        if i < n - 1 {
            next_or_lvl = (next_or_lvl & !0xffff) | (m.r16(at(ph + 0x304, i, 2)) as u16 as i32);
        }
        m.w16(ph + 0x11a0, m.s(ph + 0x119e));
        if cur == 0 {
            m.w16(ph + 0x119e, 0);
        }

        let mut j = i;
        while (flags(m, ph, j) & 4) != 0 {
            j += 1;
        }
        let level = (flags(m, ph, j) & 0x1e0) as i32;
        if level >= 0x60 {
            lvl_a = level;
        } else {
            next_or_lvl += 1;
            while j < n {
                lvl_a = (flags(m, ph, j) & 0x1e0) as i32;
                if lvl_a >= 0x60 {
                    break;
                }
                j += 1;
            }
        }
        if level > 0x60 {
            lvl_b = lvl_a;
        } else {
            next_or_lvl += 1;
            while j < n {
                lvl_b = (flags(m, ph, j) & 0x1e0) as i32;
                if lvl_b > 0x60 {
                    break;
                }
                j += 1;
            }
        }

        let mode = m.s(ph + 0x1440);
        if mode == 5 || mode == 4 {
            let v = m.s(at(m.r32(ph + 0x142c), i, 2));
            if v != 0 {
                add_cmd(m, ph, 0, v + 2000, 0, 0, &mut acc);
            }
        } else {
            if (fl & 0x200) != 0 && 2 < m.s(ph + 0x1c82) {
                m.w16(ph + 0x2836, 1);
            }
            let focus = fl & 0x400;
            if focus != 0 && 2 < m.s(ph + 0x1c82) {
                m.w16(ph + 0x2838, 1);
            }
            if (mode == 1 || mode == 3) && (cf & 1) != 0 {
                if m.s(ph + 0x2836) != 0 {
                    if mode == 1 {
                        let v = m.s(ph + 0x1526);
                        m.w16(ph + 0x2836, 0);
                        m.w16(aux + 0xae, v);
                        if m.s(ph + 0x1442) != 0 || m.s(ph + 0x1c82) < 3 {
                            m.w16(aux + 0xae, v >> 2);
                        }
                        let delay = if focus != 0 { -13 } else { 0 };
                        let v = m.s(aux + 0xae);
                        add_cmd(m, ph, 2, v, delay, 0, &mut acc);
                    } else {
                        let (v, delay) = explicit_pitch(m, ph, &mut pidx, &mut used, 1998, 2);
                        m.w16(aux + 0xae, v);
                        add_cmd(m, ph, 2, v, delay, 0, &mut acc);
                    }
                    m.w16(aux + 0xb0, m.s(aux + 0xb0) + m.s(aux + 0xae));
                    m.w16(ph + 0x11a2, 2);
                    m.w16(ph + 0x119e, 1);
                }
                if m.s(ph + 0x2844) != 0 {
                    m.w16(aux + 0xac, 5);
                }
                if glottal_rise as i16 != 0 {
                    m.w16(aux + 0xac, 3);
                    glottal_rise = 0;
                }

                if m.s(ph + 0x2844) == 0 && st != 0 {
                    let pos = m.s(aux + 0xac);
                    let mut v = if pos == 0 && st == 3 {
                        m.s(STRESS_RISE_EMPH) - 200
                    } else {
                        m.s(at(STRESS_RISE, st, 2))
                    };
                    v = wrap(v + m.s(at(STRESS_POSITION, pos, 2)));
                    if m.s(ph + 0x1196) == 0 && m.s(aux + 0xb2) == 1 {
                        v >>= 1;
                    }
                    if m.s(ph + 0x1442) != 0 {
                        v >>= 2;
                    }
                    let mut delay = wrap((dur(m, ph, i) >> 2) + 4);
                    if focus != 0 || (fl & 0x100) != 0 {
                        v = wrap(v - 0x32);
                        delay = -3;
                        if v < 0 {
                            v = 0x1e;
                        }
                    }
                    if st == 3 {
                        delay = 1;
                    }
                    if m.s(ph + 0x1440) == 3 {
                        let (pv, d) = explicit_pitch(m, ph, &mut pidx, &mut used, 9999, 1);
                        v = pv;
                        delay = d;
                    }
                    let mut g = m.s(ph + 0x1528);
                    m.w16(ph + 0x11c4, g);
                    if st == 3 && g < 0x10 {
                        m.w16(ph + 0x11c4, 0x10);
                        g = 0x10;
                    }
                    m.w16(ph + 0x11c6, v);
                    m.w16(ph + 0x11c8, 0x20);
                    add_cmd(m, ph, 1, (g * v) / 32, delay, 0, &mut acc);
                    if m.s(aux + 0xac) < 5 {
                        m.w16(aux + 0xac, m.s(aux + 0xac) + 1);
                    }
                }

                if m.s(ph + 0x2838) != 0 {
                    m.w16(ph + 0x2838, 0);
                    let delay;
                    let mut p1: i32;
                    if mode == 1 {
                        m.w16(ph + 0x119e, 2);
                        p1 = 0xd4;
                        let mut d = wrap((dur(m, ph, i) >> 1) - 5);
                        if d < 4 {
                            d = 4;
                        }
                        let lv = fl & 0x1e0;
                        if lv == 0xe0 {
                            p1 = 0x96;
                            m.w16(ph + 0x119e, 3);
                        }
                        if lv == 0xa0 {
                            p1 = 0;
                        }
                        if lv < 0xa0 {
                            let mut k = i + 1;
                            while k < m.s(ph + 0x11ae) {
                                let f2 = flags(m, ph, k);
                                if (f2 & 0x200) != 0 {
                                    p1 = 0;
                                    break;
                                }
                                if (feat(m, m.s(at(ph + 0x302, k, 2))) & 1) != 0 {
                                    if (f2 & 3) == 0 {
                                        d = wrap(dur(m, ph, i) - 8);
                                    }
                                    let e = f2 & 0x1e0;
                                    if e == 0xa0 {
                                        p1 = 0;
                                        break;
                                    }
                                    if e > 0xa0 {
                                        p1 = 0x96;
                                        m.w16(ph + 0x119e, 3);
                                        break;
                                    }
                                }
                                k += 1;
                            }
                        }
                        if lv == 0x120 {
                            p1 = 0x50;
                        }
                        let lb = lvl_b as i16 as i32;
                        if level as i16 as i32 == lb {
                            if lb == 0xe0 || lb == 0x120 {
                                rise_count += 1;
                                d = -3;
                                p1 = 0x8c;
                            }
                        } else {
                            d = wrap(dur(m, ph, i) - 3);
                            if (lb == 0xe0 || lb == 0x120) && rise_count as i16 == 0 {
                                rise_count = 1;
                                d = -3;
                                p1 = 0x8c;
                            }
                        }
                        if m.s(ph + 0x119e) == 2 {
                            rise_count = 0;
                        }
                        let mut x = wrap((m.s(ph + 0x1522) * (p1 as i16 as i32)) >> 12);
                        if m.s(ph + 0x1442) != 0 {
                            x >>= 1;
                        }
                        p1 = wrap((x >> 2) + m.s(aux + 0xae));
                        delay = d;
                    } else {
                        let (v, d) = explicit_pitch(m, ph, &mut pidx, &mut used, 0xf9e, 2);
                        p1 = v;
                        delay = d;
                    }
                    add_cmd(m, ph, 2, -p1, delay, 0, &mut acc);
                    m.w16(aux + 0xb0, m.s(aux + 0xb0) - (p1 as i16 as i32));
                }

                let lv = fl & 0x1e0;
                if lv == 0xe0 || lv == 0x120 {
                    let d0 = dur(m, ph, i);
                    m.w16(ph + 0x2842, 0);
                    let d = wrap(d0 - 0xd);
                    if lv == 0x120 {
                        add_cmd(m, ph, 1, 0x15f, d, 0, &mut acc);
                        add_cmd(m, ph, 1, 0x1c3, d0, 0, &mut acc);
                    } else {
                        m.w16(ph + 0x2842, -50);
                        add_cmd(m, ph, 1, -0x65, d + 3, 0, &mut acc);
                        add_cmd(m, ph, 1, 0x10f, d0, 0, &mut acc);
                    }
                }
                // A second focus-fall block follows in the original but is unreachable:
                // every path to it has already cleared ph+0x2838.
            }

            if (cf & 1) != 0 {
                let lv = fl & 0x1e0;
                let fall = wrap((m.s(ph + 0x1522) * -20) >> 12);
                if (st & 1) != 0 && focus != 0 {
                    if lv == 0x100 {
                        add_cmd(m, ph, 4, fall, dur(m, ph, i) - 0x10, 0, &mut acc);
                    }
                } else {
                    if lv == 0x100 || lv == 0x140 {
                        add_cmd(m, ph, 4, fall | 1, dur(m, ph, i) - 0x10, 0, &mut acc);
                    }
                    let d0 = dur(m, ph, i);
                    let d = wrap(d0 - 0xd);
                    if lv == 0x120 {
                        add_cmd(m, ph, 1, 0x15f, d, 0, &mut acc);
                        add_cmd(m, ph, 1, 0x1c3, d0, 0, &mut acc);
                    }
                    if lv == 0xe0 {
                        add_cmd(m, ph, 1, -0x65, d + 3, 0, &mut acc);
                        add_cmd(m, ph, 1, 0x10f, d0, 0, &mut acc);
                    }
                }
            }

            if cur == 0 {
                let b0 = m.s(aux + 0xb0);
                if b0 != 0 && 0 < m.s(ph + 0x1fa8) {
                    add_cmd(m, ph, 2, -b0, 0, 0, &mut acc);
                    m.w16(aux + 0xb0, 0);
                }
                if 0 < i {
                    m.w16(aux + 0xac, 1);
                }
                if (m.r32(at(ph + 0x56c, i, 4)) & 0x1e0) == 0xe0
                    && 0 < m.s(ph + 0x1fa8)
                    && m.s(at(ph + 0x300, i, 2)) != 0
                {
                    add_cmd(m, ph, 3, 0, 0, 0, &mut acc);
                    glottal_rise = 1;
                }
                if (prev_fl & 0x100) != 0 {
                    add_cmd(m, ph, 3, 0, 0, 0, &mut acc);
                    m.w16(aux + 0xb0, 0);
                    m.w16(aux + 0xac, 0);
                }
            }
        }

        acc = wrap(acc + dur(m, ph, i));
        let n = m.s(ph + 0x11ae);
        if i <= n - 1 {
            m.w16(ph + 0x1fa4, m.s(ph + 0x1fa4) + dur(m, ph, i));
        }
        let cfw = feat(m, cur);
        if next_or_lvl as i16 == 0 && (cfw & 0x40) != 0 && (cfw & 0x800) != 0 {
            // Insert a release/glottal segment after a final stop.
            if n > i {
                let mut k = n;
                while k > i {
                    m.w16(at(ph + 0x302, k, 2), m.s(at(ph + 0x302, k - 1, 2)));
                    m.w32(at(ph + 0x570, k, 4), m.r32(at(ph + 0x570, k - 1, 4)));
                    m.w16(at(ph + 0xa48, k, 2), m.s(at(ph + 0xa48, k - 1, 2)));
                    let arr = m.r32(ph + 0x142c);
                    m.w16(at(arr, k, 2), m.s(at(arr, k - 1, 2)));
                    k -= 1;
                }
            }
            m.w16(at(ph + 0x304, i, 2), 0x11);
            if m.r32(at(GLOTTAL_AFTER, prev as i16 as i32, 4)) == 1 || (feat(m, cur) & 2) != 0 {
                m.w16(at(ph + 0x304, i, 2), 0x12);
            }
            m.w16(at(ph + 0xa4a, i, 2), 4);
            acc = wrap(acc + 4 + m.s(at(ph + 0xa4a, i, 2)));
            m.w16(ph + 0x1fa4, m.s(ph + 0x1fa4) + m.s(at(ph + 0xa4a, i, 2)));
            m.w32(at(ph + 0x574, i, 4), flags(m, ph, i) | 0x800);
            m.w16(ph + 0x11ae, m.s(ph + 0x11ae) + 1);
            i += 1;
        }
        i += 1;
        if m.s(ph + 0x11ae) <= i {
            break;
        }
    }
    m.w16(aux + 0xb2, m.s(ph + 0x2fc));
}
