//! DTparser rule matching: element dispatch and the `<..>` / `[..]` repetition matchers.
//!
//! Layouts discovered from the code:
//!
//! Match state `st` (0x18 bytes, short fields; callers build it on their stack):
//!   +0x00 s16  source base position
//!   +0x02 s16  source offset added to +0x00 (base_off = st[0] + st[2])
//!   +0x08 s16  pattern position (index into the rule text); written back on exit
//!   +0x0a s16  result code (0 = no match, 1 = in progress / ok, 4 = rule syntax error)
//!   +0x0c s16  mode (1 / -1 / other; -1 = skip matching, only advance the pattern)
//!   +0x14 u32  (zeroed by FUN_1c013830, not touched here)
//!
//! Digit span `span` (FUN_1c0152b0 arg 5, 4 shorts):
//!   +0x00 s16  accumulated low bound, +0x02 s16 high bound, +0x04 s16 low bound,
//!   +0x06 s16  state (0 = unset, 1 = accumulate, 2 -> -1)
//!
//! Rule node (FUN_1c013830 arg 9): +0x0e s16 element kind, +0x14 u32 next node.
//!
//! Tables: 0x1c063b90 u16[256] character-class bits; 0x1c063d94 u8[256] case fold;
//! 0x1c063a64 i8[?][20] element-compatibility bits; string slots at p8 + i*30 (`$0`..`$9`).

use super::*;

const CHAR_BITS: u32 = 0x1c06_3b90;
const CASE_FOLD: u32 = 0x1c06_3d94;
const COMPAT: u32 = 0x1c06_3a64;
const FAIL_A: u32 = 0x1c06_45b0;
const FAIL_B: u32 = 0x1c06_45b8;
const UNBOUNDED: i32 = 0x7fff_ffff;

fn strlen(m: &Mem, s: u32) -> u32 {
    let mut n = 0;
    while m.r8(s.wrapping_add(n)) != 0 {
        n += 1;
    }
    n
}

fn mem_eq(m: &Mem, a: u32, b: u32, n: u32) -> bool {
    (0..n).all(|k| m.r8(a.wrapping_add(k)) == m.r8(b.wrapping_add(k)))
}

fn char_bits(m: &Mem, c: u8) -> u32 {
    m.r16(CHAR_BITS + c as u32 * 2) as u16 as u32
}

/// Look-ahead: can the element following the current one (at rule position `*p7`, or found by
/// scanning from `p4`) start at source position `p2 + p3`? Returns AX = 1 on success.
pub fn f_1c013830(m: &mut Mem, p1: u32, p2: u32, p3: u32, p4: u32, p5: u32, p6: u32, p7: u32, p8: u32, p9: u32) -> u32 {
    let sp = m.sp();
    let frame = m.alloca(0x20);
    for k in 0..8 {
        m.w32(frame + k * 4, 0);
    }
    let r = look_ahead(m, frame, frame + 8, p1, p2, p3, p4, p5, p6, p7, p8, p9);
    m.set_sp(sp);
    r
}

#[allow(clippy::too_many_arguments)]
fn look_ahead(m: &mut Mem, span: u32, st: u32, pat: u32, src: u32, src_pos: u32, p4: u32, prev_kind: u32, p6: u32, p7: u32, p8: u32, p9: u32) -> u32 {
    let start = m.i(p7);
    if start == -1 {
        return 0;
    }
    let mut kind = if start == -2 { 0 } else { f_1c011fa0(m, m.r8(pat.wrapping_add(start as u32)) as u32) };
    let mut pos: u32;
    if kind as u16 == 0 {
        pos = p4;
        let c = m.r8(pat.wrapping_add(pos).wrapping_sub(1));
        if c != b'>' && c != b']' {
            loop {
                let c = m.r8(pat.wrapping_add(pos));
                if c == b'>' || c == b']' {
                    break;
                }
                pos = pos.wrapping_add(1);
            }
        }
        kind = f_1c011fa0(m, m.r8(pat.wrapping_add(pos)) as u32);
        let mut node = p9;
        while kind as u16 == 0 {
            let c = m.r8(pat.wrapping_add(pos));
            if c == 0 {
                break;
            }
            let k = f_1c012100(m, c as u32);
            if k == 0 {
                let c = m.r8(pat.wrapping_add(pos));
                if (c == b'/' || c == b')') && node != 0 && c != 0 {
                    match m.s(node + 0xe) as u32 {
                        0 => {
                            m.w32(p7, 0xffff_ffff);
                            f_1c014d60(m, 0x1c06_4574, pat, pos);
                            return 0;
                        }
                        2 | 4 | 5 | 6 | 12 => {
                            let at = |m: &Mem, p: u32| m.r8(pat.wrapping_add(p));
                            pos = pos.wrapping_add(1);
                            let mut c = at(m, pos);
                            while c != b'/' {
                                if c == b'\'' || c == b'`' {
                                    let q = c;
                                    pos = pos.wrapping_add(1);
                                    c = at(m, pos);
                                    while c != q {
                                        if c == b'\\' {
                                            pos = pos.wrapping_add(1);
                                        }
                                        pos = pos.wrapping_add(1);
                                        c = at(m, pos);
                                    }
                                }
                                pos = pos.wrapping_add(1);
                                c = at(m, pos);
                            }
                        }
                        10 => {
                            pos = pos.wrapping_add(1);
                            if m.r32(pat.wrapping_add(pos)) == m.r32(FAIL_A) {
                                pos = pos.wrapping_add(5);
                            } else {
                                m.w16(st + 8, pos as i32);
                                f_1c0140a0(m, pat, st, 1, b'|' as u32);
                                pos = m.s(st + 8) as u32;
                            }
                            if m.r32(pat.wrapping_add(pos)) == m.r32(FAIL_B) {
                                pos = pos.wrapping_add(5);
                            } else {
                                m.w16(st + 8, pos as i32);
                                f_1c0140a0(m, pat, st, 1, b'/' as u32);
                                pos = m.s(st + 8) as u32;
                            }
                        }
                        _ => {}
                    }
                    node = m.r32(node + 0x14);
                }
            } else if k == 10 {
                f_1c014dc0(m, st, p9);
                m.w16(st, src_pos as i32);
                pos = pos.wrapping_add(2);
                m.w16(st + 8, pos as i32);
                let r = f_1c0129b0(m, pat, src, p8, st);
                return (r != 0) as u32;
            } else if k == 9 {
                m.w32(p7, 0xffff_ffff);
                return 0;
            } else {
                pos = pos.wrapping_add(1);
            }
            pos = pos.wrapping_add(1);
            kind = f_1c011fa0(m, m.r8(pat.wrapping_add(pos)) as u32);
        }
        m.w32(p7, pos);
    } else {
        pos = m.r32(p7);
    }

    if kind as u16 == 0x78 {
        m.w32(p7, 0xffff_ffff);
        return 0;
    }
    let cur = pat.wrapping_add(pos);
    if m.r8(cur) == 0 {
        m.w32(p7, 0xffff_ffff);
        return 0;
    }
    let positive = if m.r8(cur + 1) == b'~' { 0 } else { 1 };
    let prev_class = sx16(f_1c013eb0(m, prev_kind));
    let cur_class = sx16(f_1c013eb0(m, kind));
    let kind = kind & 0xffff;
    let src_at = src.wrapping_add(src_pos);

    let standard = match kind {
        0x400 | 0x20 | 0x100 | 0x40 | 0x80 | 0x200 | 0x2000 | 0x800 | 0x1000 | 0x4000 | 2 | 4 | 8 | 0x10 => true,
        1 => {
            if f_1c013e80(m, prev_class, p6, cur_class, positive) as u16 == 0 {
                return 0;
            }
            if m.r8(cur + 1) == b'[' || m.r8(cur + 2) == b'[' {
                m.w16(st, src_pos as i32);
                m.w16(st + 8, m.i(p7));
                m.w16(st + 0xa, 1);
                let r = f_1c0152b0(m, pat, src, p8, st, span, 0, 1);
                if m.s(st + 0xa) != 1 {
                    return 0;
                }
                return (r & 0xffff_0000) | ((r as i32 > 0) as u32);
            }
            true
        }
        0x26b9 => {
            let mut q = cur + 1;
            if m.r8(q) == b'\'' {
                return 0;
            }
            let mut n = 0u32;
            loop {
                if m.r8(q) == b'\\' {
                    q += 1;
                }
                if m.r8(src_at.wrapping_add(n)) != m.r8(q) {
                    return 0;
                }
                let next = m.r8(q + 1);
                n += 1;
                q += 1;
                if next == b'\'' {
                    break;
                }
            }
            return if (n as i32) < 1 { 0 } else { 1 };
        }
        0x26bd => {
            let mut q = cur + 1;
            if m.r8(q) == b'`' {
                return 0;
            }
            let mut n = 0u32;
            loop {
                if m.r8(q) == b'\\' {
                    q += 1;
                }
                let a = m.r8(CASE_FOLD + m.r8(src_at.wrapping_add(n)) as u32);
                let b = m.r8(CASE_FOLD + m.r8(q) as u32);
                if a != b {
                    return 0;
                }
                let next = m.r8(q + 1);
                n += 1;
                q += 1;
                if next == b'`' {
                    break;
                }
            }
            return if (n as i32) < 1 { 0 } else { 1 };
        }
        0x26bf => {
            let v = sx16(f_1c013700(m, cur, 2));
            let eq = (m.r8(src_at) as u32 == v) as u32;
            return (v & 0xffff_0000) | eq;
        }
        0x26c3 => {
            let d = m.r8(cur + 1) as i32 - 0x30;
            if !(0..=9).contains(&d) {
                return 0;
            }
            let slot = p8.wrapping_add(d as u32 * 30);
            let n = strlen(m, slot);
            if !mem_eq(m, src_at, slot, n) {
                return 0;
            }
            return (m.r8(slot) != 0) as u32;
        }
        _ => false,
    };
    if !standard {
        return 0;
    }
    if f_1c013e80(m, prev_class, p6, cur_class, positive) as u16 == 0 {
        return 0;
    }
    m.w16(st, src_pos as i32);
    m.w16(st + 8, m.i(p7));
    m.w16(st + 0xa, 1);
    let r = f_1c014de0(m, pat, kind, src, p8, st, 0, 1);
    if m.s(st + 0xa) != 1 {
        return 0;
    }
    (r & 0xffff_0000) | ((r as i32 > 0) as u32)
}

/// Element-compatibility test: COMPAT[prev][cur] & bit(p2 == 0, p4 == 0).
pub fn f_1c013e80(m: &mut Mem, p1: u32, p2: u32, p3: u32, p4: u32) -> u32 {
    let mut bit: u8 = if p2 == 0 { 4 } else { 1 };
    if p4 == 0 {
        bit <<= 1;
    }
    let row = p1.wrapping_mul(20);
    let v = sx8(m.r8(COMPAT.wrapping_add(row).wrapping_add(p3)) as u32);
    v & sx8(bit as u32)
}

/// Element kind code -> compatibility-table index (AX), -1 for unknown kinds.
pub fn f_1c013eb0(m: &mut Mem, p1: u32) -> u32 {
    let _ = m;
    let x = p1 & 0xffff;
    match x {
        1 => 0,
        2 => 1,
        4 => 2,
        8 => 3,
        0x10 => 4,
        0x20 => 5,
        0x40 => 6,
        0x80 => 7,
        0x100 => 8,
        0x200 => 9,
        0x400 => 0xa,
        0x800 => 0xb,
        0x1000 => 0xc,
        0x2000 => 0xd,
        0x4000 => 0xe,
        0x26b7 => 0xf,
        0x26b9 => 0x10,
        0x26bd => 0x11,
        0x26bf => 0x12,
        0x26c3 => 0x13,
        _ => {
            // EAX's high half is what the compare chain left there: negative residues give 0xffff.
            let neg = x == 0 || (0x21..0x80).contains(&x) || (0x26ba..0x26bf).contains(&x);
            if neg { 0xffff_ffff } else { 0xffff }
        }
    }
}

/// `<lo-hi>` repetition of a character class (`p2` = class mask) over the source text.
/// Returns the matched count, -1 (end of source, strict mode) or -2 (zero-length look-ahead).
pub fn f_1c014de0(m: &mut Mem, p1: u32, p2: u32, p3: u32, p4: u32, p5: u32, p6: u32, p7: u32) -> u32 {
    let sp = m.sp();
    let cells = m.alloca(8);
    m.w32(cells, p1);
    m.w32(cells + 4, 0xffff_fffe);
    let r = match_standard(m, cells, cells + 4, p1, p2, p3, p4, p5, p6, p7);
    m.set_sp(sp);
    r
}

#[derive(Clone, Copy)]
enum Std {
    Top,
    Element,
    SetDone,
    Skip,
    Count,
    Next,
    Close,
    Step,
    Check,
    End,
}

#[allow(clippy::too_many_arguments)]
fn match_standard(m: &mut Mem, len: u32, la: u32, pat: u32, mask: u32, src: u32, p4: u32, st: u32, p6: u32, once: u32) -> u32 {
    let mut verbose = p6;
    let mut lo: i32 = -1;
    let mut hi: i32 = 0;
    let mut done = 0;
    let mut idx: i32 = 0;
    let mut count: i32 = 0;
    let mut positive = 1u32;
    let mut matched = 0;
    let mut fail = 0;
    let mut state: i32 = -1;
    if st == 0 {
        return 0;
    }
    if pat == 0 || src == 0 {
        m.w16(st + 0xa, 4);
        return 0;
    }
    let base_off = (m.s(st + 2) + m.s(st)) as u32;
    let at = |m: &Mem, p: u32| m.r8(pat.wrapping_add(p));
    let mut pos = (m.s(st + 8) as u32).wrapping_add(1);
    if at(m, pos) == b'x' {
        verbose = 0;
        pos = pos.wrapping_add(1);
    }
    if at(m, pos) == b'~' {
        positive = 0;
        pos = pos.wrapping_add(1);
    }
    if at(m, pos) != b'<' {
        if verbose != 0 {
            f_1c014d60(m, 0x1c06_4a4c, pat, pos);
        }
        m.w16(st + 0xa, 4);
        return 0;
    }
    pos = pos.wrapping_add(1);

    macro_rules! syntax_error {
        ($msg:expr) => {{
            if verbose != 0 {
                f_1c014d60(m, $msg, pat, pos);
            }
            m.w16(st + 0xa, 4);
            return 0;
        }};
    }

    let mut l = if at(m, pos) == b'>' { Std::End } else { Std::Top };
    loop {
        match l {
            Std::Top => {
                l = if done != 0 {
                    Std::End
                } else if at(m, pos) == b'>' {
                    Std::Step
                } else {
                    Std::Element
                };
            }
            Std::Element => {
                if fail != 0 || matched != 0 || done != 0 {
                    l = Std::Close;
                    continue;
                }
                match at(m, pos) {
                    b'*' => {
                        if lo == -1 {
                            lo = 0;
                        }
                        pos = pos.wrapping_add(1);
                        hi = UNBOUNDED;
                        if at(m, pos) != b'>' {
                            syntax_error!(0x1c06_4a84);
                        }
                        l = Std::SetDone;
                    }
                    b'+' => {
                        if lo < 1 {
                            lo = 1;
                        }
                        pos = pos.wrapping_add(1);
                        hi = UNBOUNDED;
                        if at(m, pos) != b'>' {
                            syntax_error!(0x1c06_4ab8);
                        }
                        l = Std::SetDone;
                    }
                    _ => {
                        lo = sx16(f_1c013790(m, pat.wrapping_add(pos), len)) as i32;
                        pos = pos.wrapping_add(m.s(len) as u32);
                        match at(m, pos) {
                            b',' => {
                                hi = lo;
                                l = Std::Skip;
                            }
                            b'-' => {
                                pos = pos.wrapping_add(1);
                                let c = at(m, pos);
                                let term;
                                if c == b'*' || c == b'+' {
                                    pos = pos.wrapping_add(1);
                                    term = at(m, pos);
                                    hi = UNBOUNDED;
                                    if term != b'>' {
                                        syntax_error!(0x1c06_4ae4);
                                    }
                                } else {
                                    hi = sx16(f_1c013790(m, pat.wrapping_add(pos), len)) as i32;
                                    pos = pos.wrapping_add(m.s(len) as u32);
                                    term = at(m, pos);
                                    if term != b'>' && term != b',' {
                                        syntax_error!(0x1c06_4b20);
                                    }
                                }
                                if term == b'>' {
                                    done = 2;
                                }
                                pos = pos.wrapping_add(1);
                                if hi < lo {
                                    hi = lo;
                                }
                                l = Std::Count;
                            }
                            b'>' => {
                                hi = lo;
                                l = Std::SetDone;
                            }
                            _ => syntax_error!(0x1c06_4b58),
                        }
                    }
                }
            }
            Std::SetDone => {
                done = 2;
                l = Std::Skip;
            }
            Std::Skip => {
                pos = pos.wrapping_add(1);
                l = Std::Count;
            }
            Std::Count => {
                l = Std::Next;
                if m.s(st + 0xc) == -1 {
                    continue;
                }
                if lo == 0 {
                    state = -2;
                    if count == 0 && verbose != 0 {
                        let r = f_1c013830(m, pat, src, base_off, pos, mask, positive, la, p4, st);
                        if r as u16 == 1 {
                            matched = 1;
                        }
                    }
                }
                idx = count;
                if count >= hi {
                    continue;
                }
                loop {
                    if matched == 1 {
                        break;
                    }
                    let c = m.r8(src.wrapping_add(base_off).wrapping_add(idx as u32));
                    let bits = char_bits(m, c) & mask;
                    if (bits == 0 && positive == 1) || (bits != 0 && positive == 0) || c == 0 {
                        fail = 1;
                        break;
                    }
                    let next = idx + 1;
                    if next >= lo {
                        count = next;
                        state = next;
                        if once == 1 {
                            fail = 1;
                            break;
                        }
                        if verbose != 0 && (done == 0 || (done == 2 && next < hi)) {
                            let at_src = base_off.wrapping_add(idx as u32).wrapping_add(1);
                            let r = f_1c013830(m, pat, src, at_src, pos, mask, positive, la, p4, st);
                            if r as u16 == 1 {
                                matched = 1;
                                break;
                            }
                        }
                    }
                    idx = next;
                    if next >= hi {
                        break;
                    }
                }
            }
            Std::Next => {
                l = if at(m, pos) != b'>' { Std::Element } else { Std::Close };
            }
            Std::Close => {
                l = if done == 2 { Std::Check } else { Std::Step };
            }
            Std::Step => {
                pos = pos.wrapping_add(1);
                l = Std::Check;
            }
            Std::Check => {
                l = if at(m, pos) != b'>' { Std::Top } else { Std::End };
            }
            Std::End => break,
        }
    }

    if at(m, pos) == b'>' {
        pos = pos.wrapping_add(1);
    }
    if state == -1 {
        let mode = m.s(st + 0xc);
        if mode == 1 || mode == -1 {
            m.w16(st + 0xc, -1);
        }
        m.w16(st + 0xa, 0);
    } else if state == -2 {
        m.w16(st + 8, pos as i32);
        return if count == 0 { 0xffff_fffe } else { count as u32 };
    }
    if m.r8(src.wrapping_add(base_off).wrapping_add(idx as u32)) == 0 && count == 0 {
        let mode = m.s(st + 0xc);
        if mode != 1 && mode != -1 {
            m.w16(st + 8, pos as i32);
            return 0xffff_ffff;
        }
        m.w16(st + 0xc, -1);
        m.w16(st + 0xa, 0);
    }
    m.w16(st + 8, pos as i32);
    count as u32
}

/// `[lo-hi]` digit-string matcher: the source must be a digit run whose value lies in range.
/// Returns the matched length, or -1 at end of source in strict mode.
pub fn f_1c0152b0(m: &mut Mem, p1: u32, p2: u32, p3: u32, p4: u32, p5: u32, p6: u32, p7: u32) -> u32 {
    let sp = m.sp();
    let cells = m.alloca(8);
    m.w32(cells, 0);
    m.w32(cells + 4, 0xffff_fffe);
    let r = match_digits(m, cells, cells + 4, p1, p2, p3, p4, p5, p6, p7);
    m.set_sp(sp);
    r
}

#[derive(Clone, Copy)]
enum Dig {
    Top,
    Element,
    Close,
    Step,
    Check,
    End,
}

#[allow(clippy::too_many_arguments)]
fn match_digits(m: &mut Mem, len: u32, la: u32, pat: u32, src: u32, p3: u32, st: u32, span: u32, p6: u32, once: u32) -> u32 {
    let mut verbose = p6;
    let mut done = 0;
    let mut count: i32 = 0;
    let mut fail = 0;
    let mut matched = 0;
    let mut last: i32 = 0;
    let mut state: i32 = -1;
    let mut saved: i32 = -1;
    let mut idx: i32 = 0;
    if st == 0 {
        return 0;
    }
    if pat == 0 || src == 0 || span == 0 {
        m.w16(st + 0xa, 4);
        return 0;
    }
    let base_off = (m.s(st + 2) + m.s(st)) as u32;
    let at = |m: &Mem, p: u32| m.r8(pat.wrapping_add(p));
    let mut pos = (m.s(st + 8) as u32).wrapping_add(1);
    if at(m, pos) == b'x' {
        verbose = 0;
        pos = pos.wrapping_add(1);
    }
    if at(m, pos) != b'[' {
        if verbose != 0 {
            f_1c014d60(m, 0x1c06_4b80, pat, pos);
        }
        m.w16(st + 0xa, 4);
        return 0;
    }
    pos = pos.wrapping_add(1);

    macro_rules! syntax_error {
        ($msg:expr) => {{
            if verbose != 0 {
                f_1c014d60(m, $msg, pat, pos);
            }
            m.w16(st + 0xa, 4);
            return 0;
        }};
    }

    let mut l = if at(m, pos) == b']' { Dig::End } else { Dig::Top };
    loop {
        match l {
            Dig::Top => {
                l = if done != 0 {
                    Dig::End
                } else if at(m, pos) == b']' {
                    Dig::Step
                } else {
                    Dig::Element
                };
            }
            Dig::Element => {
                if fail != 0 || matched != 0 || done != 0 {
                    l = Dig::Close;
                    continue;
                }
                let lo = sx16(f_1c013790(m, pat.wrapping_add(pos), len)) as i32;
                pos = pos.wrapping_add(m.s(len) as u32);
                let mut hi;
                match at(m, pos) {
                    b',' => {
                        hi = lo;
                        pos = pos.wrapping_add(1);
                    }
                    b'-' => {
                        pos = pos.wrapping_add(1);
                        let c = at(m, pos);
                        let term;
                        if c == b'*' || c == b'+' {
                            pos = pos.wrapping_add(1);
                            hi = UNBOUNDED;
                            term = at(m, pos);
                            if term != b']' {
                                syntax_error!(0x1c06_4bb4);
                            }
                        } else {
                            hi = sx16(f_1c013790(m, pat.wrapping_add(pos), len)) as i32;
                            pos = pos.wrapping_add(m.s(len) as u32);
                            term = at(m, pos);
                            if term != b']' && term != b',' {
                                syntax_error!(0x1c06_4bf0);
                            }
                        }
                        if term == b']' {
                            done = 2;
                        }
                        pos = pos.wrapping_add(1);
                        if hi < lo {
                            hi = lo;
                        }
                    }
                    b']' => {
                        hi = lo;
                        done = 2;
                        pos = pos.wrapping_add(1);
                    }
                    _ => syntax_error!(0x1c06_4c24),
                }

                if m.s(st + 0xc) != -1 {
                    let flag = m.s(span + 6);
                    if flag == 0 {
                        m.w16(span + 6, 1);
                        m.w16(span, lo);
                    } else {
                        if flag == 2 {
                            m.w16(span + 6, -1);
                        }
                        if m.s(span + 6) == 1 {
                            let acc = m.s(span) + (lo - m.s(span + 2));
                            m.w16(span, acc);
                        }
                    }
                    m.w16(span + 4, lo);
                    m.w16(span + 2, hi);
                    f_1c013800(m, lo as u32);
                    let max_len = sx16(f_1c013800(m, hi as u32)) as i32;
                    let digits = base_off.wrapping_add(src);
                    idx = 0;
                    if m.r8(CHAR_BITS + m.r8(digits) as u32 * 2) & 1 != 0 {
                        loop {
                            let v = sx16(f_1c013760(m, digits, (idx + 1) as u32)) as i32;
                            last = v;
                            if v > m.s(span + 2) || idx >= max_len {
                                break;
                            }
                            if v >= m.s(span + 4) {
                                saved = m.s(span);
                                count = idx + 1;
                                state = v;
                                if once == 1 {
                                    fail = 1;
                                    break;
                                }
                                if verbose != 0 && (done == 0 || (done == 2 && count < max_len)) {
                                    let at_src = base_off.wrapping_add(idx as u32).wrapping_add(1);
                                    let r = f_1c013830(m, pat, src, at_src, pos, 1, 1, la, p3, st);
                                    if r as u16 == 1 {
                                        matched = 1;
                                        break;
                                    }
                                }
                            }
                            idx += 1;
                            let c = m.r8(digits.wrapping_add(idx as u32));
                            if m.r8(CHAR_BITS + c as u32 * 2) & 1 == 0 {
                                break;
                            }
                        }
                    }
                    if last > m.s(span + 2) && done == 2 {
                        fail = 1;
                    }
                }
                l = if at(m, pos) != b']' { Dig::Element } else { Dig::Close };
            }
            Dig::Close => {
                l = if done == 2 { Dig::Check } else { Dig::Step };
            }
            Dig::Step => {
                pos = pos.wrapping_add(1);
                l = Dig::Check;
            }
            Dig::Check => {
                l = if at(m, pos) != b']' { Dig::Top } else { Dig::End };
            }
            Dig::End => break,
        }
    }

    if at(m, pos) == b']' {
        pos = pos.wrapping_add(1);
    }
    if state == -1 {
        if m.s(st + 0xc) == 1 {
            m.w16(st + 0xc, -1);
        }
        m.w16(st + 0xa, 0);
    } else if m.s(span) != saved {
        m.w16(span, saved);
    }
    if m.r8(src.wrapping_add(base_off).wrapping_add(idx as u32)) == 0 && count == 0 {
        let mode = m.s(st + 0xc);
        if mode != 1 && mode != -1 {
            m.w16(st + 8, pos as i32);
            return 0xffff_ffff;
        }
        m.w16(st + 0xc, -1);
        m.w16(st + 0xa, 0);
    }
    m.w16(st + 8, pos as i32);
    count as u32
}

#[derive(Clone, Copy)]
enum Elem {
    Standard,
    Digits,
    Special,
    Quoted,
    Folded,
    Hex,
    Shuffle,
    Bad,
}

/// Match one rule element of kind `p2` at the rule position in `p5` against the source text.
/// Returns the number of source characters consumed, or -1 at end of source in strict mode.
pub fn f_1c0130f0(m: &mut Mem, p1: u32, p2: u32, p3: u32, p4: u32, p5: u32, p6: u32, p7: u32, p8: u32) -> u32 {
    let pat = p1;
    let src = p3;
    let st = p5;
    let base_off = (m.s(st + 2) + m.s(st)) as u32;
    let mut pos = m.s(st + 8) as u32;
    let mut res: i32 = 0;
    let at = |m: &Mem, p: u32| m.r8(pat.wrapping_add(p));

    let k = p2 as i32;
    let mut elem = match k {
        0x20 | 0x40 | 0x80 | 0x100 | 0x200 | 0x400 | 0x800 | 0x1000 | 0x2000 | 0x4000 => Elem::Standard,
        2 | 4 | 8 | 0x10 => Elem::Standard,
        1 => Elem::Digits,
        0x26b7 => Elem::Special,
        0x26b9 => Elem::Quoted,
        0x26bd => Elem::Folded,
        0x26bf => Elem::Hex,
        0x26c3 => Elem::Shuffle,
        _ => Elem::Bad,
    };
    if matches!(elem, Elem::Digits) && at(m, pos.wrapping_add(1)) != b'[' && at(m, pos.wrapping_add(2)) != b'[' {
        elem = Elem::Standard;
    }

    match elem {
        Elem::Standard => {
            res = f_1c014de0(m, p1, p2, p3, p4, p5, p7, p8) as i32;
            pos = m.s(st + 8) as u32;
        }
        Elem::Digits => {
            res = f_1c0152b0(m, p1, p3, p4, p5, p6, p7, p8) as i32;
            pos = m.s(st + 8) as u32;
        }
        Elem::Special => {
            res = f_1c0156e0(m, p1, p3, p4, p5, p6, p7, p8) as i32;
            pos = m.s(st + 8) as u32;
        }
        Elem::Quoted | Elem::Folded => {
            let quote = if let Elem::Quoted = elem { b'\'' } else { b'`' };
            pos = pos.wrapping_add(1);
            let mut c = at(m, pos);
            while c != quote {
                if c == b'\\' {
                    pos = pos.wrapping_add(1);
                }
                let mode = m.s(st + 0xc);
                if mode != -1 {
                    let pc = at(m, pos);
                    let sc = m.r8(src.wrapping_add(base_off).wrapping_add(res as u32));
                    let same = if let Elem::Quoted = elem {
                        pc == sc
                    } else {
                        m.r8(CASE_FOLD + pc as u32) == m.r8(CASE_FOLD + sc as u32)
                    };
                    if !same {
                        m.w16(st + 0xa, 0);
                        if mode == 1 {
                            m.w16(st + 0xc, -1);
                        }
                        res = -1;
                    }
                    res += 1;
                }
                pos = pos.wrapping_add(1);
                c = at(m, pos);
            }
            pos = pos.wrapping_add(1);
        }
        Elem::Hex => {
            if m.s(st + 0xc) != -1 {
                if at(m, pos.wrapping_add(1)) != b'x' {
                    f_1c014d60(m, 0x1c06_441c, pat, pos);
                    m.w16(st + 0xa, 4);
                    return 0;
                }
                let v = sx16(f_1c013700(m, pat.wrapping_add(pos), 2)) as i32;
                if v < 0 {
                    f_1c014d60(m, 0x1c06_4448, pat, pos);
                    m.w16(st + 0xa, 4);
                    return 0;
                }
                if m.r8(src.wrapping_add(base_off)) as i32 != v {
                    m.w16(st + 0xa, 0);
                    if m.s(st + 0xc) == 1 {
                        m.w16(st + 0xc, -1);
                    }
                    return 0;
                }
                res = 1;
            }
            pos = pos.wrapping_add(4);
        }
        Elem::Shuffle => {
            let mode = m.s(st + 0xc);
            pos = pos.wrapping_add(1);
            if mode != -1 {
                let d = at(m, pos) as i32 - 0x30;
                if !(0..=10).contains(&d) {
                    f_1c014d60(m, 0x1c06_eff8, pat, pos);
                    pos = pos.wrapping_add(1);
                    m.w16(st + 8, pos as i32);
                    m.w16(st + 0xa, 4);
                    return 0;
                }
                let slot = p4.wrapping_add(d as u32 * 30);
                let n = strlen(m, slot);
                res = n as i32;
                if !mem_eq(m, src.wrapping_add(base_off), slot, n) {
                    res = 0;
                    m.w16(st + 0xa, 0);
                    if m.s(st + 0xc) == 1 {
                        m.w16(st + 0xc, -1);
                    }
                }
            }
            pos = pos.wrapping_add(1);
        }
        Elem::Bad => {
            f_1c014d60(m, 0x1c06_449c, pat, pos);
        }
    }

    m.w16(st + 8, pos as i32);
    if res == -2 {
        return 0;
    }
    if m.r8(src.wrapping_add(base_off).wrapping_add(res as u32)) == 0 && res <= 0 {
        let mode = m.s(st + 0xc);
        if mode != 1 && mode != -1 {
            return 0xffff_ffff;
        }
        m.w16(st + 0xc, -1);
        m.w16(st + 0xa, 0);
        res = 0;
    }
    res as u32
}
