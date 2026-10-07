//! Text parser front end: user/main dictionary lookups (binary search over sorted entry
//! tables) and the rule-driven rewrite loop FUN_1c010c00 over the input text buffer.
//!
//! Layouts:
//! - Char class table 0x1c063b90: u16 per byte value. 0x04 = letter (case-foldable),
//!   0x20 = separator/white space, 0x800 = trailing punctuation (boundary if followed by
//!   a separator or NUL). Fold table 0x1c063e94: u8 per byte value (case fold).
//! - Dictionary entry: +0 u32, +4 NUL-terminated key. Tables are arrays of entry pointers.
//!   kernel+0xf8 = active language i; kernel+i*4 + {0x00 table, 0x1c count} (entry list for
//!   the sequential/halving search), {0x38 table, 0x54 count} (sorted list, binary search).
//! - Record: 6 bytes (3 x i16), parallel to every text byte (input p7, output p8).
//! - Cursor (24 bytes, p13 and frame copies C/S/T): +0 src base, +2 src off, +4 dst base,
//!   +6 dst off (all i16), +8/+0xa/+0xc/+0xe matcher results (+0xa == 1: match; +0xe is the
//!   dst off at match end), +0x10 i16 carried back to p13+0x10, +0x14 u32.
//! - Rule tables: 0x1c064e04 i16 last section; 0x1c064e08[section] i16 first rule;
//!   0x1c064e0e i16 rule count; 0x1c064e10[rule] i16 byte offset into 0x1c064eb8.
//!   Rule record: +0 i16 kind (0 rule, 1 end, 2 pop/return, 3 goto +2, 4 call +2),
//!   +4 u32 mask vs p9, +8 u32 mask vs p10 (-1 = any), +0xc i16 rule number,
//!   +0xe/+0x12 hit: next / push-return, +0x10/+0x14 miss: next / push-return,
//!   +0x16 i16 condition on attr byte (-1 always, 1 nonzero, 0 zero), +0x18 pattern.
//! - Return stack: 10 x i16 with an i16 depth.

use super::*;

const CLASS: u32 = 0x1c06_3b90;
const FOLD: u32 = 0x1c06_3e94;

fn class16(m: &Mem, ch: u32) -> u32 {
    m.r16(CLASS + (ch & 0xff) * 2) as u16 as u32
}

fn fold_u(m: &Mem, ch: u32) -> u8 {
    m.r8(FOLD + (ch & 0xff))
}

fn fold_s(m: &Mem, ch: u32) -> u8 {
    m.r8(FOLD.wrapping_add(sx8(ch)))
}

fn strlen(m: &Mem, s: u32) -> u32 {
    let mut n = 0;
    while m.r8(s.wrapping_add(n)) != 0 {
        n += 1;
    }
    n
}

/// rep movsd/movsb: forward copy.
fn fwd_copy(m: &mut Mem, dst: u32, src: u32, n: u32) {
    for k in 0..n {
        let b = m.r8(src.wrapping_add(k));
        m.w8(dst.wrapping_add(k), b);
    }
}

/// Dictionary lookup of `p2` in both dictionaries of kernel `p1`; with p3 == 1 a trailing
/// '.' is also tried stripped. 1 = found.
pub fn f_1c0106d0(m: &mut Mem, p1: u32, p2: u32, p3: u32) -> u32 {
    let kernel = p1;
    let word = p2;
    let sp = m.sp();
    let buf = m.alloca(0x100);
    let mut stripped: u16 = 0;
    if p3 == 1 {
        let len = sx16(strlen(m, word));
        if m.r8(word.wrapping_add(len).wrapping_sub(1)) == b'.' {
            let n = len.wrapping_sub(1);
            fwd_copy(m, buf, word, n);
            stripped = 1;
            m.w8(buf.wrapping_add(len).wrapping_sub(1), 0);
        }
    }
    let lang = m.r32(kernel + 0xf8);
    let seq_count = m.r32(kernel + lang * 4 + 0x1c);
    let seq_table = m.r32(kernel + lang * 4);
    let bin_count = m.r32(kernel + lang * 4 + 0x54);
    let bin_table = m.r32(kernel + lang * 4 + 0x38);
    if word == 0 || m.r8(word) == 0 {
        m.set_sp(sp);
        return 0;
    }
    if bin_count != 0
        && (f_1c010970(m, bin_count, bin_table, word) != 0
            || (stripped == 1 && f_1c010970(m, bin_count, bin_table, buf) != 0))
    {
        m.set_sp(sp);
        return 1;
    }
    if seq_count != 0
        && (f_1c0108b0(m, seq_count, seq_table, word) != 0
            || (stripped == 1 && f_1c0108b0(m, seq_count, seq_table, buf) != 0))
    {
        m.set_sp(sp);
        return 1;
    }
    m.set_sp(sp);
    0
}

/// Halving search (step n/4, n/8, ...) from the middle, then linear walk; 1 = found.
pub fn f_1c0108b0(m: &mut Mem, p1: u32, p2: u32, p3: u32) -> u32 {
    let count = p1;
    let table = p2;
    let word = p3;
    let sp = m.sp();
    // The original reuses its own p1 stack slot as the out-pointer.
    let out = m.alloca(4);
    m.w32(out, p1);
    let mut step = (count as i32) >> 1;
    let mut idx = step as u32;
    step >>= 1;
    let mut r = f_1c0109d0(m, count, table, idx, out, word);
    loop {
        if r == 1 {
            m.set_sp(sp);
            return 1;
        }
        if r == 0xffff {
            idx = idx.wrapping_add(step as u32);
        } else {
            idx = idx.wrapping_sub(step as u32);
        }
        if step == 0 {
            break;
        }
        step >>= 1;
        r = f_1c0109d0(m, count, table, idx, out, word);
    }
    if r == 0xffff {
        loop {
            idx = idx.wrapping_add(1);
            r = f_1c0109d0(m, count, table, idx, out, word);
            if r != 0xffff {
                break;
            }
        }
    } else if r == 0xfffe {
        loop {
            idx = idx.wrapping_sub(1);
            r = f_1c0109d0(m, count, table, idx, out, word);
            if r != 0xfffe {
                break;
            }
        }
    }
    m.set_sp(sp);
    (r == 1) as u32
}

/// Binary search of `p3` in the sorted table `p2` of `p1` entries; 1 = found.
pub fn f_1c010970(m: &mut Mem, p1: u32, p2: u32, p3: u32) -> u32 {
    let mut lo: i32 = 0;
    let mut hi = (p1 as i32).wrapping_sub(1);
    if hi < 0 {
        return 0;
    }
    loop {
        let mid = lo.wrapping_add(hi) >> 1;
        let r = f_1c010a80(m, p1, p2, mid as u32, p3);
        if r == 0xfffe {
            hi = mid - 1;
        } else if r == 0xffff {
            lo = mid + 1;
        } else {
            return 1;
        }
        if lo > hi {
            return 0;
        }
    }
}

/// Compare entry `p3` of table `p2` (count `p1`) with `p5`; stores the entry at *p4.
/// 1 = equal, 0xffff = search upward, 0xfffe = search downward.
pub fn f_1c0109d0(m: &mut Mem, p1: u32, p2: u32, p3: u32, p4: u32, p5: u32) -> u32 {
    let last = (p1 as i32).wrapping_sub(1);
    let idx = p3 as i32;
    if idx < 0 {
        return 0xffff;
    }
    if idx > last {
        return 0xfffe;
    }
    let entry = m.r32(p2.wrapping_add(p3.wrapping_mul(4)));
    m.w32(p4, entry);
    let word = p5;
    let mut i: u32 = 0;
    let mut cl = m.r8(entry + 4);
    while cl != 0 {
        let dl = m.r8(word + i);
        if dl == 0 {
            return 0xfffe;
        }
        let cl2 = m.r8(entry + i + 4);
        if dl != cl2 && (class16(m, cl2 as u32) & 4 == 0 || dl != fold_u(m, cl2 as u32)) {
            if idx == 0 {
                return 0xffff;
            }
            if idx == last {
                return 0xfffe;
            }
            return f_1c010b20(m, entry, word);
        }
        cl = m.r8(entry + i + 5);
        i += 1;
    }
    if m.r8(word + i) == 0 { 1 } else { 0xffff }
}

/// Compare entry `p3` of sorted table `p2` with `p4`: 1 = equal, 0xffff / 0xfffe = direction.
pub fn f_1c010a80(m: &mut Mem, p1: u32, p2: u32, p3: u32, p4: u32) -> u32 {
    let _ = p1;
    let key = m.r32(p2.wrapping_add(p3.wrapping_mul(4))) + 4;
    let word = p4;
    let mut i: u32 = 0;
    let mut cl = m.r8(key);
    while cl != 0 {
        let dl = m.r8(word + i);
        if dl as u32 != sx8(cl as u32) {
            if dl == 0 {
                return 0xfffe;
            }
            let c = m.r8(key + i);
            if class16(m, c as u32) & 4 == 0 || dl != fold_s(m, c as u32) {
                return f_1c010b90(m, key, word);
            }
        }
        cl = m.r8(key + i + 1);
        i += 1;
    }
    if m.r8(word + i) != 0 { 0xffff } else { 1 }
}

/// Case-folded ordering of entry `p1` (key at +4) vs `p2`: 0xffff if entry < word
/// (also when both are equal), else 0xfffe.
pub fn f_1c010b20(m: &mut Mem, p1: u32, p2: u32) -> u32 {
    let entry = p1;
    let word = p2;
    let mut i: u32 = 0;
    let mut al = m.r8(word);
    let mut dl: u8;
    if al == 0 {
        dl = p2 as u8;
    } else {
        loop {
            dl = fold_u(m, m.r8(entry + i + 4) as u32);
            if fold_u(m, al as u32) != dl {
                break;
            }
            i += 1;
            al = m.r8(word + i);
            if al == 0 {
                break;
            }
        }
    }
    let al = m.r8(word + i);
    if al == 0 && m.r8(entry + i + 4) == 0 {
        return 0xffff;
    }
    if dl < fold_u(m, al as u32) { 0xffff } else { 0xfffe }
}

/// Case-folded ordering of key `p1` vs `p2` (signed fold index for the key byte).
pub fn f_1c010b90(m: &mut Mem, p1: u32, p2: u32) -> u32 {
    let key = p1;
    let word = p2;
    let mut i: u32 = 0;
    let mut al = m.r8(word);
    let mut dl: u8;
    if al == 0 {
        dl = p2 as u8;
    } else {
        loop {
            dl = fold_s(m, m.r8(key + i) as u32);
            if fold_u(m, al as u32) != dl {
                break;
            }
            i += 1;
            al = m.r8(word + i);
            if al == 0 {
                break;
            }
        }
    }
    let bl = fold_u(m, m.r8(word + i) as u32);
    if dl < bl { 0xffff } else { 0xfffe }
}

/// Clear the matcher's 0x12c-byte scratch block.
pub fn f_1c010bf0(m: &mut Mem, p1: u32) -> u32 {
    for k in 0..0x12c {
        m.w8(p1 + k, 0);
    }
    0
}

fn aborted(m: &Mem, kernel: u32) -> bool {
    m.r32(kernel + 0x170) != 0 || m.r32(kernel + 0x16c) == 1
}

/// Rule-driven rewrite of text `p2` (copied to `p3`, records `p6` -> `p7`) into output
/// `p4`/`p8`, using rule section `p11`, attribute bytes `p5`, masks `p9`/`p10`, mode `p12`
/// and cursor `p13`. Returns `p13`, or cmd+0x3224 when interrupted.
pub fn f_1c010c00(m: &mut Mem, p1: u32, p2: u32, p3: u32, p4: u32, p5: u32, p6: u32, p7: u32, p8: u32, p9: u32, p10: u32, p11: u32, p12: u32, p13: u32) -> u32 {
    let cmd = m.r32(p1 + 0xc);
    let kernel = m.r32(p1 + 8);
    let sp = m.sp();
    // Frame addressed with the disassembly's esp offsets (after the 4 register pushes).
    let fr = m.alloca(0x1b8).wrapping_sub(0x10);
    for k in 0x10..0x1c8 {
        m.w8(fr + k, 0);
    }
    let depth = fr + 0x18;
    let save = fr + 0x1c;
    let cur = fr + 0x34;
    let stack = fr + 0x64;
    let tail = fr + 0x78;
    let scratch = fr + 0x9c;

    m.w16(cur, m.s(p13 + 2).wrapping_add(m.s(p13)));
    m.w16(cur + 4, m.s(p13 + 4).wrapping_add(m.s(p13 + 6)));
    m.w16(cur + 0x10, m.s(p13 + 0x10));

    if (p11 as i16) > m.r16(0x1c06_4e04) {
        fwd_copy(m, p4, 0x1c06_4088, 23);
        m.set_sp(sp);
        return p13;
    }
    for k in 0..5 {
        m.w32(stack + k * 4, 0xffff_ffff);
    }
    let len = strlen(m, p2);
    fwd_copy(m, p3, p2, len + 1);
    let n = sx16(len);
    fwd_copy(m, p7, p6, n.wrapping_mul(6));
    let max = (n.wrapping_mul(2) as i32) / 3;
    let rule_count = m.r16(0x1c06_4e0e);

    let mut done: u32 = 0;
    let mut shift: u32 = 0;
    loop {
        let sb = m.s(cur);
        let so = m.s(cur + 2);
        let ch = m.r8(p3.wrapping_add(sb.wrapping_add(so) as u32));
        if (ch == 0 || p12 as u16 != 0)
            && (sb - (shift as i16 as i32) + so >= (max as i16 as i32) || p12 as u16 != 1)
        {
            m.w8(p4.wrapping_add((m.s(cur + 4) + m.s(cur + 6)) as u32), 0);
            let so = m.r32(cur + 2).wrapping_sub(shift);
            m.w16(p13 + 2, so as i32);
            m.w16(p13 + 6, m.s(cur + 6));
            m.set_sp(sp);
            return p13;
        }
        if aborted(m, kernel) {
            f_1c005980(m, p1);
            m.set_sp(sp);
            return cmd + 0x3224;
        }
        if f_1c014c40(m, p3, p7, p4, p8, cur) == 0xffff_ffff {
            done = 1;
        }
        f_1c014dc0(m, save, cur);
        let mut rule = m.r16(0x1c06_4e08u32.wrapping_add(sx16(p11).wrapping_mul(2))) as u16 as u32;
        let mut state: u32 = 0;
        if done as u16 == 0 {
            loop {
                if aborted(m, kernel) {
                    f_1c005980(m, p1);
                    m.set_sp(sp);
                    return cmd + 0x3224;
                }
                f_1c010bf0(m, scratch);
                let rp = 0x1c06_4eb8u32
                    .wrapping_add(sx16(m.r16(0x1c06_4e10u32.wrapping_add(sx16(rule).wrapping_mul(2))) as u32));
                match m.r16(rp) {
                    0 => {}
                    1 => {
                        m.w32(depth, 0);
                        if (state as i16) > 0 {
                            f_1c014dc0(m, cur, tail);
                        }
                        break;
                    }
                    3 => {
                        rule = (rule & 0xffff_0000) | m.r16(rp + 2) as u16 as u32;
                        continue;
                    }
                    4 => {
                        rule = rule.wrapping_add(1);
                        f_1c0116d0(m, stack, depth, rule);
                        rule = (rule & 0xffff_0000) | m.r16(rp + 2) as u16 as u32;
                        continue;
                    }
                    2 => {
                        rule = f_1c011690(m, stack, depth, rule);
                        continue;
                    }
                    _ => break,
                }
                m.w16(cur + 8, 0);
                m.w16(cur + 0xa, 0);
                if state as u16 == 0 {
                    let m10 = m.r32(rp + 8);
                    if p9 & m.r32(rp + 4) == 0 || (m10 != 0xffff_ffff && p10 & m10 == 0) {
                        rule = rule.wrapping_add(1);
                        if rule as i16 >= rule_count {
                            break;
                        }
                        continue;
                    }
                }
                let cond = m.r16(rp + 0x16);
                let attempt = if cond == -1 {
                    true
                } else {
                    let a = m.r8(p5.wrapping_add((m.s(cur) + m.s(cur + 2)) as u32));
                    (cond == 1 && a != 0) || (cond == 0 && a == 0)
                };
                if !attempt {
                    rule = rule.wrapping_add(1);
                    if rule as i16 >= rule_count {
                        break;
                    }
                    done = 0;
                    continue;
                }

                if aborted(m, kernel) {
                    f_1c005980(m, p1);
                    m.set_sp(sp);
                    return cmd + 0x3224;
                }
                let ret = f_1c0117b0(m, rp + 0x18, 0, p3, p4, p7, p8, scratch, cur, 0, 0);
                if std::env::var_os("DT_RULE_TRACE").is_some() {
                    let st: String = (0..0x18).map(|k| format!("{:02x}", m.r8(cur + k))).collect();
                    eprintln!("rule={:08x} ret={ret:08x} st={st}", rp + 0x18);
                }
                if aborted(m, kernel) {
                    f_1c005980(m, p1);
                    m.set_sp(sp);
                    return cmd + 0x3224;
                }
                m.w16(p13 + 0x10, m.s(cur + 0x10));
                let mut hit = false;
                if m.r16(cur + 0xa) == 1 {
                    let pos = p3.wrapping_add(m.s(cur + 2) as u32).wrapping_add(m.s(cur) as u32);
                    let c = m.r8(pos) as u32;
                    hit = c == 0 || class16(m, c) & 0x20 != 0 || (class16(m, c) & 0x800 != 0 && {
                        let nx = m.r8(pos.wrapping_add(1)) as u32;
                        class16(m, nx) & 0x20 != 0 || nx == 0
                    });
                    if !hit {
                        f_1c014dc0(m, cur, save);
                    }
                }
                if hit {
                    if m.r16(rp + 0xe) == -1 && m.r16(rp + 0x12) == -1 {
                        done = 1;
                        state = 0;
                    } else {
                        f_1c014dc0(m, tail, cur);
                        rewrite(m, p3, p4, p5, p7, p8, save, cur, tail, &mut shift);
                        if m.r16(rp + 0x12) == -1 {
                            rule = (rule & 0xffff_0000) | m.r16(rp + 0xe) as u16 as u32;
                        } else {
                            let next = m.r16(rp + 0xe);
                            let ret = if next != -1 { next as u16 as u32 } else { rule.wrapping_add(1) };
                            f_1c0116d0(m, stack, depth, ret);
                            rule = (rule & 0xffff_0000) | m.r16(rp + 0x12) as u16 as u32;
                        }
                        f_1c014dc0(m, save, cur);
                        state = 1;
                    }
                } else if m.r16(rp + 0x14) != -1 {
                    let next = m.r16(rp + 0x10);
                    let ret = if next != -1 {
                        next as u16 as u32
                    } else {
                        rule = rule.wrapping_add(1);
                        rule
                    };
                    f_1c0116d0(m, stack, depth, ret);
                    rule = (rule & 0xffff_0000) | m.r16(rp + 0x14) as u16 as u32;
                    state = 0xffff_ffff;
                } else {
                    let next = m.r16(rp + 0x10);
                    if next != -1 {
                        // ebx = ebp with its low word replaced: upper bits come from the rule pointer.
                        rule = (rp & 0xffff_0000) | next as u16 as u32;
                        state = 0xffff_ffff;
                    } else {
                        rule = rule.wrapping_add(1);
                        state = 0;
                    }
                }
                if rule as i16 >= rule_count || (rule as i16) < 0 || done as u16 != 0 {
                    break;
                }
            }
        }
        let r = f_1c013fe0(m, p3, p4, p7, p8, cur);
        done = (r == 0xffff_ffff) as u32;
    }
}

/// Splice the output produced since `save` (p4/p8 at S.db+S.do, length C.do-S.do) back into
/// the input (p3/p5/p7) over the consumed span (length C.so-S.so), and rewind the cursor
/// onto it.
#[allow(clippy::too_many_arguments)]
fn rewrite(m: &mut Mem, p3: u32, p4: u32, p5: u32, p7: u32, p8: u32, save: u32, cur: u32, tail: u32, shift: &mut u32) {
    let src_len = m.r32(cur + 2).wrapping_sub(m.r32(save + 2));
    let dst_len = m.r32(cur + 6).wrapping_sub(m.r32(save + 6));
    let out_src = p4.wrapping_add(m.s(save + 4) as u32).wrapping_add(m.s(save + 6) as u32);
    let out_rec = m.r32(save + 6).wrapping_add(m.r32(save + 4));
    if (src_len as i16) < (dst_len as i16) {
        let grow = dst_len.wrapping_sub(src_len);
        let s_so = m.r16(save + 2);
        if s_so > grow as i16 {
            let dst = p3
                .wrapping_add(m.s(save) as u32)
                .wrapping_add((s_so as i32 - grow as i16 as i32) as u32);
            fwd_copy(m, dst, out_src, sx16(dst_len));
            let idx = m.r32(save).wrapping_mul(2).wrapping_sub(grow);
            f_1c011710(m, p7, idx, p8, out_rec, dst_len);
            let so = m.r32(save + 2).wrapping_sub(grow);
            let s_do = m.s(save + 6);
            m.w16(cur + 2, so as i32);
            m.w16(cur + 6, s_do);
            m.w16(save + 2, so as i32);
        } else {
            let lim = m.r32(cur + 2).wrapping_add(m.r32(cur));
            let mut hi = strlen(m, p3).wrapping_add(grow);
            if (hi as i16) > (lim as i16) {
                let mut idx = sx16(hi);
                loop {
                    let from = idx.wrapping_sub(sx16(grow));
                    let b = m.r8(p3.wrapping_add(from));
                    m.w8(p3.wrapping_add(idx), b);
                    f_1c011750(m, p7, hi, p7, hi.wrapping_sub(grow));
                    hi = hi.wrapping_sub(1);
                    let b = m.r8(p5.wrapping_add(from));
                    m.w8(p5.wrapping_add(idx), b);
                    idx = idx.wrapping_sub(1);
                    if (hi as i16) <= (lim as i16) {
                        break;
                    }
                }
            }
            *shift = shift.wrapping_add(grow);
            let dst = p3.wrapping_add(m.s(save + 2) as u32).wrapping_add(m.s(save) as u32);
            fwd_copy(m, dst, out_src, sx16(dst_len));
            let idx = m.r32(save + 2).wrapping_add(m.r32(save));
            f_1c011710(m, p7, idx, p8, out_rec, dst_len);
            m.w16(tail + 2, m.s(tail + 2).wrapping_add(grow as i32));
            m.w16(cur + 2, m.s(save + 2));
            m.w16(cur + 6, m.s(save + 6));
        }
    } else if src_len as i16 == dst_len as i16 {
        let dst = p3.wrapping_add(m.s(save + 2) as u32).wrapping_add(m.s(save) as u32);
        fwd_copy(m, dst, out_src, sx16(src_len));
        let idx = m.r32(save + 2).wrapping_add(m.r32(save));
        f_1c011710(m, p7, idx, p8, out_rec, src_len);
        m.w16(cur + 2, m.s(save + 2));
        m.w16(cur + 6, m.s(save + 6));
    } else {
        let shrink = src_len.wrapping_sub(dst_len);
        let dst = p3
            .wrapping_add(m.s(save + 2) as u32)
            .wrapping_add(sx16(shrink))
            .wrapping_add(m.s(save) as u32);
        fwd_copy(m, dst, out_src, sx16(dst_len));
        let idx = m.r32(save + 2).wrapping_add(shrink).wrapping_add(m.r32(save));
        f_1c011710(m, p7, idx, p8, out_rec, dst_len);
        let so = shrink.wrapping_add(m.r32(save + 2));
        m.w16(cur + 2, so as i32);
        m.w16(cur + 6, m.s(save + 6));
    }
}

/// Pop from the 10-entry return stack `p1` (depth *p2). Underflow returns p3+1.
pub fn f_1c011690(m: &mut Mem, p1: u32, p2: u32, p3: u32) -> u32 {
    let d = m.r16(p2);
    if d > 0 {
        let d = d - 1;
        m.w16(p2, d as i32);
        return m.r16(p1.wrapping_add((d as i32 * 2) as u32)) as u16 as u32;
    }
    sx16(p3).wrapping_add(1)
}

/// Push p3 onto the 10-entry return stack `p1` (depth *p2); overflow is only logged.
pub fn f_1c0116d0(m: &mut Mem, p1: u32, p2: u32, p3: u32) -> u32 {
    let d = m.r16(p2);
    if d < 10 {
        m.w16(p1.wrapping_add((d as i32 * 2) as u32), p3 as i32);
        m.w16(p2, d.wrapping_add(1) as i32);
        return p2;
    }
    // EAX = printf's return value.
    0
}

/// Copy p5 6-byte records: p1[p2..] = p3[p4..].
pub fn f_1c011710(m: &mut Mem, p1: u32, p2: u32, p3: u32, p4: u32, p5: u32) -> u32 {
    let bytes = sx16(p5).wrapping_mul(6);
    let src = p3.wrapping_add(sx16(p4).wrapping_mul(6));
    let dst = p1.wrapping_add(sx16(p2).wrapping_mul(6));
    fwd_copy(m, dst, src, bytes);
    bytes
}

/// Copy one 6-byte record: p1[p2] = p3[p4]. Returns the destination address.
pub fn f_1c011750(m: &mut Mem, p1: u32, p2: u32, p3: u32, p4: u32) -> u32 {
    let src = p3.wrapping_add(sx16(p4).wrapping_mul(6));
    let dst = p1.wrapping_add(sx16(p2).wrapping_mul(6));
    let lo = m.r32(src);
    m.w32(dst, lo);
    let hi = m.s(src + 4);
    m.w16(dst + 4, hi);
    dst
}

/// Is record p1[p2] non-zero? Only AX is defined; the upper half is the record address's.
pub fn f_1c011780(m: &mut Mem, p1: u32, p2: u32) -> u32 {
    let rec = p1.wrapping_add(sx16(p2).wrapping_mul(6));
    let nz = m.r16(rec) != 0 || m.r16(rec + 2) != 0 || m.r16(rec + 4) != 0;
    (rec & 0xffff_0000) | nz as u32
}

/// Copy a run of non-separator characters (and their records) from input to output at
/// cursor `p5`, advancing it. Returns -1 at end of input, else 0.
pub fn f_1c013fe0(m: &mut Mem, p1: u32, p2: u32, p3: u32, p4: u32, p5: u32) -> u32 {
    if p5 == 0 || p1 == 0 || p2 == 0 {
        return 0;
    }
    let src = m.s(p5).wrapping_add(m.s(p5 + 2)) as u32;
    let dst = m.s(p5 + 4).wrapping_add(m.s(p5 + 6)) as u32;
    let mut i: u32 = 0;
    let mut c = m.r8(p1.wrapping_add(src));
    if class16(m, c as u32) & 0x20 == 0 {
        while c != 0 {
            m.w8(p2.wrapping_add(dst + i), c);
            f_1c011750(m, p4, i.wrapping_add(dst), p3, i.wrapping_add(src));
            i += 1;
            c = m.r8(p1.wrapping_add(src).wrapping_add(i));
            if class16(m, c as u32) & 0x20 != 0 {
                break;
            }
        }
    }
    m.w16(p5 + 2, m.s(p5 + 2).wrapping_add(i as i32));
    m.w16(p5 + 6, m.s(p5 + 6).wrapping_add(i as i32));
    if m.r8(p1.wrapping_add(i).wrapping_add(src)) != 0 { 0 } else { 0xffff_ffff }
}

/// Copy a run of separator characters from input to output at cursor `p5`; after the first,
/// characters whose record is all-zero are dropped. Returns -1 at end of input, else 0.
pub fn f_1c014c40(m: &mut Mem, p1: u32, p2: u32, p3: u32, p4: u32, p5: u32) -> u32 {
    if p5 == 0 || p1 == 0 || p3 == 0 {
        return 0;
    }
    let src = m.s(p5 + 2).wrapping_add(m.s(p5)) as u32;
    let dst = m.s(p5 + 4).wrapping_add(m.s(p5 + 6)) as u32;
    let base = p1.wrapping_add(src);
    let mut emitted: u32 = 0;
    let mut i: u32 = 0;
    if class16(m, m.r8(base) as u32) & 0x20 != 0 {
        loop {
            let c = m.r8(base.wrapping_add(i));
            if c == 0 {
                break;
            }
            if i == 0 || f_1c011780(m, p2, src.wrapping_add(i)) as u16 != 0 {
                m.w8(p3.wrapping_add(emitted.wrapping_add(dst)), c);
                if f_1c011780(m, p2, src.wrapping_add(i)) as u16 != 0 {
                    f_1c011750(m, p4, dst.wrapping_add(emitted), p2, src.wrapping_add(i));
                }
                emitted = emitted.wrapping_add(1);
            }
            i += 1;
            if class16(m, m.r8(base.wrapping_add(i)) as u32) & 0x20 == 0 {
                break;
            }
        }
    }
    m.w16(p5 + 2, m.s(p5 + 2).wrapping_add(i as i32));
    if (i as i32) > 0 {
        m.w16(p5 + 6, m.s(p5 + 6).wrapping_add(emitted as i32));
    }
    if m.r8(p1.wrapping_add(i).wrapping_add(src)) != 0 { 0 } else { 0xffff_ffff }
}

/// Debug trace of rule matching (printf only).
pub fn f_1c014d60(m: &mut Mem, p1: u32, p2: u32, p3: u32) -> u32 {
    if std::env::var_os("DT_RULE_TRACE").is_some() {
        let s = |m: &Mem, a: u32| -> String {
            (0..200).map(|k| m.r8(a.wrapping_add(k))).take_while(|&b| b != 0).map(|b| b as char).collect()
        };
        let rule = if p2 > 0x1000_0000 { s(m, p2) } else { format!("{p2:#x}") };
        eprintln!("parser message: {:?} rule {:?} at {}", s(m, p1), rule, p3 as i32);
    }
    // EAX = the last printf's return value.
    0
}

/// Copy a 24-byte cursor: *p1 = *p2. EAX is left as the caller's.
pub fn f_1c014dc0(m: &mut Mem, p1: u32, p2: u32) -> u32 {
    fwd_copy(m, p1, p2, 24);
    0
}
