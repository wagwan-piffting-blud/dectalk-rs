//! Segment duration rules (FUN_1c01f620 + FUN_1c0202e0). Final phonemes at ph+0x302 with
//! flags at ph+0x570; durations (frames) written to ph+0xa48.

use super::mem::Mem;
use super::{kernel_of, ph_of, wrap};

const FEATURES: u32 = 0x1c06_73fc;
const DUR_INHERENT: u32 = 0x1c06_7144;
const DUR_MINIMUM: u32 = 0x1c06_70d0;

#[inline]
fn at(base: u32, i: i32, scale: i32) -> u32 {
    base.wrapping_add((i * scale) as u32)
}

#[inline]
fn feat(m: &Mem, v: i32) -> i32 {
    m.r16(at(FEATURES, v, 2)) as u16 as i32
}

#[inline]
fn scale(k: i32, p: i32) -> i32 {
    wrap((k * p) >> 14)
}

/// FUN_1c01ec50: milliseconds-ish units to frames.
pub fn to_frames(v: i32) -> i32 {
    (v * 10) >> 6
}

/// FUN_1c0202e0: derive rate-dependent duration scale factors (aux+0x22/0x24/0x2e, ph+0x2f6).
fn rate_setup(m: &mut Mem, ctx: u32) {
    let ph = ph_of(m, ctx);
    let kernel = kernel_of(m, ctx);
    let aux = m.r32(ph + 0x2848);
    let rate = m.s(kernel + 0x222);
    if rate == m.s(aux + 0x20) {
        m.w32(ph + 0x1fbc, 0);
        return;
    }
    m.w16(ph + 0x2f6, 4000 / rate);
    m.w16(aux + 0x20, rate);
    if rate < 0xfb {
        m.w16(aux + 0x2e, rate);
    } else {
        m.w16(aux + 0x2e, ((rate - 0xfa) >> 1) + 0xfa);
    }
    let eff = m.s(aux + 0x2e);
    if eff < 0xb4 {
        m.w16(ph + 0x11c8, 0x78);
        m.w16(ph + 0x11c6, 300 - eff);
    } else {
        m.w16(ph + 0x11c8, 0xdc);
        m.w16(ph + 0x11c6, 0x1a9 - eff);
    }
    m.w16(ph + 0x11c4, 0x4000);
    if m.s(ph + 0x11c6) < 0 {
        m.w16(ph + 0x11c6, 1);
    }
    let k22 = wrap((m.s(ph + 0x11c6) << 14) / m.s(ph + 0x11c8));
    m.w16(aux + 0x22, k22);
    if 0xb4 < eff {
        m.w16(ph + 0x11c8, 0x118);
        let d = wrap(0x1cc - eff);
        m.w16(ph + 0x11c6, d);
        if d < 1 {
            m.w16(ph + 0x11c6, 1);
        }
        let x = m.s(ph + 0x11c6) * m.s(ph + 0x11c4);
        m.w16(aux + 0x24, x / 0x118);
    } else {
        m.w16(aux + 0x24, ((k22 + 0x4000) as u32 >> 1) as i32);
    }
    m.w32(ph + 0x1fbc, 0);
}

/// FUN_1c01f620
pub fn durations(m: &mut Mem, ctx: u32) {
    let ph = ph_of(m, ctx);
    let kernel = kernel_of(m, ctx);
    let aux = m.r32(ph + 0x2848);
    let mut last_primary: i32 = 0;
    let mut foot_dur: i32 = 0;
    let mut foot_vowels: i32 = 0;
    let mut lengthen_after = false;
    let mut prev: i32 = 0;
    let mut pflags: i32 = 0;
    let mut pfeat = feat(m, 0);
    rate_setup(m, ctx);
    m.w16(ph + 0x1fa4, 0);

    let mut i: i32 = 0;
    while i < m.s(ph + 0x11ae) {
        let n = m.s(ph + 0x11ae);
        if 0 < i {
            prev = m.s(at(ph + 0x300, i, 2));
            pflags = m.r16(at(ph + 0x56c, i, 4)) as u16 as i32;
            pfeat = feat(m, prev);
        }
        let cur = m.s(at(ph + 0x302, i, 2));
        let fl = m.r16(at(ph + 0x570, i, 4)) as u16 as i32;
        let stress_b = fl & 0x1e0;
        let st = fl & 3;
        let cf = feat(m, cur);
        let vow = (cf & 1) != 0;
        if i < n - 1 {
            let nx = m.s(at(ph + 0x304, i, 2));
            m.w16(aux + 0x28, nx);
            m.w16(aux + 0x2a, m.s(at(ph + 0x574, i, 4)));
            m.w16(aux + 0x2c, feat(m, nx));
        }
        if st != 0 {
            m.w16(aux + 0xb6, m.s(aux + 0xb6) + 1);
        }
        let next = m.s(aux + 0x28);
        let nfeat = m.r16(aux + 0x2c) as u16 as i32;
        let nflags_lo = m.r8(aux + 0x2a) as i32;

        let a = m.s(at(m.r32(ph + 0x1428), i, 2));
        'done: {
            if a != 0 {
                m.w16(aux + 0x30, to_frames(a + 4));
                break 'done;
            }
            let mut inh = wrap(((m.s(at(DUR_INHERENT, cur, 2)) + 5) * 10) >> 6);
            let mut add: i32 = 0;
            if cur == 0 {
                add = if ((nfeat & 2) == 0 || (nfeat & 0x20) == 0) && (nfeat & 0x40) == 0 { 0 } else { 1 };
                let x = (m.i(ph + 0x2010) - 500) / 10;
                m.w32(ph + 0x2010, x as u32);
                if i < 2 {
                    if m.r8(ph + 0x143e) != 0 {
                        add = 0x78;
                    }
                } else {
                    if (pflags & 0x1e0) == 0xe0 {
                        m.w32(ph + 0x2010, (-4i32) as u32);
                        add = wrap(m.s(ph + 0x1532) - 4 + 0x3c);
                    }
                    if (pflags & 0x100) != 0 {
                        m.w32(ph + 0x2010, (-10i32) as u32);
                        add = wrap(m.s(ph + 0x1534) - 10 + 0x78);
                    }
                }
                m.w32(ph + 0x2010, 0);
                let d = wrap((m.s(aux + 0x22) * add) >> 14).max(1);
                m.w16(aux + 0x30, d);
                break 'done;
            }

            if 0xdf < stress_b {
                add = 6;
                if (cf & 2) != 0 && (cf & 0x20) != 0 {
                    add = 3;
                }
                if (cf & 0x40) != 0 {
                    add = 0;
                }
                if (cur == 0x1d || cur == 0x1e) && (nfeat & 0x20) != 0 && (nfeat & 2) == 0 {
                    add = 2;
                }
                if m.s(ph + 0x11ae) < 10 && vow && st != 0 {
                    add += 5 - (m.s(ph + 0x11ae) >> 1);
                }
                if (nfeat & 8) != 0 {
                    add -= 3;
                }
            }

            let mut pct: i32 = 0x80;
            if !vow {
                if (fl & 4) == 0 {
                    if (cf & 0x20) == 0 || (cf & 0x40) != 0 || stress_b != 0x60 {
                        pct = 0x6c;
                    } else {
                        add += 3;
                    }
                }
            } else {
                if (stress_b < 0xa0 && 0xa0 < m.s(kernel + 0x222)) || stress_b < 0x80 {
                    pct = 0x59;
                }
                if (fl & 0x18) == 0 {
                    if (fl & 1) == 0 {
                        pct = scale(if st == 0 { 0x2ccd } else { 0x3667 }, pct);
                    }
                } else if stress_b < 0x60 {
                    pct = scale(0x3667, pct);
                }
                if (fl & 0x18) != 0 {
                    pct = scale(0x3334, pct);
                }
            }

            let min = wrap(((m.s(at(DUR_MINIMUM, cur, 2)) + 5) * 10) >> 6);
            if (fl & 1) == 0 {
                if inh < min && (cf & 0x20) == 0 {
                    if st == 0 {
                        inh >>= 1;
                    } else {
                        inh -= inh >> 2;
                    }
                }
                if !vow {
                    pct = if (0x18..=0x1b).contains(&cur) { pct >> 1 } else { scale(0x2ccd, pct) };
                } else {
                    pct = if (fl & 0x18) != 0x10 { scale(0x2ccd, pct) } else { pct >> 1 };
                    if (cur == 0x11 || cur == 0x12) && (prev == 0x33 || next == 0x33 || next == 0x1c) {
                        add += 4;
                    }
                }
            } else if vow && (fl & 0x400) != 0 && stress_b < 0xa0 && 0x40 < stress_b {
                add += 4;
            }

            // Stress-3 vowels lengthen themselves and the unmarked consonants that follow.
            let mut extra;
            if (fl & 4) == 0 && vow && st != 3 {
                lengthen_after = false;
                extra = add;
            } else if (fl & 4) != 0 {
                lengthen_after = false;
                if st == 3 {
                    lengthen_after = true;
                    extra = add + if vow { 9 } else { 3 };
                } else {
                    extra = add;
                }
            } else if st == 3 || lengthen_after {
                if st == 3 {
                    lengthen_after = true;
                }
                extra = add + if vow { 9 } else { 3 };
            } else {
                extra = add;
            }

            let mut k: i32 = 0x4000;
            let consonant_path = !vow
                && (cur < 0x1d || 0x21 < cur || (fl & 7) != 0 || (nfeat & 0x20) == 0);
            let mut run_consonant = consonant_path;
            if !consonant_path {
                let mut through = false;
                if (nfeat & 1) == 0 && (nflags_lo & 7) == 0 {
                    let mut s20 = next;
                    if 0x1c < s20 && s20 < 0x22 {
                        let s8 = m.s(at(ph + 0x306, i, 2));
                        if (feat(m, s8) & 0x20) != 0 && (m.r8(at(ph + 0x578, i, 4)) & 7) == 0 {
                            through = true;
                            s20 = s8;
                        }
                    }
                    if s20 != 0 {
                        let g = feat(m, s20);
                        if (g & 2) == 0 {
                            extra = wrap(extra - (extra >> 1));
                            k = 0x3334;
                            if (g & 0x40) != 0 || s20 == 0x36 {
                                k = 0x2ccd;
                            }
                        } else if (g & 0x20) == 0 || cur == 0x24 {
                            if (g & 0x80) != 0 {
                                k = 0x3667;
                            }
                        } else {
                            k = 0x4ccd;
                            if (g & 0x40) == 0 && s20 != 0x33 && vow {
                                extra += 4;
                            }
                        }
                    }
                }
                if stress_b < 0xa0 || through {
                    k = (k >> 1) + 0x2000;
                }
                if cur == 0x20 && next == 0x2f && (nflags_lo & 7) == 0 {
                    k = 0x666;
                    if (feat(m, m.s(at(ph + 0x306, i, 2))) & 1) != 0 && (m.r8(at(ph + 0x578, i, 4)) & 0x10) == 0 {
                        m.w16(at(ph + 0x304, i, 2), 0x30);
                        k = 0x2ccd;
                    }
                }
                pct = scale(k, pct);
                if !vow {
                    run_consonant = true;
                } else {
                    if (nfeat & 1) != 0 {
                        extra += 5;
                    }
                    if (fl & 0x18) == 8 && (fl & 1) != 0 && (pflags & 4) == 0 {
                        extra += 4;
                    }
                    if next == 0x1e {
                        extra -= 3;
                        pct = scale(0x199a, pct);
                    }
                }
            }
            if run_consonant && (cf & 0x100) != 0 {
                if (nfeat & 0x100) != 0 && stress_b < 0xa0 {
                    k = 0x2ccd;
                    if (cf & 0x80) == 0 || (nflags_lo & 4) == 0 {
                        inh -= inh >> 2;
                    } else {
                        k = 0x6000;
                    }
                    if cur == 0x29 || cur == 0x27 {
                        if (nfeat & 0x40) != 0 {
                            k = 0x2000;
                        }
                        if next == 0x2b {
                            m.w16(aux + 0x30, 2);
                            break 'done;
                        }
                    }
                    pct = scale(k, pct);
                }
                if (pfeat & 0x100) != 0 && (pflags & 0x1e0) < 0xa0 {
                    inh -= inh >> 2;
                    k = 0x2ccd;
                    if (cf & 0x40) != 0 {
                        if prev == 0x29 {
                            k = 0x2667;
                        }
                        if (pfeat & 0x80) != 0 && st == 0 {
                            k = 0x666;
                        }
                    }
                    pct = scale(k, pct);
                }
            }

            if (cf & 8) != 0 && (pfeat & 2) == 0 && (pfeat & 0x40) != 0 {
                extra += 3;
            }
            if (cf & 4) != 0 {
                if prev == 0 {
                    extra += 3;
                }
                if (pfeat & 0x400) != 0 && (pfeat & 0x80) == 0 && extra as i16 == 0 {
                    extra = 3;
                }
            }
            let mut p = pct;
            if m.s(ph + 0x11ae) < 10 && min != inh {
                p = wrap(pct + 0x1e);
            }
            if (cf & 0x200) != 0 && (pfeat & 0x20) != 0 {
                p = scale(0x2ccd, p);
            }
            if cur == 0x27 {
                if (fl & 0x18) == 0 && 0x5f < stress_b && st == 0 {
                    p = scale(0x2667, p);
                }
            } else if cur == 1 && 0x40 < stress_b && st == 0 && (fl & 0x18) == 0 {
                p = scale(0x6000, p);
            }
            if (cf & 0x40) != 0 && (pfeat & 0x40) != 0 && (nfeat & 0x20) != 0 && 0x40 < stress_b {
                inh >>= 1;
                p = wrap((p << 12) >> 14);
            }
            if next == 0x38 {
                p = scale(0x1666, p);
            }
            if next == 0x2f
                && (cf & 4) != 0
                && (feat(m, m.s(at(ph + 0x306, i, 2))) & 4) != 0
                && i + 2 <= m.s(ph + 0x11ae)
                && 0x32 < (p as u16)
            {
                p = scale(0x1666, p);
            }
            if (nfeat & 0x40) != 0 && (cf & 0x100) != 0 {
                inh >>= 1;
                p = wrap((p << 13) >> 14);
            }
            if m.s(at(ph + 0x304, i, 2)) == 0x21 {
                p = scale(0x2667, p);
            }

            m.w16(aux + 0x26, st);
            let inh = wrap((((min - inh) * p) >> 7) + inh);
            let rate = m.s(aux + 0x2e);
            m.w16(aux + 0x30, inh);
            let mut extra = wrap(extra);
            if rate != 0xb4 && inh != 0 {
                m.w16(aux + 0x30, wrap((m.s(aux + 0x24) * inh) >> 14) + 1);
                extra = wrap((m.s(aux + 0x22) * extra) >> 14);
            }
            m.w16(aux + 0x30, m.s(aux + 0x30) + extra);
            if m.s(aux + 0x30) < 0 {
                m.w16(aux + 0x30, 1);
            }
            let cur2 = m.s(at(ph + 0x302, i, 2));
            if cur2 != 0 {
                foot_dur = wrap(foot_dur + m.s(aux + 0x30));
            }
            if (cf & 0x10) != 0 && cur2 != 0 {
                foot_vowels += 1;
            }
            if (fl & 1) != 0 {
                let q = m.s(ph + 0x2f6) - (foot_dur >> 1);
                let mut adj = match foot_vowels as i16 {
                    1 => wrap(q),
                    2 => wrap(q >> 1),
                    3 => wrap(wrap(q >> 3) * 3),
                    4 => wrap(q >> 2),
                    5 => wrap(q >> 3),
                    _ => wrap(q >> 4),
                };
                if rate < 0xfb {
                    adj = 0;
                } else if rate < 0x145 {
                    adj >>= 2;
                } else {
                    adj >>= 1;
                }
                if m.s(at(m.r32(ph + 0x1428), i, 2)) != 0 || m.s(ph + 0x1440) == 4 {
                    adj = 0;
                }
                let mut j = i;
                while last_primary != j {
                    if (feat(m, m.s(at(ph + 0x302, j, 2))) & 0x10) != 0 {
                        let a = at(ph + 0xa48, j, 2);
                        m.w16(a, m.s(a) + adj);
                        if m.s(a) < 7 {
                            m.w16(a, 6);
                        }
                    }
                    j -= 1;
                }
                foot_dur = 0;
                foot_vowels = 0;
                last_primary = i;
            }
        }
        if m.s(aux + 0x30) < 1 {
            m.w16(aux + 0x30, 1);
        }
        m.w16(at(ph + 0xa48, i, 2), m.s(aux + 0x30));
        m.w32(ph + 0x1fbc, (m.i(ph + 0x1fbc) + m.s(aux + 0x30) * 0x47) as u32);
        i += 1;
    }
}
