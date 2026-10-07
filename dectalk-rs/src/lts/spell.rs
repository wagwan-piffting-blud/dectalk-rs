use super::*;

const CLS: u32 = 0x1c02fd80;
const DIGIT_NAMES: u32 = 0x1c032a10;
const MONTHS: u32 = 0x1c0329c4;

/// Byte class table lookup with a sign-extended character index.
fn cls(m: &Mem, c: i32) -> u8 {
    m.r8(CLS.wrapping_add(c as u32))
}

fn tbl(m: &Mem, base: u32, idx: i32) -> u32 {
    m.r32(base.wrapping_add((idx as u32).wrapping_mul(4)))
}

fn w(m: &Mem, a: u32) -> u16 {
    m.r16(a) as u16
}

/// Index 0..11 of the month whose 3-letter abbreviation matches, 12 if none.
fn month_index(m: &Mem, c1: u16, c2: u16, c3: u16) -> u32 {
    let mut i = 0;
    while i < 12 {
        let s = m.r32(MONTHS + i * 4);
        if c1 == m.r8(s) as i8 as i16 as u16
            && c2 == m.r8(s + 1) as i8 as i16 as u16
            && c3 == m.r8(s + 2) as i8 as i16 as u16
        {
            break;
        }
        i += 1;
    }
    i
}

/// Currency handler: "$" / pound sign followed by an optional sign and an amount.
pub fn f_1c00d540(m: &mut Mem, p1: u32, p2: u32, p3: u32) -> u32 {
    let lts = m.r32(p1 + 0x10);
    let sym = w(m, p2);
    if sym != 0x24 && sym != 0xa3 {
        return 0;
    }
    let dollar = sym == 0x24;
    let unit = if dollar { 0x1c032c7c } else { 0x1c032c74 };
    let mut start = p2 + 8;
    if start == p3 {
        return 2;
    }
    let c = w(m, start);
    if c == 0x2d || c == 0x2b || c == 0xb1 {
        if m.r32(lts + 0x1118) != 0 {
            return 2;
        }
        start += 8;
        m.w32(lts + 0x1118, sx16(c as u32));
        if start == p3 {
            return 2;
        }
    }
    let sp = m.sp();
    let st = m.alloca(0x14);
    let end = f_1c00c290(m, p1, start, p3, st);
    let (int_s, int_e, frac_s, frac_e, exp) =
        (m.r32(st), m.r32(st + 4), m.r32(st + 8), m.r32(st + 0xc), m.r32(st + 0x10));
    m.set_sp(sp);
    if exp != 0 || end != p3 {
        return 2;
    }
    let mut lookahead = false;
    f_1c00c510(m, lts);
    if f_1c00f100(m, lts + 0x900) != 0 {
        let next = lts + 0xd14;
        lookahead = true;
        f_1c00c820(m, p1, next);
        let hit = f_1c00c440(m, lts, next, 0x1c032928);
        if hit != 0 {
            f_1c0086e0(m, p1, m.r32(lts + 0x1118));
            let r = f_1c008c10(m, p1, start, p3, 0);
            m.w32(lts + 0x1124, r);
            f_1c00ef80(m, p1, m.r32(lts + 0x1120));
            f_1c00ef50(m, p1, hit);
            f_1c00ef50(m, p1, unit);
            f_1c00ef80(m, p1, 0x2a);
            return 3;
        }
    }
    f_1c0086e0(m, p1, m.r32(lts + 0x1118));
    let finish = |m: &mut Mem| -> u32 {
        if lookahead {
            f_1c00ef80(m, p1, m.r32(lts + 0x1120));
            f_1c00ef10(m, lts + 0x914, lts + 0xd14);
            1
        } else {
            3
        }
    };
    if frac_s != 0 && (frac_e.wrapping_sub(frac_s) & !7) != 0x18 {
        let r = f_1c008c10(m, p1, start, p3, 0);
        m.w32(lts + 0x1124, r);
        f_1c00ef50(m, p1, unit);
        if m.r32(lts + 0x1124) != 0 {
            f_1c00ef80(m, p1, 0x2a);
        }
        return finish(m);
    }
    if int_s != 0 {
        let r = f_1c008c10(m, p1, int_s, int_e, 0);
        m.w32(lts + 0x1124, r);
        f_1c00ef50(m, p1, unit);
        if m.r32(lts + 0x1124) != 0 {
            f_1c00ef80(m, p1, 0x2a);
        }
        if frac_s == 0 {
            return finish(m);
        }
        if w(m, frac_s + 8) == 0x30 && w(m, frac_s + 0x10) == 0x30 {
            return finish(m);
        }
        f_1c00ef50(m, p1, 0x1c032c84);
    }
    let mut cents = frac_s + 8;
    if w(m, cents) == 0x30 {
        cents += 8;
    }
    let r = f_1c008c10(m, p1, cents, frac_e, 0);
    m.w32(lts + 0x1124, r);
    f_1c00ef80(m, p1, 0x6f);
    if dollar {
        f_1c00ef50(m, p1, 0x1c032c6c);
        if m.r32(lts + 0x1124) != 0 {
            f_1c00ef80(m, p1, 0x29);
        }
    } else {
        f_1c00ef50(m, p1, 0x1c032c58);
    }
    finish(m)
}

/// Date (DD-MON-YY[YY]) and clock-time (H:MM[:SS][.ff]) handler.
pub fn f_1c00d8c0(m: &mut Mem, p1: u32, p2: u32, p3: u32) -> u32 {
    let lts = m.r32(p1 + 0x10);
    if m.r32(lts + 0x1118) != 0 {
        return 0;
    }
    if f_1c009310(m, p2, p3) != 0 {
        f_1c0094a0(m, p1, p2, p3);
        return 3;
    }
    if f_1c0097f0(m, lts, p2, p3) == 0 {
        return 0;
    }
    f_1c009950(m, p1, p2, p3);
    f_1c00c510(m, lts);
    if f_1c00f100(m, lts + 0x900) == 0 {
        return 3;
    }
    f_1c00ef80(m, p1, m.r32(lts + 0x1120));
    let next = lts + 0xd14;
    f_1c00c820(m, p1, next);
    let mut end = next;
    while w(m, end) != 0 {
        end += 8;
    }
    if f_1c009900(m, next, end) != 0 {
        f_1c009b00(m, p1, next, end);
        f_1c00c550(m, p1, 0);
        f_1c00c820(m, p1, lts + 0x914);
        return 1;
    }
    f_1c00ef10(m, lts + 0x914, next);
    1
}

/// Fraction handler (N/D, optional trailing %).
pub fn f_1c00d9d0(m: &mut Mem, p1: u32, p2: u32, p3: u32) -> u32 {
    let lts = m.r32(p1 + 0x10);
    if f_1c009650(m, p2, p3) == 0 {
        return 0;
    }
    f_1c0086e0(m, p1, m.r32(lts + 0x1118));
    f_1c009710(m, p1, p2, p3);
    3
}

/// Plain number handler: cardinals, ordinals, years, and number+suffix (%, cents, degrees, 's, s).
pub fn f_1c00da20(m: &mut Mem, p1: u32, p2: u32, p3: u32, p4: u32) -> u32 {
    let lts = m.r32(p1 + 0x10);
    let c = w(m, p4);
    if c != 0x2e && c != 0xbc && c != 0xbd && cls(m, c as i16 as i32) & 0x10 == 0 {
        return 0;
    }
    m.w32(lts + 0x8fc, 3);
    let sp = m.sp();
    let st = m.alloca(0x18);
    let end = f_1c00c290(m, p1, p4, p3, st);
    if end == p3 {
        m.set_sp(sp);
        let n = m.r32(lts + 0x8bc);
        m.w32(lts.wrapping_add(n.wrapping_mul(4)).wrapping_add(0x4bc), 1);
        if m.r32(lts + 0x1118) == 0 && f_1c00eeb0(m, p2, p3) != 0 {
            f_1c008a20(m, p1, p2);
            m.w32(lts + 0x1124, 1);
        } else {
            f_1c0086e0(m, p1, m.r32(lts + 0x1118));
            let r = f_1c008c10(m, p1, p4, p3, 0);
            m.w32(lts + 0x1124, r);
        }
        f_1c00c510(m, lts);
        if f_1c00f100(m, lts + 0x900) == 0 {
            return 3;
        }
        f_1c00ef80(m, p1, m.r32(lts + 0x1120));
        let next = lts + 0xd14;
        f_1c00c820(m, p1, next);
        let mut nend = next;
        while w(m, nend) != 0 {
            nend += 8;
        }
        if f_1c009900(m, next, nend) != 0 {
            f_1c009b00(m, p1, next, nend);
        } else {
            let hit = if f_1c00ed60(m, lts) != 0 {
                f_1c00c440(m, lts, next, 0x1c03264c)
            } else {
                0
            };
            if hit == 0 {
                f_1c00ef10(m, lts + 0x914, next);
                return 1;
            }
            m.w32(lts + 0x8fc, 3);
            let mut s = hit;
            if m.r32(lts + 0x1124) != 0 {
                // plural form is the second NUL-terminated string of the entry
                loop {
                    let b = m.r8(s);
                    s += 1;
                    if b == 0 {
                        break;
                    }
                }
            }
            f_1c00ef50(m, p1, s);
            f_1c00f140(m, p1);
        }
        f_1c00c550(m, p1, 0);
        f_1c00c820(m, p1, lts + 0x914);
        return 1;
    }
    if end + 0x10 == p3 {
        if w(m, end) == 0x27 && w(m, end + 8) == 0x73 {
            m.set_sp(sp);
            f_1c0086e0(m, p1, m.r32(lts + 0x1118));
            let r = f_1c008c10(m, p1, p4, end, 0);
            m.w32(lts + 0x1124, r);
            f_1c00f600(m, p1);
            return 3;
        }
        if m.r32(lts + 0x1118) != 0 || f_1c00ee10(m, p1, st) == 0 {
            m.set_sp(sp);
            return 0;
        }
        m.set_sp(sp);
        let r = f_1c008c10(m, p1, p2, end, 1);
        m.w32(lts + 0x1124, r);
        return 3;
    }
    m.set_sp(sp);
    if end + 8 != p3 {
        return 0;
    }
    let suf = m.s(end);
    if suf == 0x73 {
        f_1c0086e0(m, p1, m.r32(lts + 0x1118));
        let r = f_1c008c10(m, p1, p4, end, 0);
        m.w32(lts + 0x1124, r);
        f_1c00f600(m, p1);
        return 3;
    }
    if suf != 0x25 && suf != 0xa2 && suf != 0xb0 {
        return 0;
    }
    f_1c0086e0(m, p1, m.r32(lts + 0x1118));
    let r = f_1c008c10(m, p1, p4, end, 0);
    m.w32(lts + 0x1124, r);
    if suf == 0x25 {
        f_1c00ef50(m, p1, 0x1c032c60);
        return 3;
    }
    f_1c00ef80(m, p1, 0x6f);
    if suf == 0xa2 {
        f_1c00ef50(m, p1, 0x1c032c6c);
        if m.r32(lts + 0x1124) != 0 {
            f_1c00ef80(m, p1, 0x29);
        }
    } else {
        f_1c00ef50(m, p1, 0x1c0329f4);
        if m.r32(lts + 0x1124) != 0 {
            f_1c00ef80(m, p1, 0x2a);
        }
    }
    3
}

/// Mixed digit/letter word handler (e.g. "a-1", "10/20"); sets 0x8f8 flags for digits and apostrophes.
pub fn f_1c00dd80(m: &mut Mem, p1: u32, p2: u32, p3: u32) -> u32 {
    let lts = m.r32(p1 + 0x10);
    let mut p = p2;
    while p != p3 {
        let c = m.s(p);
        if c == 0x2f || (0x30..=0x39).contains(&c) {
            let f = m.r32(lts + 0x8f8);
            m.w32(lts + 0x8f8, f | 4);
        } else if c == 0x27 {
            let f = m.r32(lts + 0x8f8);
            m.w32(lts + 0x8f8, f | 8);
        } else if c != 0x2d && cls(m, c) & 3 == 0 {
            return 2;
        }
        p += 8;
    }
    let f = m.r32(lts + 0x8f8);
    if f & 4 == 0 {
        return 0;
    }
    if f & 8 != 0 {
        return 2;
    }
    f_1c008790(m, p1, p2, p3);
    3
}

/// Emit a sign word ('-' / '+' fixed strings, otherwise the dictionary entry for the char).
pub fn f_1c0086e0(m: &mut Mem, p1: u32, p2: u32) -> u32 {
    if p2 == 0x2d {
        f_1c00ef50(m, p1, 0x1c0329fc);
        f_1c00ef80(m, p1, 0x6f);
        return 0;
    }
    if p2 == 0x2b {
        f_1c00ef50(m, p1, 0x1c032a04);
        f_1c00ef80(m, p1, 0x6f);
        return 0;
    }
    if p2 == 0 {
        return 0;
    }
    let sp = m.sp();
    let recs = m.alloca(0x10);
    // bytes +2..+3 of each record are uninitialized in the original
    m.w16(recs, 0x20);
    m.w16(recs + 2, 0);
    m.w32(recs + 4, 0);
    m.w16(recs + 8, p2 as i32);
    m.w16(recs + 0xa, 0);
    m.w32(recs + 0xc, 0);
    let found = f_1c00f4c0(m, p1, recs, recs + 0x10, 0);
    m.set_sp(sp);
    if found == 0 {
        f_1c00ef80(m, p1, 3);
    }
    f_1c00ef80(m, p1, 0x6f);
    0
}

/// Speak a digit/slash/hyphen run: 2-4 digit groups as numbers, other runs spelled or looked up.
pub fn f_1c008790(m: &mut Mem, p1: u32, p2: u32, p3: u32) -> u32 {
    let mut p = p2;
    if p == p3 {
        return 0;
    }
    loop {
        let start = p;
        p = start + 8;
        let c = m.s(start);
        if c == 0x2d || c == 0x2f {
            f_1c009b00(m, p1, start, p);
            if p == p3 {
                return 0;
            }
            f_1c00ef80(m, p1, 0x6f);
        } else if (0x30..=0x39).contains(&c) {
            while p != p3 && (0x30..=0x39).contains(&m.s(p)) {
                p += 8;
            }
            match (p.wrapping_sub(start) as i32) >> 3 {
                2 => f_1c008910(m, p1, start),
                3 => f_1c0089a0(m, p1, start),
                4 => f_1c008a20(m, p1, start),
                _ => f_1c009b00(m, p1, start, p),
            };
            if p == p3 {
                return 0;
            }
            f_1c00ef80(m, p1, 0x6f);
        } else {
            while p != p3 && f_1c0088e0(m, m.s(p) as u32) != 0 {
                p += 8;
            }
            if (p.wrapping_sub(start) as i32 & !7) >= 0x18 && f_1c00f4c0(m, p1, start, p, 0) != 0 {
                if p == p3 {
                    return 0;
                }
                f_1c00ef80(m, p1, 0x6f);
            } else {
                let r = f_1c009be0(m, start, p);
                f_1c009b00(m, p1, start, p);
                f_1c00ef80(m, p1, if r == 0 { 0x6f } else { 0x73 });
            }
        }
        if p == p3 {
            return 0;
        }
    }
}

/// True if the char is not '-', '/' or a digit.
pub fn f_1c0088e0(_m: &mut Mem, p1: u32) -> u32 {
    let c = p1 as i32;
    if c == 0x2d || c == 0x2f || (0x30..=0x39).contains(&c) {
        return 0;
    }
    1
}

/// Speak a two-digit number (leading 0 is spelled digit by digit).
pub fn f_1c008910(m: &mut Mem, p1: u32, p2: u32) -> u32 {
    let c = w(m, p2);
    if c == 0x30 {
        f_1c009b00(m, p1, p2, p2 + 0x10);
        return 0;
    }
    if c == 0x31 {
        let s = tbl(m, 0x1c032b04, m.s(p2 + 8));
        f_1c00ef50(m, p1, s);
        return 0;
    }
    let s = tbl(m, 0x1c032b70, c as i16 as i32);
    f_1c00ef50(m, p1, s);
    if w(m, p2 + 8) != 0x30 {
        f_1c00ef80(m, p1, 0x6f);
        let s = tbl(m, DIGIT_NAMES, m.s(p2 + 8));
        f_1c00ef50(m, p1, s);
    }
    0
}

/// Speak a three-digit number (N hundred [tens]).
pub fn f_1c0089a0(m: &mut Mem, p1: u32, p2: u32) -> u32 {
    let c = w(m, p2);
    if c == 0x30 {
        f_1c009b00(m, p1, p2, p2 + 0x18);
        return 0;
    }
    let s = tbl(m, 0x1c032a38, c as i16 as i32);
    f_1c00ef50(m, p1, s);
    f_1c00ef80(m, p1, 0x6f);
    if w(m, p2 + 8) == 0x30 && w(m, p2 + 0x10) == 0x30 {
        f_1c00ef50(m, p1, 0x1c032b20);
        return 0;
    }
    f_1c008910(m, p1, p2 + 8)
}

/// Speak a four-digit number year-style (NN hundred / N thousand / NN NN).
pub fn f_1c008a20(m: &mut Mem, p1: u32, p2: u32) -> u32 {
    let c = w(m, p2);
    if c == 0x30 {
        f_1c009b00(m, p1, p2, p2 + 0x20);
        return 0;
    }
    if w(m, p2 + 0x10) == 0x30 && w(m, p2 + 0x18) == 0x30 {
        if w(m, p2 + 8) == 0x30 {
            let s = tbl(m, 0x1c032a38, c as i16 as i32);
            f_1c00ef50(m, p1, s);
            f_1c00ef80(m, p1, 0x6f);
            f_1c00ef50(m, p1, 0x1c032b2c);
            return 0;
        }
        f_1c008910(m, p1, p2);
        f_1c00ef80(m, p1, 0x6f);
        f_1c00ef50(m, p1, 0x1c032b20);
        return 0;
    }
    f_1c008910(m, p1, p2);
    f_1c00ef80(m, p1, 0x6f);
    f_1c008910(m, p1, p2 + 0x10);
    0
}

/// Speak a 3-char ASCII digit group (hundreds/tens/units); p3 != 0 selects ordinal forms.
pub fn f_1c008ae0(m: &mut Mem, p1: u32, p2: u32, p3: u32) -> u32 {
    let h = m.r8(p2);
    if h != 0x30 {
        let s = tbl(m, 0x1c032a38, h as i32);
        f_1c00ef50(m, p1, s);
        f_1c00ef80(m, p1, 0x6f);
        f_1c00ef50(m, p1, 0x1c032b20);
        if m.r8(p2 + 1) == 0x30 && m.r8(p2 + 2) == 0x30 {
            if p3 != 0 {
                f_1c00ef80(m, p1, 0x27);
            }
            return 0;
        }
        f_1c00ef50(m, p1, 0x1c032c84);
    }
    let t = m.r8(p2 + 1);
    if t == 0x31 {
        let s = tbl(m, 0x1c032b04, m.r8(p2 + 2) as i32);
        f_1c00ef50(m, p1, s);
        if p3 != 0 {
            f_1c00ef80(m, p1, 0x27);
        }
        return 0;
    }
    if t != 0x30 {
        let s = tbl(m, 0x1c032b70, t as i32);
        f_1c00ef50(m, p1, s);
        if m.r8(p2 + 2) == 0x30 {
            if p3 != 0 {
                f_1c00ef80(m, p1, 0x12);
                f_1c00ef80(m, p1, 0x27);
            }
            return 0;
        }
        f_1c00ef80(m, p1, 0x6f);
    }
    let u = m.r8(p2 + 2) as i32;
    let s = if p3 != 0 { tbl(m, 0x1c032cd8, u) } else { tbl(m, DIGIT_NAMES, u) };
    f_1c00ef50(m, p1, s);
    0
}

/// Speak a number token (digits with group separators, decimal part, exponent);
/// p4 != 0 requests ordinal form. Returns nonzero when the number is plural (not exactly 1).
pub fn f_1c008c10(m: &mut Mem, p1: u32, p2: u32, p3: u32, p4: u32) -> u32 {
    let lts = m.r32(p1 + 0x10);
    f_1c00ef80(m, p1, 0x35);
    if p2 != p3 {
        let c = w(m, p2);
        if c == 0xbc || c == 0xbd {
            f_1c009b00(m, p1, p2, p3);
            return 0;
        }
    }
    if p2 == p3 {
        return 0;
    }
    let sep = lts + 0x8ee;
    let mut has_sep = false;
    let mut ndig: i32 = 0;
    let mut end = p2;
    loop {
        let c = w(m, end);
        if cls(m, c as i16 as i32) & 0x10 == 0 && c != w(m, sep) {
            break;
        }
        if c == 0xbc || c == 0xbd {
            break;
        }
        if c == w(m, sep) {
            has_sep = true;
        } else {
            ndig += 1;
        }
        end += 8;
        if end == p3 {
            break;
        }
    }

    let mut ret: u32 = 'body: {
        let grouped;
        if ndig > 0x12 {
            if has_sep {
                // too long to read as a number: digits one by one, separator -> pause
                let mut p = p2;
                while p != end {
                    let c = m.s(p);
                    p += 8;
                    if c == m.s(sep) {
                        f_1c00ef80(m, p1, 0x73);
                    } else {
                        let s = tbl(m, DIGIT_NAMES, c);
                        f_1c00ef50(m, p1, s);
                        if p != end && w(m, p) != w(m, sep) {
                            f_1c00ef80(m, p1, 0x6f);
                        }
                    }
                }
                break 'body 1;
            }
            grouped = true;
        } else if ndig > 1 && w(m, p2) == 0x30 {
            grouped = true;
        } else if ndig == 0 {
            break 'body 0;
        } else {
            grouped = false;
        }

        if grouped {
            let mut p = p2;
            if ndig >= 6 {
                let mut groups = (ndig as u32).wrapping_sub(3) / 3;
                loop {
                    let mut k = 0;
                    while k < 3 {
                        let c = m.s(p);
                        if c != m.s(sep) {
                            if k != 0 {
                                f_1c00ef80(m, p1, 0x6f);
                            }
                            k += 1;
                            let s = tbl(m, DIGIT_NAMES, c);
                            f_1c00ef50(m, p1, s);
                        }
                        p += 8;
                    }
                    f_1c00ef80(m, p1, 0x73);
                    groups = groups.wrapping_sub(1);
                    if groups == 0 {
                        break;
                    }
                }
            }
            let mut k = 0;
            while p != end {
                let c = m.s(p);
                if c != m.s(sep) {
                    if k != 0 {
                        f_1c00ef80(m, p1, 0x6f);
                    }
                    k += 1;
                    let s = tbl(m, DIGIT_NAMES, c);
                    f_1c00ef50(m, p1, s);
                }
                p += 8;
            }
            break 'body 1;
        }

        // right-align the digits into an 18-byte zero-padded ASCII buffer
        let sp = m.sp();
        let buf = m.alloca(0x14);
        let mut n: u32 = 0x12;
        let mut p = end;
        while p != p2 {
            p -= 8;
            let c = m.s(p);
            if c != m.s(sep) {
                n -= 1;
                m.w8(buf + n, c as u8);
            }
        }
        let r = if n == 0x11 && m.r8(buf + 0x11) == 0x31 { 0 } else { 1 };
        while n != 0 {
            n -= 1;
            m.w8(buf + n, 0x30);
        }
        let scales = [0x1c032b50u32, 0x1c032b44, 0x1c032b3c, 0x1c032b34, 0x1c032b2c];
        for (k, &scale) in scales.iter().enumerate() {
            let off = 3 * k as u32;
            if f_1c0092e0(m, buf + off, 3) == 0 {
                continue;
            }
            f_1c008ae0(m, p1, buf + off, 0);
            f_1c00ef80(m, p1, 0x6f);
            f_1c00ef50(m, p1, scale);
            if f_1c0092e0(m, buf + off + 3, 15 - off) == 0 {
                if p4 != 0 {
                    f_1c00ef80(m, p1, 0x27);
                }
                m.set_sp(sp);
                break 'body r;
            }
            if f_1c0092e0(m, buf + off + 4, 14 - off) == 0 {
                f_1c00ef80(m, p1, 0x71);
            } else if f_1c0092e0(m, buf + off + 3, 1) == 0 {
                f_1c00ef50(m, p1, 0x1c032c84);
            } else {
                f_1c00ef80(m, p1, 0x73);
            }
        }
        f_1c008ae0(m, p1, buf + 15, p4);
        m.set_sp(sp);
        r
    };

    let mut p = end;
    if p == p3 {
        return ret;
    }
    let c = w(m, p);
    if c == 0xbc || c == 0xbd {
        f_1c00ef50(m, p1, 0x1c032c84);
        f_1c009b00(m, p1, p, p + 8);
        p += 8;
        ret = 1;
    }
    if p == p3 {
        return ret;
    }
    if w(m, p) == w(m, lts + 0x8ec) {
        if p2 != p {
            f_1c00ef80(m, p1, 0x6f);
        }
        f_1c00ef50(m, p1, 0x1c032c8c);
        p += 8;
        while p != p3 && w(m, p) != 0x65 {
            let c = m.s(p);
            if c != m.s(sep) {
                f_1c00ef80(m, p1, 0x6f);
                let s = tbl(m, DIGIT_NAMES, c);
                f_1c00ef50(m, p1, s);
            }
            p += 8;
        }
        ret = 1;
    }
    if p == p3 {
        return ret;
    }
    f_1c00ef50(m, p1, 0x1c032c94);
    p += 8;
    if p != p3 {
        let c = m.s(p);
        if c == 0x2d || c == 0x2b {
            f_1c0086e0(m, p1, c as u32);
            p += 8;
        }
    }
    f_1c008c10(m, p1, p, p3, 0);
    1
}

/// True if any of the first p2 bytes of the ASCII buffer p1 is not '0'.
pub fn f_1c0092e0(m: &mut Mem, p1: u32, p2: u32) -> u32 {
    let mut p = p1;
    let mut n = p2;
    while n != 0 {
        n = n.wrapping_sub(1);
        if m.r8(p) != 0x30 {
            return 1;
        }
        p += 1;
    }
    0
}

/// Date pattern test: D[D]-MON[-YY[YY]] with MON one of the 12 month abbreviations.
pub fn f_1c009310(m: &mut Mem, p1: u32, p2: u32) -> u32 {
    if cls(m, m.s(p1)) & 0x10 == 0 {
        return 0;
    }
    let mut a = p1 + 8;
    if a == p2 {
        return 0;
    }
    if w(m, a) != 0x2d {
        if cls(m, m.s(a)) & 0x10 == 0 {
            return 0;
        }
        a += 8;
        if a == p2 || w(m, a) != 0x2d {
            return 0;
        }
    }
    let mut mon = [0u16; 3];
    for c in mon.iter_mut() {
        a += 8;
        if a == p2 || cls(m, m.s(a)) & 3 == 0 {
            return 0;
        }
        *c = w(m, a);
    }
    if month_index(m, mon[0], mon[1], mon[2]) == 12 {
        return 0;
    }
    a += 8;
    if a == p2 {
        return 1;
    }
    if w(m, a) != 0x2d {
        return 0;
    }
    for _ in 0..2 {
        a += 8;
        if a == p2 || cls(m, m.s(a)) & 0x10 == 0 {
            return 0;
        }
    }
    a += 8;
    if a == p2 {
        return 1;
    }
    if cls(m, m.s(a)) & 0x10 == 0 {
        return 0;
    }
    a += 8;
    if a == p2 || cls(m, m.s(a)) & 0x10 == 0 {
        return 0;
    }
    a += 8;
    (a == p2) as u32
}

/// Speak a date token accepted by f_1c009310 (month day[, year]); kernel flag 0x200&8 = "the Nth of Month".
pub fn f_1c0094a0(m: &mut Mem, p1: u32, p2: u32, p3: u32) -> u32 {
    let mut dash = p2;
    while w(m, dash) != 0x2d {
        dash += 8;
    }
    let idx = month_index(m, w(m, dash + 8), w(m, dash + 0x10), w(m, dash + 0x18));
    let kernel = m.r32(p1 + 8);
    let day = if dash != p2 + 8 && w(m, p2) == 0x30 { p2 + 8 } else { p2 };
    let month = m.r32(0x1c032d1c + idx * 4);
    if m.r8(kernel + 0x200) & 8 != 0 {
        f_1c00ef50(m, p1, 0x1c032dd0);
        f_1c00ef80(m, p1, 0x6f);
        f_1c008c10(m, p1, day, dash, 1);
        f_1c00ef80(m, p1, 0x6f);
        f_1c00ef50(m, p1, 0x1c032dd4);
        f_1c00ef80(m, p1, 0x6f);
        f_1c00ef50(m, p1, month);
    } else {
        f_1c00ef50(m, p1, month);
        f_1c00ef80(m, p1, 0x6f);
        f_1c008c10(m, p1, day, dash, 1);
    }
    let dash2 = dash + 0x20;
    if dash2 == p3 {
        return 0;
    }
    f_1c00ef80(m, p1, 0x73);
    let year = dash2 + 8;
    if dash2 + 0x18 == p3 {
        f_1c008910(m, p1, year);
        return 0;
    }
    if w(m, year) != 0x30
        && w(m, dash2 + 0x10) == 0x30
        && w(m, dash2 + 0x18) == 0x30
        && w(m, dash2 + 0x20) != 0x30
    {
        f_1c008910(m, p1, year);
        f_1c00ef80(m, p1, 0x6f);
        f_1c00ef50(m, p1, 0x1c032a2c);
        let s = tbl(m, DIGIT_NAMES, m.s(dash2 + 0x20));
        f_1c00ef50(m, p1, s);
        return 0;
    }
    f_1c008a20(m, p1, year)
}

/// Fraction pattern test: N/D or NN/D (D 1-3 digits, no leading 0, 3 digits only as 100), optional %.
pub fn f_1c009650(m: &mut Mem, p1: u32, p2: u32) -> u32 {
    let c = w(m, p1);
    if cls(m, c as i16 as i32) & 0x10 == 0 || c == 0x30 {
        return 0;
    }
    let mut a = p1 + 8;
    if a == p2 {
        return 0;
    }
    if w(m, a) != 0x2f {
        if cls(m, m.s(a)) & 0x10 == 0 {
            return 0;
        }
        a += 8;
        if a == p2 || w(m, a) != 0x2f {
            return 0;
        }
    }
    a += 8;
    let mut n: i32 = 0;
    if a == p2 {
        return 0;
    }
    loop {
        let d = w(m, a);
        if cls(m, d as i16 as i32) & 0x10 == 0 {
            break;
        }
        if n == 0 && d == 0x30 {
            return 0;
        }
        a += 8;
        n += 1;
        if a == p2 {
            break;
        }
    }
    if n == 0 || n > 3 {
        return 0;
    }
    if n == 3 && (w(m, a - 8) != 0x30 || w(m, a - 0x10) != 0x30 || w(m, a - 0x18) != 0x31) {
        return 0;
    }
    if a == p2 {
        return 1;
    }
    if w(m, a) != 0x25 {
        return 0;
    }
    (a + 8 == p2) as u32
}

/// Speak a fraction accepted by f_1c009650 (numerator, ordinal denominator, "half(s)", percent).
pub fn f_1c009710(m: &mut Mem, p1: u32, p2: u32, p3: u32) -> u32 {
    let mut slash = p2;
    while w(m, slash) != 0x2f {
        slash += 8;
    }
    let plural = f_1c008c10(m, p1, p2, slash, 0);
    f_1c00ef80(m, p1, 0x6f);
    let den = slash + 8;
    let mut e = den;
    while e != p3 && w(m, e) != 0x25 {
        e += 8;
    }
    if den + 8 == e && w(m, den) == 0x32 {
        f_1c00ef50(m, p1, if plural != 0 { 0x1c032dc8 } else { 0x1c032dc0 });
    } else {
        f_1c008c10(m, p1, den, e, 1);
        if plural != 0 {
            let last = m.s(e - 8);
            let sym = if e > den + 8 && w(m, e - 0x10) == 0x31 {
                0x29
            } else if last == 0x32 || last == 0x33 {
                0x2a
            } else {
                0x29
            };
            f_1c00ef80(m, p1, sym);
        }
    }
    if e != p3 {
        f_1c00ef50(m, p1, 0x1c032c60);
    }
    0
}

/// Clock-time pattern test: H[H]:MM[:SS][<decimal point>D+]; p1 = lts state.
pub fn f_1c0097f0(m: &mut Mem, p1: u32, p2: u32, p3: u32) -> u32 {
    let digit = |m: &Mem, a: u32| cls(m, m.s(a)) & 0x10 != 0;
    if !digit(m, p2) {
        return 0;
    }
    let mut a = p2 + 8;
    if a == p3 {
        return 0;
    }
    if w(m, a) != 0x3a {
        if !digit(m, a) {
            return 0;
        }
        a += 8;
        if a == p3 || w(m, a) != 0x3a {
            return 0;
        }
    }
    for _ in 0..2 {
        a += 8;
        if a == p3 || !digit(m, a) {
            return 0;
        }
    }
    a += 8;
    if a == p3 {
        return 1;
    }
    if w(m, a) == 0x3a {
        for _ in 0..2 {
            a += 8;
            if a == p3 || !digit(m, a) {
                return 0;
            }
        }
        a += 8;
    }
    if a == p3 || w(m, a) != w(m, p1 + 0x8ec) {
        return 1;
    }
    a += 8;
    if a == p3 || !digit(m, a) {
        return 0;
    }
    a += 8;
    while a != p3 && digit(m, a) {
        a += 8;
    }
    1
}

/// True if [p1,p2) is exactly "am"/"pm" (either case per letter).
pub fn f_1c009900(m: &mut Mem, p1: u32, p2: u32) -> u32 {
    let c = w(m, p1);
    if c != 0x61 && c != 0x41 && c != 0x70 && c != 0x50 {
        return 0;
    }
    let c = w(m, p1 + 8);
    if c != 0x6d && c != 0x4d {
        return 0;
    }
    (p1 + 0x10 == p2) as u32
}

/// Speak a clock time accepted by f_1c0097f0.
pub fn f_1c009950(m: &mut Mem, p1: u32, p2: u32, p3: u32) -> u32 {
    f_1c00ef80(m, p1, 0x35);
    let mut p;
    if w(m, p2 + 8) == 0x3a {
        let s = tbl(m, DIGIT_NAMES, m.s(p2));
        f_1c00ef50(m, p1, s);
        p = p2 + 0x10;
    } else {
        f_1c008910(m, p1, p2);
        p = p2 + 0x18;
    }
    f_1c00ef80(m, p1, 0x35);
    f_1c00ef80(m, p1, 0x71);
    if w(m, p) != 0x30 || w(m, p + 8) != 0x30 {
        f_1c008910(m, p1, p);
    }
    p += 0x10;
    if p == p3 {
        return 0;
    }
    if w(m, p) == 0x3a {
        f_1c00ef80(m, p1, 0x71);
        f_1c008910(m, p1, p + 8);
        p += 0x18;
    }
    if p == p3 {
        return 0;
    }
    if w(m, p) != 0x2e {
        f_1c009b00(m, p1, p, p3);
        return 0;
    }
    f_1c00ef80(m, p1, 0x6f);
    f_1c00ef50(m, p1, 0x1c032c8c);
    p += 8;
    while p != p3 {
        f_1c00ef80(m, p1, 0x6f);
        let s = tbl(m, DIGIT_NAMES, m.s(p));
        f_1c00ef50(m, p1, s);
        p += 8;
    }
    0
}
