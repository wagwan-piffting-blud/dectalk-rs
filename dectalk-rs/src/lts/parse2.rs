use super::*;

/// Byte table of per-character class bits (bit 0x20 consonant, 0x40 vowel), indexed by signed char.
const CHAR_CLASS: u32 = 0x1c02fd80;
/// Byte table mapping input characters to form-class letters ('a'..'z', '\'').
const CHAR_FC: u32 = 0x1c02fa80;
/// Packed byte-code stream for the form-class rules / suffix patterns.
const FC_CODE: u32 = 0x1c04f200;
/// u16 per-form-class default flag words.
const FC_FLAGS: u32 = 0x1c032dd8;
/// u16 per-part-of-speech attribute words.
const POS_ATTR: u32 = 0x1c032e18;
/// Language-guess scratch word (NUL-terminated chars) and its length.
const LANG_WORD: u32 = 0x1c06f220;
const LANG_WORD_LEN: u32 = 0x1c06f260;
/// 8 language-guess records of 0x18 bytes: table ptr, hits, score, flag byte, total, result.
const LANG_REC: u32 = 0x1c032270;
const LANG_REC_END: u32 = 0x1c032330;
/// NULL-terminated list of pointers to exclusion patterns checked by 1c00eab0.
const LANG_EXCL: u32 = 0x1c032350;

/// Handle an ordinary word: dictionary / LTS lookup per hyphen-separated piece, with
/// form-class parsing of unknown compound pieces. Always returns 3.
pub fn f_1c00de20(m: &mut Mem, p1: u32, p2: u32, p3: u32) -> u32 {
    let ctx = p1;
    let end = p3;
    let lts = m.r32(ctx + 0x10);
    let mut first = true;
    let mut cur = p2;
    if cur == end {
        return 3;
    }
    loop {
        // de49
        let f = m.r32(lts + 0x8f8) & 0xffff_ff4f;
        m.w32(lts + 0x8f8, f);
        let mut start = cur;
        loop {
            let c = m.s(cur);
            if c == 0x2d {
                break;
            }
            let attr = m.r16(0x1c03244c_u32.wrapping_add((c as u32).wrapping_mul(2))) as u16;
            let or = if attr & 0x400 != 0 {
                Some(0xa0)
            } else if attr & 0x200 != 0 {
                Some(0x90)
            } else if c == 0x79 || c == 0x59 {
                Some(if cur == start { 0x20 } else { 0x10 })
            } else {
                None
            };
            if let Some(v) = or {
                let f = m.r32(lts + 0x8f8) | v;
                m.w32(lts + 0x8f8, f);
            }
            cur = cur.wrapping_add(8);
            if cur == end {
                break;
            }
        }
        let f = m.r32(lts + 0x8f8);
        m.w32(lts + 0x1114, f);
        let found = f_1c00f4c0(m, ctx, start, cur, 0);
        let mut skip_output = false;
        if found != 0 {
            if first {
                // Quirk: skips one record past the piece even when the piece ended at `end`.
                cur = cur.wrapping_add(8);
                f_1c00ef80(m, ctx, 0x6e);
                first = false;
                skip_output = true;
            } else {
                start = cur;
            }
        } else if m.r8(lts + 0x1114) & 0x10 != 0 && start.wrapping_add(8) != cur {
            f_1c00d2f0(m, start, cur);
            f_1c00bd80(m, ctx, start, cur);
            start = cur;
            if cur != end {
                while m.r16(cur) as u16 == 0x2d {
                    cur = cur.wrapping_add(8);
                    if cur == end {
                        break;
                    }
                }
                let next = start.wrapping_add(8);
                f_1c00ef80(m, ctx, if next != cur { 0x73 } else { 0x6e });
                if next == cur {
                    start = next;
                }
            }
        }
        if !skip_output {
            // df64
            first = false;
            if start != cur {
                if m.r8(lts + 0x8f8) & 0x80 == 0 {
                    f_1c009b00(m, ctx, start, cur);
                }
                if m.r8(lts + 0x8f8) & 0x10 == 0 {
                    f_1c009b00(m, ctx, start, cur);
                }
                if m.r8(lts + 0x8f8) & 0x20 == 0 {
                    f_1c009b00(m, ctx, start, cur);
                }
            }
            if cur == end {
                return 3;
            }
            let f = m.r32(lts + 0x8f8) | 0x40;
            m.w32(lts + 0x8f8, f);
            while m.r16(cur) as u16 == 0x2d {
                cur = cur.wrapping_add(8);
                if cur == end {
                    return 3;
                }
            }
        }
        // deea
        if cur == end {
            return 3;
        }
    }
}

/// Clause-level form-class parse of a word range, with optional language guess, then emit.
pub fn f_1c00bd80(m: &mut Mem, p1: u32, p2: u32, p3: u32) -> u32 {
    let kernel = m.r32(p1 + 8);
    if m.r8(kernel + 0x20c) & 4 != 0 {
        let lang = f_1c00e850(m, p2, p3);
        let dbg = m.r16(kernel + 0x478) as u16;
        if m.r8(kernel + 0x204) & 4 != 0 || (dbg & 0x4000 != 0 && dbg & 0x80 != 0) {
            f_1c00ea90(m, sx16(lang));
        }
        f_1c00b430(m, p1, p2, p3, 0, sx16(lang));
        let v = m.r32(kernel + 0x20c) & 0xffff_fffb;
        m.w32(kernel + 0x20c, v);
    } else {
        f_1c00b430(m, p1, p2, p3, 0, 0);
    }
    f_1c00b780(m, p1);
    0
}

/// Build form-class records for a word range, run the rule matcher, and build/refine the
/// node list at lts+0x8c8.
pub fn f_1c00b430(m: &mut Mem, p1: u32, p2: u32, p3: u32, p4: u32, p5: u32) -> u32 {
    let lts = m.r32(p1 + 0x10);
    let base = lts + 4;
    let mut rec = base;
    let mut src = p2;
    while src < p3 {
        let c = m.s(src) as u32;
        let fc = m.r8(CHAR_FC.wrapping_add(c)) as i32;
        let r = if (0x61..=0x7a).contains(&fc) {
            Some(f_1c00b8d0(m, lts, rec, (fc - 0x60) as u32))
        } else if fc == 0x27 {
            Some(f_1c00b8d0(m, lts, rec, 0x1d))
        } else {
            None
        };
        if let Some(r) = r
            && r != 0
        {
            rec += 8;
        }
        src += 8;
    }
    let head = lts + 0x8c8;
    m.w16(rec, 0);
    let dflt = m.s(FC_FLAGS);
    m.w16(rec + 2, dflt);
    m.w32(rec + 4, 0);
    m.w32(lts + 0x8d0, 0);
    let mut locked = false;
    m.w32(head, head);
    m.w32(lts + 0x8cc, head);
    while rec != base {
        rec = f_1c00b9b0(m, p1, rec, p4, p5);
        let mut pc = m.r16(lts) as u16;
        if pc == 0 {
            continue;
        }
        let n = m.r8(FC_CODE + pc as u32);
        if n != 0 {
            pc = pc.wrapping_add(1);
            m.w16(lts, pc as i32);
            let mut code = n as u32;
            loop {
                if rec < lts + 0x3fc && f_1c00b8d0(m, lts, rec, code) != 0 {
                    rec += 8;
                }
                let pc = m.r16(lts) as u16;
                code = m.r8(FC_CODE + pc as u32) as u32;
                m.w16(lts, pc.wrapping_add(1) as i32);
                if code == 0 {
                    break;
                }
            }
            m.w16(rec, 0);
            let dflt = m.s(FC_FLAGS);
            m.w16(rec + 2, dflt);
            m.w32(rec + 4, 0);
        } else {
            m.w16(lts, pc.wrapping_add(1) as i32);
        }
        let mut stressed = false;
        let pc = m.r16(lts) as u16;
        let mut op = m.r8(FC_CODE + pc as u32) as u32;
        m.w16(lts, pc.wrapping_add(1) as i32);
        if op == 0 {
            continue;
        }
        loop {
            let first = m.r32(head);
            match op {
                0x6c | 0x6d | 0x6e => {
                    if first != head {
                        let bit = 1 << (op - 0x6c);
                        let v = m.r32(first + 0xc) | bit;
                        m.w32(first + 0xc, v);
                    }
                    stressed = false;
                    locked = false;
                }
                0x74 => {
                    if first != head {
                        let v = m.r32(first + 0xc) | 8;
                        m.w32(first + 0xc, v);
                    }
                }
                0x73 => {
                    if !locked && first != head {
                        let v = m.r32(first + 0xc) | 0x10;
                        m.w32(first + 0xc, v);
                    }
                }
                0x79..=0x7e => {
                    if op != 0x7a {
                        stressed = true;
                    }
                    if !locked && first != head {
                        m.w32(first + 0x10, op);
                    }
                }
                _ => {
                    if op & 0x80 != 0 {
                        let pc = m.r16(lts) as u16;
                        let arg = m.r8(FC_CODE + pc as u32) as u32;
                        m.w16(lts, pc.wrapping_add(1) as i32);
                        f_1c00bd30(m, lts, op & 0x7f, arg);
                    } else {
                        f_1c00bd30(m, lts, op, 0);
                    }
                }
            }
            let pc = m.r16(lts) as u16;
            op = m.r8(FC_CODE + pc as u32) as u32;
            m.w16(lts, pc.wrapping_add(1) as i32);
            if op == 0 {
                break;
            }
        }
        if stressed {
            locked = true;
        }
    }
    let mut node = m.r32(head);
    while node != head {
        let next = next_boundary(m, node, head);
        f_1c00b280(m, lts, node, next);
        f_1c00a6e0(m, node, next);
        node = next;
    }
    f_1c00b840(m, lts);
    let mut kind = 0x7c;
    let mut node = m.r32(head);
    while node != head {
        let next = next_boundary(m, node, head);
        f_1c00a910(m, p1, node, next, kind, 0);
        kind = 0x7b;
        node = next;
    }
    f_1c00a2f0(m, lts);
    0
}

/// Next node after `node` that starts a phrase (flags & 7), or `head`.
fn next_boundary(m: &Mem, node: u32, head: u32) -> u32 {
    let mut n = m.r32(node);
    while n != head && m.r8(n + 0xc) & 7 == 0 {
        n = m.r32(n);
    }
    n
}

/// Retag phrase-final 0x2a / 0x30 nodes in [p2, p3) by the preceding node's attributes,
/// inserting a 0x12 node when required.
pub fn f_1c00b280(m: &mut Mem, p1: u32, p2: u32, p3: u32) -> u32 {
    let mut node = p2;
    while node != p3 {
        let cls = m.r32(node + 0x14);
        let tail = m.r8(node + 0xc) & 0xf != 0 && m.r32(node) == p3 && node != p2;
        if cls == 0x2a && tail {
            let prev_cls = m.r32(m.r32(node + 4) + 0x14);
            let attr = m.r16(POS_ATTR.wrapping_add(prev_cls.wrapping_mul(2))) as u16 as u32;
            if attr & 0x11 == 0x11 {
                if f_1c00b3b0(m, p1, node, 0x12, 0, 0x7a) == 0 {
                    return 0;
                }
                node = m.r32(node);
                continue;
            }
            if attr & 9 == 1 {
                m.w32(node + 0x14, 0x29);
                node = m.r32(node);
                continue;
            }
        }
        if cls == 0x30 && tail {
            let prev_cls = m.r32(m.r32(node + 4) + 0x14);
            if prev_cls == 0x2f || prev_cls == 0x30 {
                if f_1c00b3b0(m, p1, node, 0x12, 0, 0x7a) == 0 {
                    return 0;
                }
            } else if m.r8(POS_ATTR.wrapping_add(prev_cls.wrapping_mul(2))) & 9 == 1 {
                m.w32(node + 0x14, 0x2f);
            }
        }
        node = m.r32(node);
    }
    0
}

/// Merge `node`'s predecessor into it: set class, OR flags, max the stress, free the predecessor.
pub fn f_1c00b350(m: &mut Mem, p1: u32, p2: u32, p3: u32) -> u32 {
    let prev = m.r32(p2 + 4);
    m.w32(p2 + 0x14, p3);
    let fl = m.r32(p2 + 0xc) | m.r32(prev + 0xc);
    m.w32(p2 + 0xc, fl);
    let ps = m.i(prev + 0x10);
    if ps > m.i(p2 + 0x10) {
        m.w32(p2 + 0x10, ps as u32);
    }
    f_1c00b410(m, p1, prev);
    0
}

/// Attribute bit 5 of the node's class.
pub fn f_1c00b390(m: &mut Mem, p1: u32) -> u32 {
    let cls = m.r32(p1 + 0x14);
    (m.r8(POS_ATTR.wrapping_add(cls.wrapping_mul(2))) as u32 >> 5) & 1
}

/// Insert a new node (class p3, arg p4, stress p5) before p2, moving p2's flags onto it.
pub fn f_1c00b3b0(m: &mut Mem, p1: u32, p2: u32, p3: u32, p4: u32, p5: u32) -> u32 {
    let n = f_1c00be40(m, p1);
    if n == 0 {
        return 0;
    }
    let prev = m.r32(p2 + 4);
    m.w32(prev, n);
    m.w32(n, p2);
    m.w32(p2 + 4, n);
    m.w32(n + 4, prev);
    m.w32(n + 8, 0);
    m.w32(n + 0x14, p3);
    m.w32(n + 0x18, p4);
    let fl = m.r32(p2 + 0xc);
    m.w32(n + 0xc, fl);
    m.w32(p2 + 0xc, 0);
    m.w32(n + 0x10, p5);
    m.w32(p2 + 0x10, 0x79);
    1
}

/// Unlink node p2 and return it to the free pool.
pub fn f_1c00b410(m: &mut Mem, p1: u32, p2: u32) -> u32 {
    let prev = m.r32(p2 + 4);
    let next = m.r32(p2);
    m.w32(prev, next);
    m.w32(next + 4, prev);
    f_1c00be70(m, p1, p2);
    0
}

/// Emit the node list as PH symbols (phrase markers, stress, class) and free the nodes.
pub fn f_1c00b780(m: &mut Mem, p1: u32) -> u32 {
    let ctx = p1;
    let lts = m.r32(ctx + 0x10);
    let head = lts + 0x8c8;
    let mut stress = 0u32;
    let mut node = m.r32(head);
    while node != head {
        let fl = m.r8(node + 0xc);
        if fl & 1 != 0 {
            f_1c00ef80(m, ctx, 0x6c);
        }
        if m.r8(node + 0xc) & 2 != 0 {
            f_1c00ef80(m, ctx, 0x6d);
        }
        if m.r8(node + 0xc) & 4 != 0 {
            f_1c00ef80(m, ctx, 0x6e);
        }
        if m.r8(node + 0xc) & 0x10 != 0 {
            stress = m.r32(node + 0x10);
        }
        if stress != 0x7a && f_1c00a8d0(m, node) == 0 {
            if stress == 0x7c {
                f_1c00ef80(m, ctx, 0x67);
            } else if stress == 0x7b {
                f_1c00ef80(m, ctx, 0x66);
            }
            stress = 0x7a;
        }
        let cls = m.r32(node + 0x14);
        f_1c00ef80(m, ctx, cls);
        node = m.r32(node);
    }
    let mut node = m.r32(head);
    while node != head {
        let next = m.r32(node);
        f_1c00be70(m, lts, node);
        node = next;
    }
    0
}

/// Collapse adjacent node pairs that form a single class (0x1b+0x22, 0x27/0x2f, 0x29/0x2b, dups).
pub fn f_1c00b840(m: &mut Mem, p1: u32) -> u32 {
    let head = p1 + 0x8c8;
    let mut node = m.r32(head);
    while node != head {
        let cls = m.r32(node + 0x14);
        let prev_cls = m.r32(m.r32(node + 4) + 0x14);
        let merge = if cls == 0x1b && prev_cls == 0x22 {
            Some(prev_cls)
        } else if m.r8(node + 0xc) & 0xf != 0 {
            None
        } else if (cls == 0x2f && prev_cls == 0x27) || (cls == 0x27 && prev_cls == 0x2f) {
            Some(0x27)
        } else if (cls == 0x29 && prev_cls == 0x2b) || (cls == 0x2b && prev_cls == 0x29) {
            Some(0x2b)
        } else if cls == prev_cls && f_1c00a8d0(m, node) != 0 {
            Some(m.r32(node + 0x14))
        } else {
            None
        };
        if let Some(c) = merge {
            f_1c00b350(m, p1, node, c);
        }
        node = m.r32(node);
    }
    0
}

/// Store form-class record `p3` at p2 (lts=p1); may fuse with a preceding 0x15 record.
/// Returns 1 if the record pointer should advance, 0 if it was fused.
pub fn f_1c00b8d0(m: &mut Mem, p1: u32, p2: u32, p3: u32) -> u32 {
    let mut rec = p2;
    let ret;
    if f_1c00f670(m, p3) != 0
        && rec > p1 + 0xc
        && m.r16(rec - 8) as u16 == 0x15
        && matches!(m.r16(rec - 0x10) as u16, 7 | 0x11)
    {
        rec -= 8;
        let v = (m.r16(rec - 8) as u16 != 7) as i32 + 0x1b;
        m.w16(rec - 8, v);
        ret = 0;
    } else {
        ret = 1;
    }
    m.w16(rec, p3 as i32);
    m.w32(rec + 4, 0);
    let fl = m.s(FC_FLAGS.wrapping_add(p3.wrapping_mul(2)));
    m.w16(rec + 2, fl);
    if p3 == 0x19 {
        if rec == p1 + 4 {
            let b = m.r8(rec + 2) | 4;
            m.w8(rec + 2, b);
        } else {
            let w = m.s(rec + 2) | 0x8002;
            m.w16(rec + 2, w);
        }
    }
    if rec != p1 + 4 {
        let prev = m.s(rec - 8) as u32;
        if ((prev == 0x13 || prev == 3) && p3 == 8) || (prev == 4 && (p3 == 7 || p3 == 10)) {
            let b = m.r8(rec + 2) | 0x40;
            m.w8(rec + 2, b);
        }
        let w = m.s(rec + 2);
        if w & 4 != 0 && prev == p3 {
            m.w16(rec + 2, w | 0x200);
        }
        if m.r8(rec - 5) & 0x80 != 0 {
            let w = m.s(rec + 2) | 0x8000;
            m.w16(rec + 2, w);
        }
    }
    ret
}

/// Try the rewrite rules keyed on the record before p2; on a match, set the rule's action index
/// at lts+0 and return the new record end, else return p2-8.
pub fn f_1c00b9b0(m: &mut Mem, p1: u32, p2: u32, p3: u32, p4: u32) -> u32 {
    let lts = m.r32(p1 + 0x10);
    let last = p2.wrapping_sub(8);
    let key = m.s(last) as u32;
    let mut count = m.r16(0x1c04b052_u32.wrapping_add(key.wrapping_mul(4))) as u16 as u32;
    let mut idx = m.r16(0x1c04b050_u32.wrapping_add(key.wrapping_mul(4))) as u16 as u32;
    m.w16(lts, 0);
    while count != 0 {
        count -= 1;
        let rule = 0x1c04b050_u32.wrapping_add(idx.wrapping_mul(2));
        'try_rule: {
            let k = m.r16(rule) as u16 as u32;
            let kv = k & 0x7fff;
            if k & 0x8000 != 0 {
                if kv != p4 {
                    break 'try_rule;
                }
            } else if kv != p3 && kv != p4 {
                break 'try_rule;
            }
            let mut pos = last;
            let pat = m.r16(rule + 2) as u16 as u32;
            if pat != 0 {
                let mut c = m.r8(FC_CODE + pat) as i32;
                let mut pp = pat + 1;
                while c != 0 {
                    if pos == lts + 4 {
                        break 'try_rule;
                    }
                    let prev = m.s(pos - 8);
                    pos -= 8;
                    if prev != c {
                        break 'try_rule;
                    }
                    c = m.r8(FC_CODE + pp) as i32;
                    pp += 1;
                }
            }
            let left = m.r16(rule + 8) as u16 as u32;
            if left != 0 && f_1c00bb00(m, lts, left, last, 0) == 0 {
                break 'try_rule;
            }
            let right = m.r16(rule + 6) as u16 as u32;
            if right != 0 && f_1c00bb00(m, lts, right, pos, 1) == 0 {
                break 'try_rule;
            }
            let act = m.s(0x1c04b054_u32.wrapping_add(idx.wrapping_mul(2)));
            m.w16(lts, act);
            return pos;
        }
        idx = idx.wrapping_add(5);
    }
    last
}

/// Match context pattern at code offset p2 against records from p3 in direction p4
/// (0 = forward, else backward). Returns the final record pointer, or 0 on mismatch.
pub fn f_1c00bb00(m: &mut Mem, p1: u32, p2: u32, p3: u32, p4: u32) -> u32 {
    let lts = p1;
    let byte = |m: &Mem, i: u32| m.r8(FC_CODE.wrapping_add(i)) as u32;
    let mut pc = p2 + 1;
    let end = pc + byte(m, p2);
    let mut rec = p3;
    if pc == end {
        return rec;
    }
    loop {
        let op = byte(m, pc);
        pc += 1;
        match op {
            0x1f => {
                let min = byte(m, pc);
                let max = byte(m, pc + 1);
                pc += 2;
                for _ in 0..min {
                    let r = f_1c00bb00(m, lts, pc, rec, p4);
                    if r == 0 {
                        return 0;
                    }
                    rec = r;
                }
                for _ in 0..max {
                    let r = f_1c00bb00(m, lts, pc, rec, p4);
                    if r == 0 {
                        break;
                    }
                    rec = r;
                }
                pc = pc + byte(m, pc) + 1;
            }
            0x20 => {
                let n = byte(m, pc);
                pc += 1;
                let alt_end = pc + n;
                loop {
                    if pc == alt_end {
                        return 0;
                    }
                    let r = f_1c00bb00(m, lts, pc, rec, p4);
                    if r != 0 {
                        rec = r;
                        pc = alt_end;
                        break;
                    }
                    pc = pc + byte(m, pc) + 1;
                }
            }
            0x21 => {
                let mask = byte(m, pc) | (byte(m, pc + 1) << 8);
                let val = byte(m, pc + 2) | (byte(m, pc + 3) << 8);
                pc += 4;
                if p4 == 0 {
                    if m.r16(rec) == 0 {
                        return 0;
                    }
                    rec = rec.wrapping_add(8);
                } else {
                    if rec == lts + 4 {
                        return 0;
                    }
                    rec = rec.wrapping_sub(8);
                }
                if (m.s(rec + 2) as u32) & mask != val {
                    return 0;
                }
            }
            0x1e => {
                if p4 == 0 {
                    if m.r16(rec) == 0 {
                        return 0;
                    }
                    let nx = m.r16(rec + 8);
                    if nx == 0x1e {
                        rec = rec.wrapping_add(8);
                    } else if nx != 0 {
                        return 0;
                    }
                } else if rec != lts + 4 {
                    rec = rec.wrapping_sub(8);
                    if m.r16(rec) != 0x1e {
                        return 0;
                    }
                }
            }
            0x22 => {
                if p4 == 0 {
                    if m.r16(rec) == 0 || m.r16(rec + 8) != 0 {
                        return 0;
                    }
                } else if rec != lts + 4 {
                    return 0;
                }
            }
            _ => {
                if p4 == 0 {
                    if m.r16(rec) == 0 {
                        return 0;
                    }
                    rec = rec.wrapping_add(8);
                } else {
                    if rec == lts + 4 {
                        return 0;
                    }
                    rec = rec.wrapping_sub(8);
                }
                if m.s(rec) as u32 != op {
                    return 0;
                }
            }
        }
        if pc == end {
            return rec;
        }
    }
}

/// Push a new node (class p2, arg p3) at the front of the list at lts+0x8c8.
pub fn f_1c00bd30(m: &mut Mem, p1: u32, p2: u32, p3: u32) -> u32 {
    let n = f_1c00be40(m, p1);
    if n == 0 {
        return 0;
    }
    let head = p1 + 0x8c8;
    let old = m.r32(head);
    m.w32(head, n);
    m.w32(n, old);
    m.w32(old + 4, n);
    m.w32(n + 4, head);
    m.w32(n + 8, 0);
    m.w32(n + 0xc, 0);
    m.w32(n + 0x14, p2);
    m.w32(n + 0x18, p3);
    m.w32(n + 0x10, 0x79);
    n
}

/// Debug-trace stub (always 0).
pub fn f_1c00be30(m: &mut Mem) -> u32 {
    let _ = m;
    0
}

/// Pop a node from the free pool at lts+0x404 (clearing its link fields); 0 if empty.
pub fn f_1c00be40(m: &mut Mem, p1: u32) -> u32 {
    let n = m.r32(p1 + 0x404);
    if n != 0 {
        let next = m.r32(n);
        m.w32(p1 + 0x404, next);
        m.w32(n + 8, 0);
        m.w32(n + 4, 0);
        m.w32(n, 0);
    }
    n
}

/// Push node p2 onto the free pool at lts+0x404.
pub fn f_1c00be70(m: &mut Mem, p1: u32, p2: u32) -> u32 {
    let head = m.r32(p1 + 0x404);
    m.w32(p2, head);
    m.w32(p1 + 0x404, p2);
    p1
}

/// Guess the language/origin of a word from letter-trigram tables; returns the winning
/// record's result value, or 0.
pub fn f_1c00e850(m: &mut Mem, p1: u32, p2: u32) -> u32 {
    let mut len = 0u32;
    let mut src = p1;
    while src < p2 {
        let c = m.r8(src);
        m.w8(LANG_WORD + len, c);
        if m.r8(CHAR_CLASS.wrapping_add(c as i8 as i32 as u32)) & 0x60 == 0 {
            return 0;
        }
        src += 8;
        len += 1;
    }
    m.w32(LANG_WORD_LEN, len);
    m.w8(LANG_WORD + len, 0);
    let len = len as i32;
    if len < 3 {
        return 0;
    }
    if f_1c00eab0(m) != 0 {
        return 0;
    }
    // Original is a 0x44-byte stack buffer; sized generously here.
    let mut letters = vec![0i8; (len as usize) + 3];
    let mut term = 1usize;
    if len > 0 {
        term = len as usize + 1;
        for i in 0..len as usize {
            letters[i + 1] = m.r8(LANG_WORD + i as u32).wrapping_sub(0x60) as i8;
        }
    }
    letters[term] = 0;
    let n = term as i32 - 1;
    let mut r = LANG_REC;
    while r < LANG_REC_END {
        m.w32(r + 0x10, 0);
        m.w32(r + 4, 0);
        m.w32(r + 8, 0);
        m.w8(r + 0xc, 0x2a);
        r += 0x18;
    }
    let mut weight: i32 = if n > 2 { n / 2 } else { 1 };
    let mut step: i32 = -1;
    for i in 0..n.max(0) as usize {
        let tri = (letters[i] as i32 * 27 + letters[i + 1] as i32) * 27 + letters[i + 2] as i32;
        let edge: u8 = if i as i32 > n - 3 {
            0x40
        } else if (i as i32) < 3 {
            0x80
        } else {
            0
        };
        let mut r = LANG_REC;
        while r < LANG_REC_END {
            let tab = m.r32(r);
            let b = m.r8(tab.wrapping_add(tri as u32));
            if b == 0 {
                m.w32(r + 8, 0);
            } else {
                let hits = m.r32(r + 4).wrapping_add(1);
                m.w32(r + 4, hits);
                let v: u8 = if edge & b != 0 {
                    (b & 0x3f).wrapping_mul(5)
                } else if b & 0xc0 != 0 {
                    let q = (b & 0x3f) / 5;
                    if q == 0 { 1 } else { q }
                } else {
                    b & 0x3f
                };
                let score = (m.i(r + 8) / 2).wrapping_add(v as i32);
                m.w32(r + 8, score as u32);
                let total = m.i(r + 0x10).wrapping_add(score.wrapping_mul(weight));
                m.w32(r + 0x10, total as u32);
            }
            r += 0x18;
        }
        if weight == 1 {
            step = 1;
        }
        weight = weight.wrapping_add(step);
    }
    let mut best = 0i32;
    let mut r = LANG_REC;
    while r < LANG_REC_END {
        best = best.max(m.i(r + 4));
        r += 0x18;
    }
    let mut r = LANG_REC;
    while r < LANG_REC_END {
        if m.i(r + 4) != best {
            m.w8(r + 0xc, 0x20);
        }
        r += 0x18;
    }
    let mut best = 0i32;
    let mut r = LANG_REC;
    while r < LANG_REC_END {
        let t = m.i(r + 0x10);
        if best < t && m.r8(r + 0xc) == 0x2a {
            best = t;
        }
        r += 0x18;
    }
    let mut r = LANG_REC;
    while r < LANG_REC_END {
        if best > m.i(r + 0x10) {
            m.w8(r + 0xc, 0x20);
        }
        r += 0x18;
    }
    let mut r = LANG_REC;
    while r < LANG_REC_END {
        if m.r8(r + 0xc) == 0x2a {
            return m.r32(r + 0x14);
        }
        r += 0x18;
    }
    0
}

/// Debug print of the guessed language name (printf only; omitted).
pub fn f_1c00ea90(m: &mut Mem, p1: u32) -> u32 {
    let _ = (m, p1);
    0
}

/// Returns 1 if the word at 0x1c06f220 matches (right-to-left) any exclusion pattern.
/// Pattern chars: '#' word start, '$' any, '*' accept, 'C' consonants, 'N' optional consonant,
/// 'V' 1..2 vowels, other = literal.
pub fn f_1c00eab0(m: &mut Mem) -> u32 {
    let class = |m: &Mem, pos: i32| {
        let c = m.r8(LANG_WORD.wrapping_add(pos as u32)) as i8 as i32 as u32;
        m.r8(CHAR_CLASS.wrapping_add(c))
    };
    let start = m.i(LANG_WORD_LEN).wrapping_sub(1);
    let mut list = LANG_EXCL;
    let mut pat = m.r32(list);
    while pat != 0 {
        let mut pos = start;
        let mut pi = 0u32;
        let mut run = 0i32;
        // true: ran off the start of the word (eb6f); false: mismatch (eb6b)
        let exhausted = loop {
            if pos < 0 {
                break true;
            }
            let pch = m.r8(pat + pi) as u32;
            let ok = match pch {
                0x23 => false,
                0x24 => {
                    pi += 1;
                    true
                }
                0x2a => return 1,
                0x43 => {
                    if class(m, pos) & 0x20 != 0 {
                        run += 1;
                        true
                    } else if run == 0 {
                        false
                    } else {
                        pos += 1;
                        pi += 1;
                        run = 0;
                        true
                    }
                }
                0x4e => {
                    if class(m, pos) & 0x20 == 0 {
                        pos += 1;
                        pi += 1;
                    }
                    true
                }
                0x56 => {
                    if class(m, pos) & 0x40 != 0 {
                        run += 1;
                        true
                    } else if run <= 0 || run > 2 {
                        false
                    } else {
                        pos += 1;
                        pi += 1;
                        run = 0;
                        true
                    }
                }
                _ => {
                    let c = m.r8(LANG_WORD.wrapping_add(pos as u32)) as i8 as i32;
                    if c as u32 != pch {
                        false
                    } else {
                        pi += 1;
                        true
                    }
                }
            };
            if !ok {
                break pos < 0;
            }
            pos -= 1;
        };
        if exhausted && m.r8(pat + pi + 1) == 0x23 {
            return 1;
        }
        list += 4;
        pat = m.r32(list);
    }
    0
}
