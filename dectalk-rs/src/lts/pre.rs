use super::*;

const CHAR_CLASS: u32 = 0x1c03244c;
const CASE_FOLD: u32 = 0x1c02fb80;
const CHAR_FLAGS: u32 = 0x1c02fd80;

/// Low byte of the u16 class word for `c` (movsx index, stride 2).
fn class_lo(m: &Mem, c: i32) -> u8 {
    m.r8(CHAR_CLASS.wrapping_add((c as u32).wrapping_mul(2)))
}

/// High byte of the u16 class word for `c`.
fn class_hi(m: &Mem, c: i32) -> u8 {
    m.r8(CHAR_CLASS.wrapping_add(1).wrapping_add((c as u32).wrapping_mul(2)))
}

fn flags(m: &Mem, c: i32) -> u8 {
    m.r8(CHAR_FLAGS.wrapping_add(c as u32))
}

fn fold(m: &Mem, c: i32) -> u8 {
    m.r8(CASE_FOLD.wrapping_add(c as u32))
}

/// Spell-mode handler: when kernel flag 0x200 bit 0x10 is set, spell the whole word letter by letter.
pub fn f_1c00cbf0(m: &mut Mem, p1: u32, p2: u32, p3: u32) -> u32 {
    let kernel = m.r32(p1 + 8);
    let lts = m.r32(p1 + 0x10);
    if m.r8(kernel + 0x200) & 0x10 == 0 {
        return 0;
    }
    f_1c009b00(m, p1, p2, p3);
    f_1c00c550(m, p1, 0);
    f_1c00c820(m, p1, lts + 0x914);
    1
}

/// Look the word up in the table at 0x1c032f08; on a hit emit its expansion and reset both pause types to 0x6f.
pub fn f_1c00cc40(m: &mut Mem, p1: u32, p2: u32, _p3: u32) -> u32 {
    let lts = m.r32(p1 + 0x10);
    let hit = f_1c00c440(m, lts, p2, 0x1c032f08);
    if hit == 0 {
        return 0;
    }
    f_1c00ef50(m, p1, hit);
    m.w32(lts + 0x111c, 0x6f);
    m.w32(lts + 0x1120, 0x6f);
    3
}

/// Single-character word with kernel 0x200 bit 4 set: speak it via the math-symbol table (FUN_1c008630).
pub fn f_1c00cc90(m: &mut Mem, p1: u32, p2: u32, p3: u32) -> u32 {
    if p2.wrapping_add(8) != p3 || m.r8(p1 + 0x200) & 4 == 0 {
        return 0;
    }
    let ch = m.r8(p2) as u32;
    if f_1c008630(m, p1, ch) != 0 { 3 } else { 0 }
}

/// Abbreviation / dictionary handler: handles a trailing '.', the 0x1c03264c table, and the main dictionary (FUN_1c00f4c0).
pub fn f_1c00ccd0(m: &mut Mem, p1: u32, p2: u32, p3: u32) -> u32 {
    let lts = m.r32(p1 + 0x10);
    let result;
    if p3 >= p2.wrapping_add(0x10) && m.r16(p3.wrapping_sub(8)) == 0x2e {
        result = f_1c00f4c0(m, p1, p2, p3.wrapping_sub(8), 1);
        if result == 0 {
            return 0;
        }
    } else {
        let sentence_end = (f_1c00ed60(m, lts) != 0) as u32;
        if m.r32(lts + 0x8fc) != 0 && sentence_end == 1 {
            let mut hit = f_1c00c440(m, lts, lts + 0x914, 0x1c03264c);
            if hit != 0 {
                if m.r32(lts + 0x1124) != 0 {
                    // Skip to the alternate string following the first NUL.
                    loop {
                        let b = m.r8(hit);
                        hit = hit.wrapping_add(1);
                        if b == 0 {
                            break;
                        }
                    }
                }
                f_1c00ef50(m, p1, hit);
                f_1c00f140(m, p1);
                f_1c00c550(m, p1, 0);
                f_1c00c820(m, p1, lts + 0x914);
                return 1;
            }
        }
        result = f_1c00f4c0(m, p1, p2, p3, sentence_end);
        if result == 0 {
            return 0;
        }
        if result == 2 {
            f_1c00f140(m, p1);
        }
    }
    f_1c00c550(m, p1, result);
    f_1c00c820(m, p1, lts + 0x914);
    1
}

/// Strip leading punctuation (class bit 0x40) from *p2, setting lts+0x8f8 bit 1 and possibly the leading pause type.
pub fn f_1c00cde0(m: &mut Mem, p1: u32, p2: u32) -> u32 {
    m.w32(p1 + 0x111c, 0x6f);
    m.w32(p1 + 0x1120, 0x6f);
    m.w32(p1 + 0x8f8, 0);
    while class_lo(m, m.s(m.r32(p2))) & 0x40 != 0 {
        let v = m.r32(p1 + 0x8f8) | 1;
        m.w32(p1 + 0x8f8, v);
        if class_hi(m, m.s(m.r32(p2))) & 1 != 0 {
            m.w32(p1 + 0x111c, 0x73);
        }
        let cur = m.r32(p2).wrapping_add(8);
        m.w32(p2, cur);
    }
    0
}

/// Strip trailing punctuation (class bit 0x80) from the end pointer *p3, setting lts+0x8f8 bit 2 and the trailing pause type.
pub fn f_1c00ce50(m: &mut Mem, p1: u32, p2: u32, p3: u32) -> u32 {
    while m.r32(p3) != p2 {
        let end = m.r32(p3);
        if class_lo(m, m.s(end.wrapping_sub(8))) & 0x80 == 0 {
            break;
        }
        let v = m.r32(p1 + 0x8f8) | 2;
        m.w32(p1 + 0x8f8, v);
        let end = m.r32(p3).wrapping_sub(8);
        m.w32(p3, end);
        if class_hi(m, m.s(end)) & 1 != 0 {
            m.w32(p1 + 0x1120, 0x73);
        }
        let end = m.r32(p3);
        if end > p2.wrapping_add(8) {
            let pause = match m.s(end.wrapping_sub(8)) {
                0x21 => Some(0x76),
                0x2c | 0x3a | 0x3b => Some(0x73),
                0x3f => Some(0x75),
                _ => None,
            };
            if let Some(pause) = pause {
                m.w32(p1 + 0x1120, pause);
                let end = m.r32(p3).wrapping_sub(8);
                m.w32(p3, end);
            }
        }
    }
    0
}

/// If the word is empty after stripping (p2 == p3), spell the punctuation range [p4, p5); otherwise emit any pending leading pause.
pub fn f_1c00cf30(m: &mut Mem, p1: u32, p2: u32, p3: u32, p4: u32, p5: u32) -> u32 {
    let lts = m.r32(p1 + 0x10);
    if p2 == p3 {
        f_1c009b00(m, p1, p4, p5);
        f_1c00c550(m, p1, 0);
        f_1c00c820(m, p1, lts + 0x914);
        return 1;
    }
    let pause = m.r32(lts + 0x111c);
    if pause != 0x6f {
        f_1c00ef80(m, p1, pause);
    }
    0
}

/// Dictionary lookup for words that had punctuation stripped; handles an abbreviation-style trailing '.' in *p3.
pub fn f_1c00cf90(m: &mut Mem, p1: u32, p2: u32, p3: u32) -> u32 {
    let lts = m.r32(p1 + 0x10);
    let mut sentence_end = 0u32;
    if m.r8(lts + 0x8f8) & 2 == 0 {
        if f_1c00ed60(m, lts) != 0 {
            sentence_end = 1;
        }
    } else {
        let last = m.r32(p3).wrapping_sub(8);
        if p2 != last && m.r16(last) == 0x2e {
            let r = f_1c00f4c0(m, p1, p2, last, 1);
            if r != 0 {
                if r != 2 {
                    m.w32(lts + 0x1120, 0x74);
                }
                return 3;
            }
            m.w32(lts + 0x1120, 0x74);
            let end = m.r32(p3).wrapping_sub(8);
            m.w32(p3, end);
        }
    }
    if m.r8(lts + 0x8f8) & 3 != 0 {
        let end = m.r32(p3);
        let r = f_1c00f4c0(m, p1, p2, end, sentence_end);
        if r != 0 {
            if r == 2 {
                f_1c00f140(m, p1);
            }
            return 3;
        }
    }
    0
}

/// Spell the word if FUN_1c009a50 judges it unpronounceable (all-caps heuristics).
pub fn f_1c00d060(m: &mut Mem, p1: u32, p2: u32, p3: u32) -> u32 {
    if f_1c009a50(m, p2, p3) != 0 {
        return 0;
    }
    f_1c009b00(m, p1, p2, p3);
    3
}

/// Single letters ("I", "a") and dotted initialisms ("U.S.A."): speak them letter by letter.
pub fn f_1c00d0e0(m: &mut Mem, p1: u32, p2: u32, p3: u32) -> u32 {
    let lts = m.r32(p1 + 0x10);
    if p2.wrapping_add(8) == p3 {
        let ch = m.s(p2);
        if flags(m, ch & 0xff) & 2 != 0 && m.r8(lts + 0x8f8) & 3 == 0 && f_1c00ed60(m, lts) != 0 {
            m.w32(lts + 0x1120, 0x74);
            f_1c00f140(m, p1);
            return 2;
        }
        if ch == 0x69 || ch == 0x49 {
            let sym = if m.r8(lts + 0x8f8) & 3 != 0 { 0x67 } else { 0x66 };
            f_1c00ef80(m, p1, sym);
            f_1c00ef80(m, p1, 7);
            return 3;
        }
        if (ch == 0x61 || ch == 0x41)
            && (m.r32(lts + 0x8fc) == 0 || m.r32(lts + 0x8fc) == 1)
            && m.r8(lts + 0x8f8) & 3 == 0
            && f_1c00f100(m, lts + 0x900) == 1
        {
            let idx = m.r32(lts + 0x8bc);
            m.w32(lts.wrapping_add(idx.wrapping_mul(4)).wrapping_add(0x4bc), 4);
            f_1c00ef80(m, p1, 0x78);
            f_1c00ef80(m, p1, 0x11);
            return 3;
        }
        if ch == 0xbc || ch == 0xbd || flags(m, m.s(p2)) & 0x10 != 0 {
            return 0;
        }
        return 2;
    }

    // Must alternate letter, '.', letter, '.', ... (a trailing letter without its '.' is allowed).
    let mut after_letter = false;
    let mut cur = p2;
    while cur != p3 {
        if !after_letter {
            if flags(m, m.r8(cur) as i32) & 2 == 0 {
                return 0;
            }
            after_letter = true;
        } else {
            if m.r16(cur) != 0x2e {
                return 0;
            }
            after_letter = false;
        }
        cur = cur.wrapping_add(8);
    }
    if p2 != p3 {
        let mut letter = p2;
        let mut next = p2.wrapping_add(8);
        loop {
            f_1c009b00(m, p1, letter, next);
            letter = letter.wrapping_add(8);
            next = next.wrapping_add(8);
            if letter == p3 {
                break;
            }
            letter = letter.wrapping_add(8);
            next = next.wrapping_add(8);
            if letter == p3 {
                break;
            }
            f_1c00ef80(m, p1, 0x6f);
        }
    }
    if after_letter && m.r8(lts + 0x8f8) & 2 == 0 && f_1c00ed60(m, lts) != 0 {
        f_1c00f140(m, p1);
    }
    3
}

/// Case-fold each record's char in [p1, p2) through the byte table at 0x1c02fb80.
pub fn f_1c00d2f0(m: &mut Mem, p1: u32, p2: u32) -> u32 {
    let mut cur = p1;
    while cur != p2 {
        let folded = fold(m, m.s(cur));
        m.w16(cur, folded as i32);
        cur = cur.wrapping_add(8);
    }
    0
}

/// Ordinal suffixes "st"/"rd" (optionally followed by '.'): pick the ordinal form based on the preceding number word.
pub fn f_1c00d320(m: &mut Mem, p1: u32, p2: u32, p3: u32) -> u32 {
    let lts = m.r32(p1 + 0x10);
    let bare_end = p2.wrapping_add(0x10);
    let dotted = p2.wrapping_add(0x18) == p3 && m.r16(bare_end) == 0x2e;
    if !dotted && (bare_end != p3 || m.r8(lts + 0x8f8) & 2 != 0 || f_1c00ed60(m, lts) == 0) {
        return 0;
    }
    let c0 = m.s(p2);
    let c1 = m.s(p2 + 8);
    let is_st = c0 == 0x73;
    if is_st {
        if c1 != 0x74 {
            return 0;
        }
    } else if c0 != 0x64 || c1 != 0x72 {
        return 0;
    }
    if bare_end == p3 {
        f_1c00f140(m, p1);
    }
    f_1c00c510(m, lts);
    if f_1c00f100(m, lts + 0x900) == 0 {
        f_1c00ef50(m, p1, if is_st { 0x1c032a0c } else { 0x1c032a24 });
        return 3;
    }
    let prev = lts + 0xd14;
    f_1c00c820(m, p1, prev);
    let pc = m.r16(prev);
    let table;
    if class_lo(m, pc as i32) & 0x20 != 0 {
        let d_r = pc == 0x44 && m.r16(lts + 0xd1c) == 0x72 && m.r16(lts + 0xd24) == 0;
        let s_t = pc == 0x53 && m.r16(lts + 0xd1c) == 0x74 && m.r16(lts + 0xd24) == 0;
        if d_r || s_t {
            table = if is_st { 0x1c032a0c } else { 0x1c032a24 };
        } else {
            table = if is_st { 0x1c032a14 } else { 0x1c032a1c };
        }
    } else if m.r32(lts + 0x8bc) == 1 {
        table = if is_st { 0x1c032a14 } else { 0x1c032a1c };
    } else {
        table = if is_st { 0x1c032a0c } else { 0x1c032a24 };
    }
    f_1c00ef50(m, p1, table);
    let pause = m.r32(lts + 0x1120);
    f_1c00ef80(m, p1, pause);
    f_1c00ef10(m, lts + 0x914, prev);
    1
}

/// Number-scan setup: set decimal/group separators from kernel 0x200 bit 8, and consume a leading sign/symbol into lts+0x1118.
pub fn f_1c00d4a0(m: &mut Mem, p1: u32, p2: u32, p3: u32, p4: u32) -> u32 {
    let kernel = m.r32(p1 + 8);
    let lts = m.r32(p1 + 0x10);
    m.w16(lts + 0x8ec, 0x2e);
    m.w16(lts + 0x8ee, 0x2c);
    if m.r8(kernel + 0x200) & 8 != 0 {
        m.w16(lts + 0x8ec, 0x2c);
        m.w16(lts + 0x8ee, 0x2e);
    }
    m.w32(lts + 0x1118, 0);
    m.w32(p4, p2);
    let c = m.r16(p2) as u16;
    if matches!(c, 0x2d | 0x2b | 0xa7 | 0xb1 | 0xb6) {
        m.w32(lts + 0x1118, c as i16 as i32 as u32);
        let next = m.r32(p4).wrapping_add(8);
        m.w32(p4, next);
        if next == p3 {
            return 2;
        }
    }
    0
}

/// Latches kernel 0x20c bit 4 on the first capitalized word (one of '@'..'a' followed by lowercase); returns 0 to reject otherwise.
pub fn f_1c00ed80(m: &mut Mem, p1: u32, p2: u32, p3: u32) -> u32 {
    let kernel = m.r32(p1 + 8);
    let state = m.r32(kernel + 0x20c);
    if state & 4 != 0 {
        return 1;
    }
    if m.r8(kernel + 0x200) & 0x40 == 0 {
        return 1;
    }
    let lts = m.r32(p1 + 0x10);
    if m.r32(lts + 0x8bc) == 0 {
        return 0;
    }
    let c = m.r16(p2);
    if !(0x40..=0x61).contains(&c) {
        return 0;
    }
    let mut cur = p2.wrapping_add(8);
    while cur < p3 {
        let c = m.r16(cur);
        if c > 0x7a || c < 0x61 {
            return 0;
        }
        cur = cur.wrapping_add(8);
    }
    m.w32(kernel + 0x20c, state | 4);
    1
}

/// Scan a number in [p2, p3): fills p4[0..6] with (int start, int end, frac start, frac end, exp start, exp end).
pub fn f_1c00c290(m: &mut Mem, p1: u32, p2: u32, p3: u32, p4: u32) -> u32 {
    let lts = m.r32(p1 + 0x10);
    let kernel = m.r32(p1 + 8);
    m.w32(p4, 0);
    m.w32(p4 + 8, 0);
    m.w32(p4 + 0x10, 0);
    let mut cur = p2;
    if cur == p3 {
        return cur;
    }
    let c = m.r16(p2);
    if c == 0xbc || c == 0xbd || flags(m, c as i32) & 0x10 != 0 {
        m.w32(p4, p2);
        'int: loop {
            let c = m.r16(cur);
            if c == 0xbc || c == 0xbd {
                m.w32(p4 + 4, cur.wrapping_add(8));
                return cur.wrapping_add(8);
            }
            let group = m.r16(lts + 0x8ee);
            if c == group {
                // A group separator needs exactly three digits on each side.
                let mut back = cur;
                let mut n = 2;
                let at_start = loop {
                    if back == p2 {
                        break true;
                    }
                    back = back.wrapping_sub(8);
                    if m.r16(back) == group {
                        break 'int;
                    }
                    let t = n;
                    n -= 1;
                    if t == 0 {
                        break false;
                    }
                };
                if !at_start && back != p2 && m.r16(back.wrapping_sub(8)) != group {
                    break 'int;
                }
                let after = cur.wrapping_add(8);
                let mut fwd = after;
                let mut n = 2;
                loop {
                    if fwd == p3 || flags(m, m.s(fwd)) & 0x10 == 0 {
                        break 'int;
                    }
                    fwd = fwd.wrapping_add(8);
                    let t = n;
                    n -= 1;
                    if t == 0 {
                        break;
                    }
                }
                if fwd != p3 && flags(m, m.s(fwd)) & 0x10 != 0 {
                    break 'int;
                }
                cur = after;
            } else {
                if flags(m, c as i32) & 0x10 == 0 {
                    break 'int;
                }
                cur = cur.wrapping_add(8);
            }
            if cur == p3 {
                break 'int;
            }
        }
        m.w32(p4 + 4, cur);
    }
    if cur == p3 {
        return cur;
    }
    if m.r16(cur) == m.r16(lts + 0x8ec) {
        m.w32(p4 + 8, cur);
        cur = cur.wrapping_add(8);
        while cur != p3 && flags(m, m.s(cur)) & 0x10 != 0 {
            cur = cur.wrapping_add(8);
        }
        m.w32(p4 + 0xc, cur);
    }
    if cur == p3 || m.r16(cur) != 0x65 || m.r8(kernel + 0x200) & 4 == 0 {
        return cur;
    }
    let exp = cur;
    cur = cur.wrapping_add(8);
    if cur == p3 {
        return cur;
    }
    let c = m.r16(cur);
    if c == 0x2b || c == 0x2d {
        cur = cur.wrapping_add(8);
    }
    if cur == p3 || flags(m, m.s(cur)) & 0x10 == 0 {
        return cur;
    }
    cur = cur.wrapping_add(8);
    m.w32(p4 + 0x10, exp);
    while cur != p3 && flags(m, m.s(cur)) & 0x10 != 0 {
        cur = cur.wrapping_add(8);
    }
    m.w32(p4 + 0x14, cur);
    cur
}

/// Returns 0 if an all-uppercase word should be spelled out (all flag-0x40 letters, or bad 2-4 letter onset/coda bigram), else 1.
pub fn f_1c009a50(m: &mut Mem, p1: u32, p2: u32) -> u32 {
    let mut count = 0u32;
    let mut all_flagged = true;
    let mut cur = p1;
    if cur >= p2 {
        return 1;
    }
    while cur < p2 {
        let c = m.r16(cur);
        if !(0x41..=0x5a).contains(&c) {
            return 1;
        }
        count += 1;
        if all_flagged && flags(m, c as i32) & 0x40 == 0 {
            all_flagged = false;
        }
        cur = cur.wrapping_add(8);
    }
    if count <= 1 {
        return 1;
    }
    if all_flagged {
        return 0;
    }
    if count > 4 {
        return 1;
    }
    const BIGRAM: u32 = 0x1c02ff61;
    let a = m.s(p1);
    let b = m.s(p1 + 8);
    let onset = m.r8(BIGRAM.wrapping_add((a.wrapping_mul(26).wrapping_add(b)) as u32));
    if onset & 2 != 0 {
        return 0;
    }
    let a = m.s(p2.wrapping_sub(8));
    let b = m.s(p2.wrapping_sub(0x10));
    let coda = m.r8(BIGRAM.wrapping_add((a.wrapping_mul(26).wrapping_add(b)) as u32));
    (!coda & 1) as u32
}

/// Returns 0 for a single char, or a short (<4, or 4 with one '&') run of letters/'&'; else 1.
pub fn f_1c009be0(m: &mut Mem, p1: u32, p2: u32) -> u32 {
    if p1.wrapping_add(8) == p2 {
        return 0;
    }
    let mut count = 0i32;
    let mut amps = 0i32;
    let mut cur = p1;
    if cur == p2 {
        return 0;
    }
    while cur != p2 {
        let c = m.s(cur);
        cur = cur.wrapping_add(8);
        count += 1;
        if c == 0x26 {
            amps += 1;
        } else if flags(m, fold(m, c) as i32) & 3 == 0 {
            return 1;
        }
    }
    if count < 4 {
        return 0;
    }
    if count == 4 && amps == 1 {
        return 0;
    }
    1
}

/// If kernel 0x200 bit 4 is set and char p2 is in the symbol table at 0x1c030370, speak its phoneme string; returns 1 on a hit.
pub fn f_1c008630(m: &mut Mem, p1: u32, p2: u32) -> u32 {
    if m.r8(p1 + 0x200) & 4 == 0 {
        return 0;
    }
    let ch = p2 as u8;
    let mut entry = 0x1c030370u32;
    loop {
        let key = m.r8(entry);
        if key == 0 {
            return 0;
        }
        if key == ch {
            let s = m.r32(entry + 4);
            f_1c008680(m, p1, s);
            return 1;
        }
        entry += 8;
    }
}

/// Map each ASCII phoneme char of the NUL-terminated string p2 through the table at 0x1c0303c8 and write the code to PH.
pub fn f_1c008680(m: &mut Mem, p1: u32, p2: u32) -> u32 {
    let mut s = p2;
    loop {
        let b = m.r8(s) as i32;
        if b == 0 {
            return 0;
        }
        let mut entry = 0x1c0303c8u32;
        let mut i = 0u32;
        while (entry as i32) < 0x1c0305d0 {
            if b == m.r8(entry) as i8 as i32 {
                f_1c00f700(m, p1, 0x1c0303cc + i * 8, 1);
                break;
            }
            entry += 8;
            i += 1;
        }
        s = s.wrapping_add(1);
    }
}

/// Spell the records in [p2, p3) letter by letter, separating letters with symbol 0x6f.
pub fn f_1c009b00(m: &mut Mem, p1: u32, p2: u32, p3: u32) -> u32 {
    let kernel = m.r32(p1 + 8);
    let mut cur = p2;
    if cur >= p3 {
        return 0;
    }
    let sp = m.sp();
    let word = m.alloca(4);
    loop {
        let ch = m.r8(cur) as u32;
        if f_1c008630(m, kernel, ch) == 0 {
            let f = fold(m, m.s(cur)) as u32;
            if f == 0x61 {
                f_1c00ef80(m, p1, 0x67);
                f_1c00ef80(m, p1, 3);
            } else if (0x30..=0x39).contains(&f) {
                let digit = m.r32(0x1c0329e8 + f * 4);
                f_1c00ef50(m, p1, digit);
            } else {
                let mut s = m.r32(m.r32(kernel + 0x470).wrapping_add(f * 4));
                loop {
                    let b = m.r8(s) as u32;
                    if b == 0 {
                        break;
                    }
                    let code = m.r32(m.r32(kernel + 0x460).wrapping_add(b * 4));
                    m.w32(word, code);
                    f_1c00f700(m, kernel, word, 1);
                    s = s.wrapping_add(1);
                }
            }
        }
        cur = cur.wrapping_add(8);
        if cur == p3 {
            break;
        }
        f_1c00ef80(m, p1, 0x6f);
        if cur >= p3 {
            break;
        }
    }
    m.set_sp(sp);
    0
}
