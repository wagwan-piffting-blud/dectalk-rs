use super::*;

const FOLD: u32 = 0x1c02fb80;
const UPPER: u32 = 0x1c02fc80;
const CCLASS: u32 = 0x1c02fd80;
const HOMOGRAPH_RULES: u32 = 0x1c0301ac;
const HOMOGRAPH_RULES_END: u32 = 0x1c03035c;
const SUFFIX_FIRST: u32 = 0x1c03091c;
const SUFFIX_DEFAULT: u32 = 0x1c030b08;
const SUFFIX_BASE: u32 = 0x1c030b0c;
const STRESS_MASKS: u32 = 0x1c032e18;

#[inline]
fn cclass(m: &Mem, c: u8) -> u8 {
    m.r8(CCLASS + c as u32)
}

#[inline]
fn upper(m: &Mem, c: u8) -> u8 {
    m.r8(UPPER + c as u32)
}

#[inline]
fn fold(m: &Mem, c: u8) -> u8 {
    m.r8(FOLD + c as u32)
}

/// Word dictionary lookup (1c006990) wrapped with miss/hit debug logging.
pub fn f_1c00f4c0(m: &mut Mem, p1: u32, p2: u32, p3: u32, p4: u32) -> u32 {
    f_1c006990(m, p1, p2, p3, p4)
}

/// Copy a word (8-byte records) into lts+0x438 and look it up in the user, abbreviation and
/// main dictionaries, falling back to suffix stripping; returns 0 miss, 1 hit, 2 hit with period.
pub fn f_1c006990(m: &mut Mem, p1: u32, p2: u32, p3: u32, p4: u32) -> u32 {
    let ctx = p1;
    let mode = p4;
    let lts = m.r32(ctx + 0x10);
    let kernel = m.r32(ctx + 8);
    m.w32(lts + 0x8f0, 0);
    m.w32(lts + 0x8f4, 0);
    m.w32(lts + 0x4b8, 0);
    m.w32(lts + 0x1128, 0);

    let kmode = m.r32(kernel + 0x208);
    if mode == 4 || kmode == 2 || kmode == 0x10 {
        let c = m.r8(p2);
        m.w8(lts + 0x439, 0);
        m.w8(lts + 0x438, c);
        m.w32(lts + 0x8f0, 1);
        if m.r32(kernel + 0x54) != 0 && f_1c007330(m, ctx, 0) == 1 {
            return m.r32(lts + 0x1128);
        }
        if m.r32(kernel + 0x1c) == 0 {
            return 0;
        }
        f_1c006c10(m, ctx);
        return m.r32(lts + 0x1128);
    }

    if mode == 1 {
        m.w32(lts + 0x8f4, 1);
    } else if mode == 3 {
        m.w32(lts + 0x8f0, 1);
    }
    if p2 == p3 {
        return 0;
    }

    let mut len: u32 = 0;
    let mut src = p2;
    while src < p3 {
        let c = m.r8(src);
        let dst = lts + 0x438 + len;
        src = src.wrapping_add(8);
        m.w8(dst, c);
        if m.r32(lts + 0x4b8) == 0 && cclass(m, m.r8(dst)) & 0x40 != 0 {
            m.w32(lts + 0x4b8, dst);
        }
        len += 1;
    }
    let end = lts + 0x438 + len;
    let last = lts + 0x437 + len;
    m.w8(end, 0);

    if m.r32(kernel + 0x54) != 0 {
        if mode == 1 {
            m.w8(end, 0x2e);
            m.w8(lts + 0x439 + len, 0);
            let r = f_1c007330(m, ctx, 0);
            m.w8(end, 0);
            if r == 1 {
                m.w32(lts + 0x1128, 2);
                return 2;
            }
            if f_1c007330(m, ctx, 0) == 1 {
                m.w32(lts + 0x1128, 1);
                return 1;
            }
        } else if f_1c007330(m, ctx, 0) == 1 {
            m.w32(lts + 0x1128, 1);
            return 1;
        }
    }

    if m.r32(kernel + 0x8c) != 0 {
        if mode == 1 {
            m.w8(end, 0x2e);
            m.w8(lts + 0x439 + len, 0);
            let r = f_1c007330(m, ctx, 1);
            m.w8(end, 0);
            if r == 1 {
                m.w32(lts + 0x1128, 2);
                return 2;
            }
        }
        if f_1c007330(m, ctx, 1) == 1 {
            m.w32(lts + 0x1128, 1);
            return 1;
        }
    }

    if m.r32(kernel + 0x1c) == 0 {
        return 0;
    }
    if len == 1 && fold(m, m.r8(lts + 0x438)) == 0x61 {
        return 0;
    }
    let r = f_1c006c10(m, ctx);
    if r != 0 {
        return m.r32(lts + 0x1128);
    }
    if (len as i32) <= 2 {
        return 0;
    }
    m.w32(lts + 0x8f4, r);
    f_1c009c50(m, ctx, last, 1)
}

/// Binary search the main dictionary for the word at lts+0x438 (with capitalised-homograph
/// preference) and emit its phonemes; returns 1 on hit.
pub fn f_1c006c10(m: &mut Mem, p1: u32) -> u32 {
    let ctx = p1;
    let kernel = m.r32(ctx + 8);
    let lts = m.r32(ctx + 0x10);
    let sp = m.sp();
    let idx_slot = m.alloca(4);
    let ent_slot = m.alloca(4);

    let count = m.i(kernel + 0x1c);
    let mut pos = count >> 1;
    let mut step = pos >> 1;
    let mut r = f_1c006f80(m, ctx, pos as u32, idx_slot, ent_slot);
    if r != 1 {
        loop {
            if r == 0xffff {
                pos = pos.wrapping_add(step);
            } else {
                pos = pos.wrapping_sub(step);
            }
            if step == 0 {
                break;
            }
            step >>= 1;
            r = f_1c006f80(m, ctx, pos as u32, idx_slot, ent_slot);
            if r == 1 {
                break;
            }
        }
        if r == 0xffff {
            loop {
                pos = pos.wrapping_add(1);
                r = f_1c006f80(m, ctx, pos as u32, idx_slot, ent_slot);
                if r != 0xffff {
                    break;
                }
            }
        } else if r == 0xfffe {
            loop {
                pos = pos.wrapping_sub(1);
                r = f_1c006f80(m, ctx, pos as u32, idx_slot, ent_slot);
                if r != 0xfffe {
                    break;
                }
            }
        }
    }
    if r != 1 {
        m.set_sp(sp);
        return 0;
    }

    let cap = cclass(m, m.r8(lts + 0x438)) & 2 != 0 && cclass(m, m.r8(lts + 0x439)) & 1 != 0;
    let first = m.r8(m.r32(ent_slot) + 4);
    let homograph = if cap {
        cclass(m, first) & 1 != 0
    } else {
        cclass(m, first) & 2 != 0
    };
    if homograph {
        let alt = if cap { pos.wrapping_sub(1) } else { pos.wrapping_add(1) };
        if f_1c006f80(m, ctx, alt as u32, idx_slot, ent_slot) != 1 {
            f_1c006f80(m, ctx, pos as u32, idx_slot, ent_slot);
        }
    }

    let v = m.r32(kernel + 0x20c) & 0xfffffffd;
    m.w32(kernel + 0x20c, v);
    let slot = lts
        .wrapping_add(m.r32(lts + 0x8bc).wrapping_mul(4))
        .wrapping_add(0x4bc);
    let cur = m.r32(slot);
    if cur == 0 {
        let f = m.r32(m.r32(ent_slot));
        m.w32(slot, f);
    } else if m.r32(m.r32(ent_slot)) & 0x80000000 != 0 {
        m.w32(slot, cur | 0x80000000);
    }
    if m.r32(m.r32(ent_slot)) & 0x2001000 == 0x2001000 {
        f_1c00ef80(m, ctx, 0x70);
    }
    let f = m.r32(m.r32(ent_slot));
    if (f & 0x2020000 == 0x2020000 || f == 0x20000) && m.r32(lts + 0x8f0) == 0 {
        f_1c00ef80(m, ctx, 0x71);
    }
    loop {
        let idx = m.r32(idx_slot).wrapping_add(1);
        m.w32(idx_slot, idx);
        let c = m.r8(idx.wrapping_add(m.r32(ent_slot)).wrapping_add(4));
        if c == 0 {
            break;
        }
        f_1c00ef80(m, ctx, c as u32);
    }
    m.set_sp(sp);
    1
}

/// Compare the word at lts+0x438 against main dictionary entry p2; returns 1 hit (stores the
/// entry into *p4 and the phoneme offset into *p3), 0xffff look higher, 0xfffe look lower.
pub fn f_1c006f80(m: &mut Mem, p1: u32, p2: u32, p3: u32, p4: u32) -> u32 {
    let ctx = p1;
    let pos = p2 as i32;
    let kernel = m.r32(ctx + 8);
    let lts = m.r32(ctx + 0x10);
    let last = m.i(kernel + 0x1c).wrapping_sub(1);
    if pos < 0 {
        m.w32(lts + 0x1128, 0);
        return 0xffff;
    }
    if pos > last {
        m.w32(lts + 0x1128, 0);
        return 0xfffe;
    }
    let ent = m.r32(m.r32(kernel).wrapping_add(p2.wrapping_mul(4)));
    m.w32(p4, ent);
    m.w32(lts + 0x1128, 1);

    let e = m.r32(p4);
    let mut i: u32 = 0;
    let mut dc = m.r8(e + 4);
    while dc != 0 {
        let ic = m.r8(lts + 0x438 + i);
        if ic == 0 {
            if m.r32(lts + 0x8f4) != 0 && m.r8(e + i + 4) == 0x2e && m.r8(e + i + 5) == 0 {
                m.w32(lts + 0x1128, 2);
                break;
            }
            m.w32(lts + 0x1128, 0);
            return 0xfffe;
        }
        let d = m.r8(e + i + 4);
        if ic != d && (cclass(m, d) & 1 == 0 || ic != upper(m, d)) {
            m.w32(lts + 0x1128, 0);
            if pos == 0 {
                return 0xffff;
            }
            if pos == last {
                return 0xfffe;
            }
            let ent = m.r32(p4);
            return f_1c007250(m, ctx, ent);
        }
        dc = m.r8(e + i + 5);
        i += 1;
    }
    if m.r8(lts + 0x438 + i) != 0 {
        m.w32(lts + 0x1128, 0);
        return 0xffff;
    }
    if m.r32(lts + 0x1128) == 2 {
        i += 1;
    }
    if m.r32(m.r32(p4)) & 0x80000000 != 0 {
        let alt = f_1c0083b0(m, ctx, p2);
        m.w32(p4, alt);
    }
    m.w32(p3, i);
    1
}

/// Case-folded ordering of the lts+0x438 word against main dict entry p2 after a mismatch:
/// 0xffff look higher, 0xfffe look lower.
pub fn f_1c007250(m: &mut Mem, p1: u32, p2: u32) -> u32 {
    let lts = m.r32(p1 + 0x10);
    let mut dl: u8 = 0;
    let mut i: u32 = 0;
    let mut al = m.r8(lts + 0x438);
    while al != 0 {
        dl = upper(m, m.r8(p2 + i + 4));
        if upper(m, al) != dl {
            break;
        }
        al = m.r8(lts + 0x439 + i);
        i += 1;
    }
    let a = m.r8(lts + 0x438 + i);
    if a == 0 && m.r8(p2 + i + 4) == 0 {
        return 0xffff;
    }
    if upper(m, a) <= dl {
        0xfffe
    } else {
        0xffff
    }
}

/// Binary search the user dictionary (p2 == 0) or abbreviation dictionary (p2 != 0);
/// emits the phonemes and returns 1 on hit, 0 on miss.
pub fn f_1c007330(m: &mut Mem, p1: u32, p2: u32) -> u32 {
    let kernel = m.r32(p1 + 8);
    let mut hi = if p2 as u16 == 0 {
        m.i(kernel + 0x54)
    } else {
        m.i(kernel + 0x8c)
    }
    .wrapping_sub(1);
    let mut lo: i32 = 0;
    if hi < 0 {
        return 0;
    }
    loop {
        let mid = hi.wrapping_add(lo) >> 1;
        let r = f_1c0073a0(m, p1, mid as u32, p2);
        if r == 0xfffe {
            hi = mid.wrapping_sub(1);
        } else if r == 0xffff {
            lo = mid.wrapping_add(1);
        } else {
            return 1;
        }
        if lo > hi {
            return 0;
        }
    }
}

/// Compare the lts+0x438 word against user/abbreviation dict entry p2 (dict chosen by p3);
/// on exact match emits the entry's phonemes and returns 1, else 0xffff/0xfffe direction.
pub fn f_1c0073a0(m: &mut Mem, p1: u32, p2: u32, p3: u32) -> u32 {
    let ctx = p1;
    let kernel = m.r32(ctx + 8);
    let lts = m.r32(ctx + 0x10);
    let table = if p3 as u16 == 0 {
        m.r32(kernel + 0x38)
    } else {
        m.r32(kernel + 0x70)
    };
    let w = m.r32(table.wrapping_add(p2.wrapping_mul(4))).wrapping_add(4);
    let mut i: u32 = 0;
    let mut al = m.r8(w);
    while al != 0 {
        let ic = m.r8(lts + 0x438 + i);
        if ic as u32 != sx8(al as u32) {
            if ic == 0 {
                return 0xfffe;
            }
            if cclass(m, al) & 1 == 0 || ic != m.r8(UPPER.wrapping_add(sx8(al as u32))) {
                return f_1c007470(m, lts, w);
            }
        }
        al = m.r8(w + i + 1);
        i += 1;
    }
    if m.r8(lts + 0x438 + i) != 0 {
        return 0xffff;
    }
    let mut p = w + i + 1;
    loop {
        let c = m.r8(p);
        if c == 0 {
            break;
        }
        f_1c00ef80(m, ctx, sx8(c as u32));
        p += 1;
    }
    1
}

/// Case-folded ordering of the lts (p1) +0x438 word against user-dict word p2:
/// 0xffff look higher, 0xfffe look lower.
pub fn f_1c007470(m: &mut Mem, p1: u32, p2: u32) -> u32 {
    let mut dl: u8 = 0;
    let mut i: u32 = 0;
    let mut al = m.r8(p1 + 0x438);
    while al != 0 {
        dl = m.r8(UPPER.wrapping_add(sx8(m.r8(p2 + i) as u32)));
        if upper(m, al) != dl {
            break;
        }
        al = m.r8(p1 + 0x439 + i);
        i += 1;
    }
    let bl = upper(m, m.r8(p1 + 0x438 + i));
    0xfffe + (dl < bl) as u32
}

/// Resolve a homograph pair around main dict index p2 using the form-class context in the
/// lts state and the rule table at 0x1c0301a8; returns the chosen entry pointer.
pub fn f_1c0083b0(m: &mut Mem, p1: u32, p2: u32) -> u32 {
    let lts = m.r32(p1 + 0x10);
    let kernel = m.r32(p1 + 8);
    let table = m.r32(kernel);
    let at = |k: u32| table.wrapping_add(k.wrapping_mul(4));
    let same_word = |m: &Mem, a: u32, b: u32| -> bool {
        let mut k: u32 = 0;
        let mut c = m.r8(b + 4);
        while c != 0 {
            if m.r8(a + 4 + k) != c {
                break;
            }
            c = m.r8(b + 5 + k);
            k += 1;
        }
        m.r8(a + k + 4) == 0 && m.r8(b + k + 4) == 0
    };

    let mut not_found = true;
    let mut chosen = m.r32(at(p2.wrapping_sub(1)));
    let mut other = m.r32(at(p2));
    let mut matched_prev = false;
    if m.r32(other) & 0x80000000 != 0 && same_word(m, chosen, other) {
        not_found = false;
        matched_prev = true;
    }
    if !matched_prev {
        other = m.r32(at(p2.wrapping_add(1)));
        chosen = m.r32(at(p2));
        let of = m.r32(other);
        if of & 0x80000000 != 0 && same_word(m, chosen, other) {
            if of & 0x2000000 != 0 {
                chosen = other;
            }
            not_found = false;
        }
    }

    let cf = m.r32(chosen);
    if cf & 0x2000000 != 0 {
        m.w32(chosen, cf & 0xfdffffff);
    }
    if not_found {
        return chosen;
    }

    let kf = m.r32(kernel + 0x20c);
    if kf & 1 != 0 {
        m.w32(kernel + 0x20c, kf & !1);
        return chosen;
    }
    if kf & 2 != 0 {
        m.w32(kernel + 0x20c, kf & !2);
        return other;
    }

    let n = m.r32(lts + 0x8bc);
    if n == 1 {
        return chosen;
    }
    let ctx_slot = lts.wrapping_add(n.wrapping_mul(4)).wrapping_add(0x4b8);
    if m.r32(ctx_slot) == 0 {
        m.w32(ctx_slot, 0x400);
    }

    let mut rule: u32 = 0;
    let mut rec = HOMOGRAPH_RULES;
    while (rec as i32) < (HOMOGRAPH_RULES_END as i32) {
        let req = m.r32(rec - 4);
        let n = m.r32(lts + 0x8bc);
        let cur = m.r32(lts.wrapping_add(n.wrapping_mul(4)).wrapping_add(0x4bc));
        let applies = if req != 0 && cur & req != req {
            false
        } else {
            let prev_mask = m.r32(rec);
            if prev_mask == 0 {
                true
            } else {
                let n = m.r32(lts + 0x8bc);
                let prev = m.r32(lts.wrapping_add(n.wrapping_mul(4)).wrapping_add(0x4b8));
                if prev & prev_mask != 0 {
                    true
                } else if (n as i32) < 3 || prev & 2 == 0 {
                    false
                } else {
                    let prev2 = m.r32(lts.wrapping_add(n.wrapping_mul(4)).wrapping_add(0x4b4));
                    prev2 & prev_mask != 0
                }
            }
        };
        if applies {
            let mut pick = None;
            let a = m.r32(rec + 4);
            if a != 0 {
                if m.r32(chosen) & a != 0 {
                    pick = Some(chosen);
                } else if m.r32(other) & a != 0 {
                    pick = Some(other);
                }
            }
            if pick.is_none() {
                let b = m.r32(rec + 8);
                if b != 0 {
                    if m.r32(other) & b != 0 {
                        pick = Some(chosen);
                    } else if m.r32(chosen) & b != 0 {
                        pick = Some(other);
                    }
                }
            }
            if let Some(p) = pick {
                if (rule as i32) < 0x1b && m.r32(0x1c0301a8 + rule * 16) != 0 {
                    let n = m.r32(lts + 0x8bc);
                    let f = m.r32(p);
                    m.w32(lts.wrapping_add(n.wrapping_mul(4)).wrapping_add(0x4bc), f);
                }
                return p;
            }
        }
        rec += 0x10;
        rule += 1;
    }
    chosen
}

/// Suffix-stripping lookup: match suffix rules backwards from p2 (last char of the word in
/// lts+0x438), rewrite the suffix, re-look up the stem, and emit suffix phonemes on hit.
pub fn f_1c009c50(m: &mut Mem, p1: u32, p2: u32, p3: u32) -> u32 {
    let ctx = p1;
    let kernel = m.r32(ctx + 8);
    let lts = m.r32(ctx + 0x10);
    let mut result: u32 = 0;
    let c = fold(m, m.r8(p2));
    let mut off = if (0x61..=0x7a).contains(&c) {
        m.r32(SUFFIX_FIRST + c as u32 * 4)
    } else {
        m.r32(SUFFIX_DEFAULT)
    };

    while off != 0xffff {
        let rec = SUFFIX_BASE.wrapping_add(off);
        let mut pat = rec + 8;
        let mut cp = p2;
        let mut d = m.r8(pat);
        while d != 0xfe && d != 0xfd {
            if fold(m, m.r8(cp)) != d || cp == m.r32(lts + 0x4b8) {
                break;
            }
            d = m.r8(pat + 1);
            cp = cp.wrapping_sub(1);
            pat += 1;
        }
        if p3 as u16 != 0 && m.r8(pat) == 0xfd {
            let n = m.r32(lts + 0x8bc);
            let f = m.r32(rec + 4);
            m.w32(lts.wrapping_add(n.wrapping_mul(4)).wrapping_add(0x4bc), f);
            return 0;
        }
        let d = m.r8(pat);
        pat += 1;
        if d == 0xfe {
            let mut saved = Vec::new();
            let mut k = 0;
            loop {
                let b = m.r8(lts + 0x438 + k);
                saved.push(b);
                if b == 0 {
                    break;
                }
                k += 1;
            }

            let stem_end = cp;
            let mut op = m.r8(pat);
            while op != 0xff {
                pat += 1;
                if op == 0xfc {
                    let mut q = stem_end;
                    while m.r8(pat) == fold(m, m.r8(q)) {
                        pat += 1;
                        q = q.wrapping_sub(1);
                    }
                    let d = m.r8(pat);
                    pat += 1;
                    if d == 0xfb {
                        let mut dst = q.wrapping_add(1);
                        let mut b = m.r8(pat);
                        while b != 0xfa {
                            m.w8(dst, b);
                            pat += 1;
                            dst = dst.wrapping_add(1);
                            b = m.r8(pat);
                        }
                        pat += 1;
                        m.w8(dst, 0);
                        if m.r8(pat) == 0xf9 {
                            pat += 1;
                            result = f_1c009c50(m, ctx, dst, p3);
                        } else {
                            let n = m.r32(lts + 0x8bc);
                            let f = m.r32(rec + 4);
                            m.w32(lts.wrapping_add(n.wrapping_mul(4)).wrapping_add(0x4bc), f);
                            let sel = m.r32(kernel + 0xf8);
                            if m.r32(kernel.wrapping_add(sel.wrapping_mul(4)).wrapping_add(0x54)) != 0 {
                                result = f_1c007330(m, ctx, 0);
                            }
                            if m.r32(kernel + 0x1c) != 0 && result != 1 {
                                result = f_1c006c10(m, ctx);
                            }
                        }
                        if result == 1 {
                            f_1c009f30(m, ctx, pat);
                            return 1;
                        }
                        let n = m.r32(lts + 0x8bc);
                        m.w32(lts.wrapping_add(n.wrapping_mul(4)).wrapping_add(0x4bc), 0);
                        for (k, &b) in saved.iter().enumerate() {
                            m.w8(lts + 0x438 + k as u32, b);
                        }
                    }
                }
                op = m.r8(pat);
            }
        }
        off = m.r32(rec);
    }
    0
}

/// Emit the phoneme tail of a suffix rule: skip 0xf8 mask/value guards (tested against the
/// stress mask table indexed by lts+0x436) until one matches, then emit up to 0xf7.
pub fn f_1c009f30(m: &mut Mem, p1: u32, p2: u32) -> u32 {
    let lts = m.r32(p1 + 0x10);
    let mut p = p2;
    let mut al = m.r8(p);
    if al == 0xff {
        return 0;
    }
    loop {
        p += 1;
        if al == 0xf8 {
            let idx = m.s(lts + 0x436);
            let mask = m.r16(STRESS_MASKS.wrapping_add((idx as u32).wrapping_mul(2))) as u16 as u32;
            let v = mask & m.r8(p) as u32;
            p += 1;
            let want = m.r8(p) as u32;
            p += 1;
            if v == want {
                break;
            }
        }
        al = m.r8(p);
        if al == 0xff {
            return 0;
        }
    }
    let mut c = m.r8(p);
    while c != 0xf7 {
        p += 1;
        f_1c00ef80(m, p1, c as u32);
        c = m.r8(p);
    }
    0
}
