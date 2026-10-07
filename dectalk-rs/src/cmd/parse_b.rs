//! DTparser rule interpreter: the recursive rule matcher (FUN_1c0117b0), the per-state action
//! dispatcher and its actions (build string, insert, append, spread, save, lookup), and the small
//! numeric/character-class helpers they share.
//!
//! Layouts discovered from the code:
//!
//! Match state `st` (0x12 bytes of short fields; FUN_1c0117b0 builds a child copy on its stack):
//!   +0x00 s16  input base (index into the input text)
//!   +0x02 s16  input consumed (added to +0x00 for the child's base)
//!   +0x04 s16  output base (index into the output text buffer)
//!   +0x06 s16  output length
//!   +0x08 s16  rule position (index into the rule text); written back on exit
//!   +0x0a s16  status: 0 = no match, 1 = match, 2 = optional miss, 3 = overrun, 4 = rule error
//!   +0x0c s16  mode: 1 = optional (a miss turns it into -1), -1 = skip (advance rule only)
//!   +0x0e s16  state kind (FUN_1c0117b0 arg 2)
//!   +0x10 s16  saved number (`s` action result, restored by the parent)
//!
//! Attribute tables (input/output): 6-byte entries; entry 499 is the blank entry.
//! Condition `aux` (8 bytes, 4 shorts): +0x00 value, +0x06 kind (1 = relative, 2 = absolute).
//! String slots: `slots + i*30`, i = 0..10, NUL-terminated (`$0`..`$9` in build strings).
//! Lookup categories: 0x1c066364 {s16 lo, s16 hi, s16}[]; 0x1c0663d0 s16 offsets into the pool
//! at 0x1c0665dc, each entry = key NUL value NUL. Named rules: 0x1c064ed0 + s16[0x1c064e10 + 2*n].
//! Char class: 0x1c063b90 u16[256].

use super::*;

const CHAR_BITS: u32 = 0x1c06_3b90;
const RULE_POOL: u32 = 0x1c06_4ed0;
const RULE_INDEX: u32 = 0x1c06_4e10;
const LOOKUP_CATS: u32 = 0x1c06_6364;
const LOOKUP_INDEX: u32 = 0x1c06_63d0;
const LOOKUP_POOL: u32 = 0x1c06_65dc;
const FAIL_1: u32 = 0x1c06_42fc;
const FAIL_2: u32 = 0x1c06_4304;
const FAIL_3: u32 = 0x1c06_430c;
const FAIL_4: u32 = 0x1c06_4314;
const BLANK_ENTRY: u32 = 0x1f3;

const ST_IN_BASE: u32 = 0x0;
const ST_IN_LEN: u32 = 0x2;
const ST_OUT_BASE: u32 = 0x4;
const ST_OUT_LEN: u32 = 0x6;
const ST_POS: u32 = 0x8;
const ST_STATUS: u32 = 0xa;
const ST_MODE: u32 = 0xc;
const ST_KIND: u32 = 0xe;
const ST_SAVED: u32 = 0x10;

fn s16(v: u32) -> i32 {
    v as u16 as i16 as i32
}

fn strlen(m: &Mem, s: u32) -> u32 {
    let mut n = 0u32;
    while m.r8(s.wrapping_add(n)) != 0 {
        n = n.wrapping_add(1);
    }
    n
}

/// `rep movs`: forward byte copy.
fn movs(m: &mut Mem, dst: u32, src: u32, n: u32) {
    for k in 0..n {
        let b = m.r8(src.wrapping_add(k));
        m.w8(dst.wrapping_add(k), b);
    }
}

fn stos0(m: &mut Mem, dst: u32, n: u32) {
    for k in 0..n {
        m.w8(dst.wrapping_add(k), 0);
    }
}

/// Inlined strcpy (`repne scasb` + `rep movs`, NUL included).
fn strcpy(m: &mut Mem, dst: u32, src: u32) {
    let n = strlen(m, src) + 1;
    movs(m, dst, src, n);
}

/// MSVCRT `_stricmp` in the C locale.
fn stricmp(m: &Mem, a: u32, b: u32) -> i32 {
    let lower = |c: u8| if c.is_ascii_uppercase() { c + 0x20 } else { c } as i32;
    let mut k = 0u32;
    loop {
        let f = lower(m.r8(a.wrapping_add(k)));
        let l = lower(m.r8(b.wrapping_add(k)));
        k += 1;
        if f == 0 || f != l {
            return f - l;
        }
    }
}

/// MSVCRT `atoi`.
fn atoi(m: &Mem, s: u32) -> i32 {
    let mut p = s;
    while matches!(m.r8(p), b' ' | 0x09..=0x0d) {
        p += 1;
    }
    let neg = m.r8(p) == b'-';
    if matches!(m.r8(p), b'-' | b'+') {
        p += 1;
    }
    let mut v: i32 = 0;
    while m.r8(p).is_ascii_digit() {
        v = v.wrapping_mul(10).wrapping_add((m.r8(p) - b'0') as i32);
        p += 1;
    }
    if neg { v.wrapping_neg() } else { v }
}

/// Clear the output-attribute span a failed state had written (`rep stos`).
fn clear_attrs(m: &mut Mem, attrs: u32, st: u32) {
    let base = s16(m.r16(st + ST_OUT_BASE) as u32);
    let n = s16(m.r16(st + ST_OUT_LEN) as u32);
    let dst = attrs.wrapping_add((base * 6) as u32);
    stos0(m, dst, (n * 6) as u32);
}

/// Recursive rule-state matcher. p1 rule text, p2 state kind (0 = top level), p3 input text,
/// p4 output text, p5 input attrs, p6 output attrs, p7 string slots, p8 match state,
/// p9 state terminator, p10 lookup exact flag.
pub fn f_1c0117b0(m: &mut Mem, p1: u32, p2: u32, p3: u32, p4: u32, p5: u32, p6: u32, p7: u32, p8: u32, p9: u32, p10: u32) -> u32 {
    if p8 == 0 {
        return 0;
    }
    if p1 == 0 || p3 == 0 || p4 == 0 || p7 == 0 {
        m.w16(p8 + ST_STATUS, 4);
        return 0;
    }
    let sp = m.sp();
    let frame = m.alloca(0x28);
    let r = match_state(m, frame, [p1, p2, p3, p4, p5, p6, p7, p8, p9, p10]);
    m.set_sp(sp);
    r
}

enum Flow {
    Scan,
    Tail,
    Finish,
    Overrun,
    Error(u32, u32),
}

fn match_state(m: &mut Mem, frame: u32, p: [u32; 10]) -> u32 {
    let [rule, kind0, text, out, in_attrs, out_attrs, slots, rec, term0, exact] = p;
    let cnt = frame;
    let aux = frame + 8;
    let st = frame + 0x10;
    m.w32(cnt, 0);
    m.w32(frame + 4, 0);
    for k in 0..4 {
        m.w16(aux + k * 2, 0);
    }
    // The child state links back to its parent (walked by the look-ahead in 1c013830).
    m.w32(st + 0x14, rec);
    let text_len = strlen(m, text);
    let (kind, mut term) = if kind0 == 0 { (1, 0) } else { (kind0, term0) };
    let mut number: u32 = 0;

    m.w16(st + ST_KIND, kind as i32);
    let mode = if kind == 7 { 1 } else { m.s(rec + ST_MODE) };
    m.w16(st + ST_MODE, mode);
    m.w16(st + ST_IN_BASE, m.s(rec + ST_IN_LEN) + m.s(rec + ST_IN_BASE));
    m.w16(st + ST_IN_LEN, 0);
    m.w16(st + ST_OUT_BASE, m.s(rec + ST_OUT_BASE) + m.s(rec + ST_OUT_LEN));
    m.w16(st + ST_OUT_LEN, 0);
    let mut dx = m.r16(rec + ST_POS);
    let mut si: i16 = 1;
    m.w16(st + ST_STATUS, 1);
    m.w16(st + ST_POS, dx as i32);
    m.w16(st + ST_SAVED, m.s(rec + ST_SAVED));

    let at = |m: &Mem, i: i16| m.r8(rule.wrapping_add(i as i32 as u32));

    let mut flow = if kind == 8 {
        // A save state `($n,...)` always ends at the closing parenthesis.
        term = 0x29;
        if at(m, dx) != b'$' {
            Flow::Error(0x1c06_4168, dx as i32 as u32)
        } else {
            dx = dx.wrapping_add(1);
            m.w16(st + ST_POS, dx as i32);
            let v = (at(m, dx) as i32).wrapping_sub(0x30);
            number = v as u32;
            if !(0..=9).contains(&v) {
                Flow::Error(0x1c06_4194, dx as i32 as u32)
            } else {
                dx = dx.wrapping_add(1);
                m.w16(st + ST_POS, dx as i32);
                if at(m, dx) != b',' {
                    Flow::Error(0x1c06_41c4, dx as i32 as u32)
                } else {
                    dx = dx.wrapping_add(1);
                    m.w16(st + ST_POS, dx as i32);
                    Flow::Scan
                }
            }
        }
    } else if kind == 10 {
        let ax = f_1c013790(m, rule.wrapping_add(dx as i32 as u32), cnt);
        dx = m.r16(st + ST_POS).wrapping_add(m.r16(cnt));
        number = sx16(ax);
        m.w16(st + ST_POS, dx as i32);
        if at(m, dx) != b',' {
            Flow::Error(0x1c06_41fc, dx as i32 as u32)
        } else {
            si = m.r16(st + ST_STATUS);
            dx = dx.wrapping_add(1);
            m.w16(st + ST_POS, dx as i32);
            Flow::Scan
        }
    } else if kind == 9 {
        if at(m, dx) != b'R' {
            Flow::Error(0x1c06_423c, dx as i32 as u32)
        } else {
            let mut esi = (m.r16(st + ST_POS) as u16 as u32 | (m.r16(st + ST_STATUS) as u16 as u32) << 16).wrapping_add(1);
            let ax = f_1c013790(m, rule.wrapping_add(sx16(esi)), cnt);
            esi = esi.wrapping_add(m.r32(cnt));
            let sub = RULE_POOL.wrapping_add(sx16(m.r16(RULE_INDEX.wrapping_add(sx16(ax).wrapping_mul(2))) as u32));
            m.w16(st + ST_POS, 0);
            f_1c0117b0(m, sub, 0, text, out, in_attrs, out_attrs, slots, st, 0x2f, 0);
            dx = esi as i16;
            let used = m.s(st + ST_IN_LEN) + m.s(st + ST_IN_BASE);
            let mut ax = m.r16(st + ST_MODE);
            m.w16(st + ST_POS, dx as i32);
            if used > text_len as i32 {
                if ax == 1 {
                    ax = -1;
                    m.w16(st + ST_MODE, -1);
                    si = m.r16(st + ST_STATUS);
                } else {
                    si = 3;
                    m.w16(st + ST_STATUS, 3);
                }
            } else {
                si = m.r16(st + ST_STATUS);
            }
            if (si == 0 && ax == 1) || ax == -1 {
                m.w16(st + ST_MODE, -1);
                m.w16(st + ST_STATUS, 2);
                Flow::Finish
            } else {
                Flow::Tail
            }
        }
    } else {
        Flow::Scan
    };

    if let Flow::Scan = flow {
        flow = loop {
            let c = at(m, dx);
            if c as u16 == term as u16 || si == 4 || si == 3 {
                break Flow::Tail;
            }
            if si == 0 {
                break Flow::Tail;
            }
            let mut cls = f_1c011fa0(m, c as u32);
            let mut ax: i16;
            if cls != 0 {
                if cls == 0x78 {
                    let pos = m.r16(st + ST_POS).wrapping_add(1);
                    m.w16(st + ST_POS, pos as i32);
                    cls = f_1c011fa0(m, at(m, pos) as u32);
                }
                let mut n = f_1c0130f0(m, rule, cls, text, slots, st, aux, 1, 0);
                if n == 0xffff_ffff {
                    if m.r16(st + ST_MODE) != 1 {
                        break Flow::Overrun;
                    }
                    m.w16(st + ST_MODE, n as i32);
                    n = 0;
                }
                f_1c013690(m, text, in_attrs, out, out_attrs, n, st);
                m.w16(st + ST_OUT_LEN, m.s(st + ST_OUT_LEN).wrapping_add(n as i32));
                m.w16(st + ST_IN_LEN, m.s(st + ST_IN_LEN).wrapping_add(n as i32));
                si = m.r16(st + ST_STATUS);
                ax = m.r16(st + ST_MODE);
            } else {
                let pos = m.r16(st + ST_POS);
                let action = f_1c012100(m, at(m, pos) as u32);
                if action == 0 {
                    break Flow::Error(0x1c06_4280, pos as i32 as u32);
                }
                let mut cx = pos.wrapping_add(1);
                m.w16(st + ST_POS, cx as i32);
                if action != 8 {
                    if at(m, cx) != b'/' {
                        break Flow::Error(0x1c06_4250, cx as i32 as u32);
                    }
                    cx = cx.wrapping_add(1);
                    m.w16(st + ST_POS, cx as i32);
                }
                f_1c0117b0(m, rule, action, text, out, in_attrs, out_attrs, slots, st, 0x2f, 0);
                si = m.r16(st + ST_STATUS);
                if si == 3 {
                    if m.r16(st + ST_MODE) != 1 {
                        break Flow::Overrun;
                    }
                    ax = -1;
                    si = 0;
                    m.w16(st + ST_MODE, -1);
                    m.w16(st + ST_STATUS, 0);
                } else {
                    ax = m.r16(st + ST_MODE);
                }
            }
            let used = m.s(st + ST_IN_LEN) + m.s(st + ST_IN_BASE);
            if used > text_len as i16 as i32 {
                if ax == 1 {
                    ax = -1;
                    m.w16(st + ST_MODE, -1);
                } else {
                    si = 3;
                    m.w16(st + ST_STATUS, 3);
                }
            }
            if (si == 0 && ax == 1) || ax == -1 {
                si = 2;
                m.w16(st + ST_MODE, -1);
                m.w16(st + ST_STATUS, 2);
            }
            dx = m.r16(st + ST_POS);
        };
    }

    if let Flow::Tail = flow {
        if si == 0 {
            m.w16(rec + ST_STATUS, 0);
            clear_attrs(m, out_attrs, st);
            return 0;
        }
        flow = if si == 3 {
            clear_attrs(m, out_attrs, st);
            Flow::Overrun
        } else {
            Flow::Finish
        };
    }

    match flow {
        Flow::Overrun => {
            m.w16(rec + ST_STATUS, 3);
            0
        }
        Flow::Error(fmt, pos) => {
            f_1c014d60(m, fmt, rule, pos);
            m.w16(rec + ST_STATUS, 4);
            0
        }
        _ => {
            let c = at(m, dx);
            if c as u16 != term as u16 {
                f_1c014d60(m, 0x1c06_42b8, rule, dx as i32 as u32);
                m.w16(rec + ST_STATUS, 4);
                return 0;
            }
            if term as u16 != 0 {
                dx = dx.wrapping_add(1);
                m.w16(st + ST_POS, dx as i32);
            }
            f_1c011e00(m, rule, kind, text, out, in_attrs, out_attrs, slots, st, aux, number, exact);
            m.w16(rec + ST_SAVED, m.s(st + ST_SAVED));
            if m.r16(st + ST_STATUS) == 0 {
                clear_attrs(m, out_attrs, st);
                if m.r16(st + ST_MODE) != 1 {
                    m.w16(rec + ST_STATUS, 0);
                    return 0;
                }
                m.w16(st + ST_MODE, -1);
            }
            m.w16(rec + ST_POS, m.s(st + ST_POS));
            if m.r16(st + ST_MODE) == -1 {
                clear_attrs(m, out_attrs, st);
                m.w16(rec + ST_STATUS, 2);
            } else {
                m.w16(rec + ST_IN_LEN, m.s(rec + ST_IN_LEN).wrapping_add(m.s(st + ST_IN_LEN)));
                m.w16(rec + ST_OUT_LEN, m.s(rec + ST_OUT_LEN).wrapping_add(m.s(st + ST_OUT_LEN)));
                m.w16(rec + ST_STATUS, 1);
            }
            if kind != 7 {
                m.w16(rec + ST_MODE, m.s(st + ST_MODE));
            }
            m.w16(rec + ST_POS, m.s(st + ST_POS));
            0
        }
    }
}

/// Run the action of a completed state (dispatch on the state kind).
pub fn f_1c011e00(m: &mut Mem, p1: u32, p2: u32, p3: u32, p4: u32, p5: u32, p6: u32, p7: u32, p8: u32, p9: u32, p10: u32, p11: u32) -> u32 {
    if p8 == 0 {
        return 0;
    }
    if p1 == 0 || p4 == 0 {
        m.w16(p8 + ST_STATUS, 4);
        return 0;
    }
    match p2 {
        2 => f_1c0122b0(m, p1, p4, p6, p7, p8, p9),
        3 => f_1c0121f0(m, p4, p6, p8),
        4 => f_1c012440(m, p1, p4, p6, p7, p8, p9),
        5 => f_1c012550(m, p1, p4, p7, p8, p9),
        6 => f_1c012600(m, p1, p4, p6, p7, p8, p9),
        8 => f_1c012720(m, p4, sx16(p10), p7, p8),
        10 => f_1c0127b0(m, p1, p3, p4, p5, p6, p7, p10, p8, p11),
        11 => f_1c012c30(m, p4, p8),
        12 => {
            let sp = m.sp();
            let len = m.alloca(4);
            let buf = m.alloca(12);
            m.w32(len, 0);
            f_1c012cc0(m, p1, buf, p4, p7, p8, p9, 0xc, len);
            let v = atoi(m, buf);
            m.w16(p8 + ST_POS, m.s(p8 + ST_POS).wrapping_add(1));
            m.w16(p8 + ST_SAVED, v);
            m.set_sp(sp);
            v as u32
        }
        _ => 0,
    }
}

/// Element class letter -> class bit / special code (0 = not an element).
pub fn f_1c011fa0(m: &mut Mem, p1: u32) -> u32 {
    let _ = m;
    match p1 as u8 {
        b'$' => 0x26c3,
        b'\'' => 0x26b9,
        b'0' => 0x26bf,
        b'A' => 8,
        b'B' => 0x400,
        b'C' => 0x10,
        b'D' => 1,
        b'E' => 0x800,
        b'H' => 0x1000,
        b'L' => 4,
        b'N' => 0x80,
        b'O' => 0x200,
        b'P' => 0x40,
        b'S' => 0x26b7,
        b'T' => 0x4000,
        b'U' => 2,
        b'V' => 0x100,
        b'W' => 0x20,
        b'Y' => 0x2000,
        b'`' => 0x26bd,
        b'x' => 0x78,
        _ => 0,
    }
}

/// Action letter -> state kind (0 = unknown).
pub fn f_1c012100(m: &mut Mem, p1: u32) -> u32 {
    let _ = m;
    match p1 as u8 {
        b'(' => 8,
        b'a' => 5,
        b'b' => 6,
        b'c' => 1,
        b'd' => 3,
        b'h' => 10,
        b'i' => 4,
        b'o' => 7,
        b'p' => 9,
        b'r' => 2,
        b's' => 12,
        b'w' => 11,
        _ => 0,
    }
}

/// `d` action: compact the output span, keeping only positions with attributes; each kept
/// position becomes 0x82.
pub fn f_1c0121f0(m: &mut Mem, p1: u32, p2: u32, p3: u32) -> u32 {
    let (buf, attrs, st) = (p1, p2, p3);
    if st == 0 {
        return 0;
    }
    if buf == 0 {
        m.w16(st + ST_STATUS, 4);
        return 0;
    }
    let old_len = m.r16(st + ST_OUT_LEN);
    if m.r16(st + ST_MODE) != -1 {
        let dst = buf.wrapping_add(m.s(st + ST_OUT_BASE) as u32);
        stos0(m, dst, old_len as i32 as u32);
        m.w16(st + ST_OUT_LEN, 0);
    }
    let mut i = m.r16(st + ST_OUT_BASE);
    let mut j = i as i32 as u32;
    if i >= old_len {
        return 0;
    }
    let mut p = buf.wrapping_add(i as i32 as u32);
    loop {
        if f_1c011780(m, attrs, i as u16 as u32) as u16 != 0 {
            f_1c011750(m, attrs, j, attrs, i as u16 as u32);
            f_1c011750(m, attrs, i as u16 as u32, attrs, BLANK_ENTRY);
            j = j.wrapping_add(1);
            m.w8(p, 0x82);
            m.w16(st + ST_OUT_LEN, m.s(st + ST_OUT_LEN).wrapping_add(1));
        }
        i = i.wrapping_add(1);
        p = p.wrapping_add(1);
        if i >= old_len {
            return 0;
        }
    }
}

/// `r` action: replace the output span with the build string, one word per attributed position.
pub fn f_1c0122b0(m: &mut Mem, p1: u32, p2: u32, p3: u32, p4: u32, p5: u32, p6: u32) -> u32 {
    let sp = m.sp();
    let frame = m.alloca(0x2c0);
    let cnt = frame;
    let buf = frame + 4;
    let tmp = frame + 0x68;
    stos0(m, tmp, 0x258);
    let r = replace_span(m, frame, [p1, p2, p3, p4, p5, p6], cnt, buf, tmp);
    m.set_sp(sp);
    r
}

fn replace_span(m: &mut Mem, _frame: u32, p: [u32; 6], cnt: u32, buf: u32, tmp: u32) -> u32 {
    let [rule, out, attrs, slots, st, aux] = p;
    if st == 0 {
        return 0;
    }
    if rule == 0 || out == 0 || slots == 0 {
        m.w16(st + ST_STATUS, 4);
        return 0;
    }
    f_1c012cc0(m, rule, buf, out, slots, st, aux, 2, cnt);
    if m.r16(st + ST_MODE) != -1 {
        let mut di = m.r16(st + ST_OUT_BASE);
        let mut si: i16 = 0;
        if (di as i32) < m.s(st + ST_OUT_LEN) + di as i32 {
            loop {
                if f_1c011780(m, attrs, di as u16 as u32) as u16 != 0 {
                    let n = m.i(cnt);
                    while (si as i32) < n && m.r8(buf.wrapping_add(si as i32 as u32)) != b' ' {
                        si = si.wrapping_add(1);
                    }
                    let k = si as i32;
                    if k < n {
                        f_1c011750(m, tmp, si as u16 as u32, attrs, di as u16 as u32);
                        si = si.wrapping_add(1);
                    } else {
                        m.w8(buf.wrapping_add(k as u32), 0x82);
                        m.w8(buf.wrapping_add(k as u32 + 1), 0);
                        f_1c011750(m, tmp, si as u16 as u32, attrs, di as u16 as u32);
                        si = si.wrapping_add(1);
                        m.w32(cnt, m.r32(cnt).wrapping_add(1));
                    }
                }
                di = di.wrapping_add(1);
                if (di as i32) >= m.s(st + ST_OUT_LEN) + m.s(st + ST_OUT_BASE) {
                    break;
                }
            }
        }
        let dst = out.wrapping_add(m.s(st + ST_OUT_BASE) as u32);
        strcpy(m, dst, buf);
        let n = m.r32(cnt);
        f_1c011710(m, attrs, m.r16(st + ST_OUT_BASE) as u16 as u32, tmp, 0, n);
        m.w16(st + ST_OUT_LEN, m.r32(cnt) as i32);
    }
    m.w16(st + ST_POS, m.s(st + ST_POS).wrapping_add(1));
    0
}

/// `i` action: insert the build string between every pair of output characters.
pub fn f_1c012440(m: &mut Mem, p1: u32, p2: u32, p3: u32, p4: u32, p5: u32, p6: u32) -> u32 {
    let (rule, out, attrs, slots, st, aux) = (p1, p2, p3, p4, p5, p6);
    if st == 0 {
        return 0;
    }
    if rule == 0 || out == 0 || slots == 0 {
        m.w16(st + ST_STATUS, 4);
        return 0;
    }
    let sp = m.sp();
    let buf = m.alloca(0x64);
    let cntp = m.alloca(4);
    m.w32(cntp, rule);
    f_1c012cc0(m, rule, buf, out, slots, st, aux, 4, cntp);
    if m.r16(st + ST_MODE) != -1 {
        let cnt = m.r32(cntp);
        let len = m.s(st + ST_OUT_LEN);
        let new_len = (len - 1).wrapping_mul(cnt.wrapping_add(1) as i32).wrapping_add(1);
        let start = m.s(st + ST_OUT_BASE);
        let mut src = start + len - 1;
        let mut dst = start.wrapping_add(new_len) - 1;
        if start < src {
            loop {
                let b = m.r8(out.wrapping_add(src as u32));
                m.w8(out.wrapping_add(dst as u32), b);
                f_1c011750(m, attrs, dst as u32, attrs, src as u32);
                f_1c011750(m, attrs, src as u32, attrs, BLANK_ENTRY);
                dst = dst.wrapping_sub(cnt as i32);
                movs(m, out.wrapping_add(dst as u32), buf, cnt);
                src -= 1;
                dst = dst.wrapping_sub(1);
                if src <= start {
                    break;
                }
            }
        }
        m.w16(st + ST_OUT_LEN, new_len);
    }
    m.w16(st + ST_POS, m.s(st + ST_POS).wrapping_add(1));
    m.set_sp(sp);
    0
}

/// `a` action: append the build string to the output span.
pub fn f_1c012550(m: &mut Mem, p1: u32, p2: u32, p3: u32, p4: u32, p5: u32) -> u32 {
    let (rule, out, slots, st, aux) = (p1, p2, p3, p4, p5);
    if st == 0 {
        return 0;
    }
    if rule == 0 || out == 0 || slots == 0 {
        m.w16(st + ST_STATUS, 4);
        return 0;
    }
    let sp = m.sp();
    let buf = m.alloca(0x64);
    let cntp = m.alloca(4);
    m.w32(cntp, st);
    f_1c012cc0(m, rule, buf, out, slots, st, aux, 5, cntp);
    if m.r16(st + ST_MODE) != -1 {
        let dst = out.wrapping_add(m.s(st + ST_OUT_BASE) as u32).wrapping_add(m.s(st + ST_OUT_LEN) as u32);
        strcpy(m, dst, buf);
        m.w16(st + ST_OUT_LEN, m.s(st + ST_OUT_LEN).wrapping_add(m.r32(cntp) as i32));
    }
    m.w16(st + ST_POS, m.s(st + ST_POS).wrapping_add(1));
    m.set_sp(sp);
    0
}

/// `b` action: prepend the build string to the output span.
pub fn f_1c012600(m: &mut Mem, p1: u32, p2: u32, p3: u32, p4: u32, p5: u32, p6: u32) -> u32 {
    let (rule, out, attrs, slots, st, aux) = (p1, p2, p3, p4, p5, p6);
    if st == 0 {
        return 0;
    }
    if rule == 0 || out == 0 || slots == 0 {
        m.w16(st + ST_STATUS, 4);
        return 0;
    }
    let sp = m.sp();
    let buf = m.alloca(0x64);
    let cntp = m.alloca(4);
    m.w32(cntp, st);
    f_1c012cc0(m, rule, buf, out, slots, st, aux, 6, cntp);
    if m.r16(st + ST_MODE) != -1 {
        let cnt = m.r32(cntp);
        let mut i = m.s(st + ST_OUT_LEN) + m.s(st + ST_OUT_BASE) - 1;
        while i >= m.s(st + ST_OUT_BASE) {
            let b = m.r8(out.wrapping_add(i as u32));
            m.w8(out.wrapping_add(i as u32).wrapping_add(cnt), b);
            f_1c011750(m, attrs, (i as u32).wrapping_add(cnt), attrs, i as u32);
            i -= 1;
        }
        let dst = out.wrapping_add(m.s(st + ST_OUT_BASE) as u32);
        movs(m, dst, buf, cnt);
        let mut k: i32 = 0;
        while k < cnt as i32 {
            let idx = (k as i16).wrapping_add(m.r16(st + ST_OUT_BASE));
            f_1c011750(m, attrs, idx as u16 as u32, attrs, BLANK_ENTRY);
            k += 1;
        }
        m.w16(st + ST_OUT_LEN, m.s(st + ST_OUT_LEN).wrapping_add(cnt as i32));
    }
    m.w16(st + ST_POS, m.s(st + ST_POS).wrapping_add(1));
    m.set_sp(sp);
    0
}

/// `(` action: save the matched output span into string slot p2 (`$n`).
pub fn f_1c012720(m: &mut Mem, p1: u32, p2: u32, p3: u32, p4: u32) -> u32 {
    let (out, slot, slots, st) = (p1, p2, p3, p4);
    if st == 0 {
        return 0;
    }
    if out == 0 || slots == 0 {
        m.w16(st + ST_STATUS, 4);
        return 0;
    }
    if (slot as i32) < 0 || (slot as i32) > 10 {
        m.w16(st + ST_STATUS, 4);
        return 0;
    }
    if m.r16(st + ST_MODE) == -1 {
        return 0;
    }
    let len = m.r16(st + ST_OUT_LEN);
    if len >= 0x1e {
        m.w16(st + ST_STATUS, 0);
        return 0;
    }
    let dst = slots.wrapping_add(slot.wrapping_mul(30));
    movs(m, dst, out.wrapping_add(m.s(st + ST_OUT_BASE) as u32), len as i32 as u32);
    m.w8(dst.wrapping_add(m.s(st + ST_OUT_LEN) as u32), 0);
    0
}

/// `h` action: look the matched text up in a word category (p7); on a hit either accept (exact
/// mode) or run the `|`-separated success branch, else the failure branch.
pub fn f_1c0127b0(m: &mut Mem, p1: u32, p2: u32, p3: u32, p4: u32, p5: u32, p6: u32, p7: u32, p8: u32, p9: u32) -> u32 {
    let (rule, text, out, in_attrs, out_attrs, slots, cat, st, exact) = (p1, p2, p3, p4, p5, p6, p7, p8, p9);
    if m.r16(st + ST_IN_LEN) >= 0x1e || m.r16(st + ST_OUT_LEN) >= 0x1e {
        m.w16(st + ST_STATUS, 0);
        return 0;
    }
    let in_len = m.s(st + ST_IN_LEN) as u32;
    movs(m, slots + 0xd2, text.wrapping_add(m.s(st + ST_IN_BASE) as u32), in_len);
    m.w8(slots.wrapping_add(m.s(st + ST_IN_LEN) as u32) + 0xd2, 0);
    let out_len = m.s(st + ST_OUT_LEN) as u32;
    movs(m, slots + 0xf0, out.wrapping_add(m.s(st + ST_OUT_BASE) as u32), out_len);
    m.w8(slots.wrapping_add(m.s(st + ST_OUT_LEN) as u32) + 0xf0, 0);
    let ax = f_1c012a20(m, slots + 0xf0, slots + 0x10e, cat, exact) as u16;
    let fail_at = |m: &Mem, want: u32| m.r32(rule.wrapping_add(m.s(st + ST_POS) as u32)) == m.r32(want);
    if exact as u16 != 0 {
        if ax != 0 {
            m.w16(st + ST_STATUS, 1);
        }
        return 0;
    }
    if ax == 1 {
        if fail_at(m, FAIL_1) {
            m.w16(st + ST_STATUS, 0);
            if m.r16(st + ST_MODE) != 1 {
                return 0;
            }
            m.w16(st + ST_MODE, -1);
            m.w16(st + ST_POS, m.s(st + ST_POS) + 5);
        }
        m.w16(st + ST_IN_LEN, 0);
        m.w16(st + ST_OUT_LEN, 0);
        f_1c0117b0(m, rule, 1, text, out, in_attrs, out_attrs, slots, st, 0x7c, 0);
        if fail_at(m, FAIL_2) {
            m.w16(st + ST_POS, m.s(st + ST_POS) + 5);
            return 0;
        }
        return f_1c0140a0(m, rule, st, 1, 0x2f);
    }
    if fail_at(m, FAIL_3) {
        m.w16(st + ST_POS, m.s(st + ST_POS) + 5);
    } else {
        f_1c0140a0(m, rule, st, 1, 0x7c);
    }
    if fail_at(m, FAIL_4) {
        m.w16(st + ST_STATUS, 0);
        if m.r16(st + ST_MODE) == 1 {
            m.w16(st + ST_MODE, -1);
            m.w16(st + ST_POS, m.s(st + ST_POS) + 5);
        }
        return 0;
    }
    m.w16(st + ST_IN_LEN, 0);
    m.w16(st + ST_OUT_LEN, 0);
    f_1c0117b0(m, rule, 1, text, out, in_attrs, out_attrs, slots, st, 0x2f, 0)
}

/// Run rule text p1 (kind 10 = `h` lookup state) against p2 with slots p3; true if it matched.
pub fn f_1c0129b0(m: &mut Mem, p1: u32, p2: u32, p3: u32, p4: u32) -> u32 {
    let st = p4;
    let sp = m.sp();
    let frame = m.alloca(0x2bc);
    let buf = frame;
    let attrs = frame + 0x60;
    m.w16(st + ST_OUT_BASE, 0);
    m.w16(st + ST_OUT_LEN, 0);
    m.w16(st + ST_STATUS, 1);
    f_1c0117b0(m, p1, 10, p2, buf, attrs, attrs, p3, st, 0x2f, 1);
    m.set_sp(sp);
    (m.r16(st + ST_STATUS) == 1) as u32
}

fn lookup_key(m: &Mem, k: i32) -> u32 {
    let off = m.r16(LOOKUP_INDEX.wrapping_add((k as i16 as i32 * 2) as u32));
    LOOKUP_POOL.wrapping_add(off as i32 as u32)
}

/// Case-sensitive compare as inlined at 1c012bb4: 0, else -1 / 1 by the first differing byte.
fn strcmp_sign(m: &Mem, a: u32, b: u32) -> i32 {
    let mut k = 0u32;
    loop {
        let x = m.r8(a.wrapping_add(k));
        let y = m.r8(b.wrapping_add(k));
        if x != y {
            return if x < y { -1 } else { 1 };
        }
        if x == 0 {
            return 0;
        }
        k += 1;
    }
}

/// Binary-search word p1 in lookup category p3 (1-based). p4 != 0: existence test only (ax = 1
/// on a hit). Otherwise copy the entry's value to p2, preferring a case-exact match when the
/// case-insensitive run of duplicates holds a capitalized key.
pub fn f_1c012a20(m: &mut Mem, p1: u32, p2: u32, p3: u32, p4: u32) -> u32 {
    let word = p1;
    let e = (sx16(p3.wrapping_sub(1)) as i32).wrapping_mul(6) as u32;
    let lo = m.r16(LOOKUP_CATS.wrapping_add(e));
    let hi = m.r16(LOOKUP_CATS.wrapping_add(e) + 2);
    let mut eax: u32 = 0;
    let mut esi: i32 = 0;
    let mut local: i32 = 0;
    let mut ebx: i32 = lo as i32;
    let mut edi: i32 = hi as i32;
    let mut bp: i32 = lo as i32;
    if lo <= hi {
        loop {
            esi = (edi as i16 as i32 + ebx as i16 as i32) >> 1;
            eax = stricmp(m, word, lookup_key(m, esi)) as u32;
            if eax as u16 == 0 {
                break;
            }
            if (eax as i16) < 0 {
                edi = esi - 1;
            } else {
                ebx = esi + 1;
            }
            if ebx as i16 > edi as i16 {
                break;
            }
        }
    }
    if p4 as u16 != 0 {
        if eax as u16 != 0 {
            return eax & 0xffff_0000;
        }
        return (eax & 0xffff_0000) | 1;
    }
    if eax as u16 != 0 {
        return eax & 0xffff_0000;
    }
    ebx = esi - 1;
    while ebx as i16 >= bp as i16 {
        if stricmp(m, word, lookup_key(m, ebx)) != 0 {
            break;
        }
        ebx -= 1;
    }
    edi = esi + 1;
    while edi as i16 >= bp as i16 {
        if stricmp(m, word, lookup_key(m, edi)) != 0 {
            break;
        }
        edi += 1;
    }
    ebx += 1;
    edi -= 1;
    bp = edi;
    'found: {
        if edi as i16 == ebx as i16 {
            break 'found;
        }
        loop {
            let c = m.r8(lookup_key(m, edi)) as u32;
            if m.r8(CHAR_BITS + c * 2) & 2 != 0 {
                if ebx as i16 == edi as i16 {
                    break 'found;
                }
                if ebx as i16 > edi as i16 {
                    bp = local;
                    break 'found;
                }
                let mut r;
                loop {
                    local = (edi as i16 as i32 + ebx as i16 as i32) >> 1;
                    r = strcmp_sign(m, word, lookup_key(m, local));
                    if r == 0 {
                        bp = local;
                        break 'found;
                    }
                    if r < 0 {
                        edi = local - 1;
                    } else {
                        ebx = local + 1;
                    }
                    if ebx as i16 > edi as i16 {
                        break;
                    }
                }
                return (bp as i16 as i32 as u32) & 0xffff_0000;
            }
            edi -= 1;
            if edi as i16 == ebx as i16 {
                break 'found;
            }
        }
    }
    let key = lookup_key(m, bp);
    let value = key.wrapping_add(strlen(m, key)).wrapping_add(1);
    strcpy(m, p2, value);
    1
}

/// `w` action: fail unless the span has both an upper-case and a lower-case letter, is all
/// letters, and is at least 2 long.
pub fn f_1c012c30(m: &mut Mem, p1: u32, p2: u32) -> u32 {
    let (out, st) = (p1, p2);
    if st == 0 {
        return 0;
    }
    if out == 0 {
        m.w16(st + ST_STATUS, 4);
        return 0;
    }
    if m.r16(st + ST_MODE) == -1 {
        return 0;
    }
    let start = m.s(st + ST_OUT_BASE);
    let end = m.s(st + ST_OUT_LEN) + start;
    let (mut upper, mut lower) = (false, false);
    if start < end {
        let mut i = start;
        loop {
            if upper && lower {
                break;
            }
            let c = m.r8(out.wrapping_add(i as u32)) as u32;
            let bits = m.r16(CHAR_BITS + c * 2) as u16;
            if bits & 0x200 != 0 {
                upper = true;
            }
            if bits & 0x100 != 0 {
                lower = true;
            }
            if bits & 8 == 0 {
                m.w16(st + ST_STATUS, 0);
                return 0;
            }
            i += 1;
            if i >= end {
                break;
            }
        }
        if upper && lower && m.r16(st + ST_OUT_LEN) >= 2 {
            return 0;
        }
    }
    m.w16(st + ST_STATUS, 0);
    0
}

/// Build the action string from rule text p1 at st.pos into p2: `'lit'`, `$n` slots, `0xHH`,
/// and `|`-separated alternatives chosen by a number (p7 selects its source). Stores the length
/// in *p8, advances st.pos to the closing `/`, returns p2 (0 on a rule error).
pub fn f_1c012cc0(m: &mut Mem, p1: u32, p2: u32, p3: u32, p4: u32, p5: u32, p6: u32, p7: u32, p8: u32) -> u32 {
    let (rule, dst, text, slots, st, aux, kind, lenp) = (p1, p2, p3, p4, p5, p6, p7, p8);
    if st == 0 {
        return 0;
    }
    if rule == 0 || dst == 0 || slots == 0 || kind == 0 || lenp == 0 {
        m.w16(st + ST_STATUS, 4);
        return 0;
    }
    let mut bar: i32 = 0;
    let mut num: i32 = 0;
    let mut len: u32 = 0;
    let mut pos = sx16(m.r16(st + ST_POS) as u32);
    m.w8(dst, 0);
    let at = |m: &Mem, i: u32| m.r8(rule.wrapping_add(i));
    let skip = |m: &Mem| m.r16(st + ST_MODE) == -1;
    let mut c = at(m, pos);
    let err = |m: &mut Mem, fmt: u32, pos: u32| {
        f_1c014d60(m, fmt, rule, pos);
        m.w16(st + ST_STATUS, 4);
        0
    };
    if c != b'/' {
        loop {
            while c != b'/' && bar <= 0 {
                match c {
                    b'\'' => {
                        let mut al = at(m, pos.wrapping_add(1));
                        pos = pos.wrapping_add(1);
                        while al != 0 && al != b'\'' {
                            if al == b'\\' {
                                pos = pos.wrapping_add(1);
                            }
                            if !skip(m) {
                                let b = at(m, pos);
                                m.w8(dst.wrapping_add(len), b);
                                len = len.wrapping_add(1);
                            }
                            al = at(m, pos.wrapping_add(1));
                            pos = pos.wrapping_add(1);
                        }
                        if at(m, pos) != b'\'' {
                            return err(m, 0x1c06_431c, pos);
                        }
                        pos = pos.wrapping_add(1);
                    }
                    b'`' => {
                        f_1c014d60(m, 0x1c06_4354, rule, pos);
                        m.w16(st + ST_STATUS, 4);
                    }
                    b'$' => {
                        pos = pos.wrapping_add(1);
                        if !skip(m) {
                            let v = (at(m, pos) as i32).wrapping_sub(0x30);
                            if !(0..=10).contains(&v) {
                                return err(m, 0x1c06_eff4, pos);
                            }
                            let src = slots.wrapping_add((v * 30) as u32);
                            let n = strlen(m, src);
                            movs(m, dst.wrapping_add(len), src, n);
                            len = len.wrapping_add(n);
                        }
                        pos = pos.wrapping_add(1);
                    }
                    b'|' => {
                        if bar == -1 {
                            bar = 2;
                        } else {
                            bar = 1;
                            let base = m.s(st + ST_OUT_BASE) as u32;
                            let olen = m.s(st + ST_OUT_LEN) as u32;
                            m.w8(text.wrapping_add(olen).wrapping_add(base), 0);
                            num = if skip(m) {
                                0
                            } else if m.r16(aux + 6) == 2 {
                                m.s(aux)
                            } else if kind == 5 || kind == 6 {
                                let d = if kind == 5 {
                                    m.r8(text.wrapping_add(olen).wrapping_add(base).wrapping_sub(1))
                                } else {
                                    m.r8(text.wrapping_add(base))
                                };
                                let v = d as i32 - 0x30;
                                if (0..=9).contains(&v) { v } else { 0 }
                            } else {
                                m.w8(text.wrapping_add(olen).wrapping_add(base), 0);
                                let ax = f_1c0137d0(m, text.wrapping_add(m.s(st + ST_OUT_BASE) as u32));
                                let mut v = sx16(ax) as i32;
                                if m.r16(aux + 6) == 1 {
                                    v -= m.s(aux);
                                }
                                v
                            };
                        }
                    }
                    b'0' => {
                        if !skip(m) {
                            if at(m, pos.wrapping_add(1)) != b'x' {
                                return err(m, 0x1c06_43b0, pos);
                            }
                            let ax = f_1c013700(m, rule.wrapping_add(pos), 2);
                            if (ax as i16) < 0 {
                                return err(m, 0x1c06_43d8, pos);
                            }
                            m.w8(dst.wrapping_add(len), ax as u8);
                            len = len.wrapping_add(1);
                        }
                        pos = pos.wrapping_add(4);
                    }
                    _ => return err(m, 0x1c06_43f8, pos),
                }
                c = at(m, pos);
            }
            if num > 0 && bar == 1 {
                pos = f_1c013510(m, rule, pos, num as u32);
                bar = -1;
                len = 0;
                m.w8(dst, 0);
            } else if bar == 2 || num == 0 {
                pos = f_1c013620(m, rule, pos);
                bar = 0;
            }
            c = at(m, pos);
            if c == b'/' {
                break;
            }
        }
    }
    if !skip(m) {
        m.w8(dst.wrapping_add(len), 0);
        m.w32(lenp, len);
    }
    m.w16(st + ST_POS, pos as i32);
    dst
}

/// Skip p3 `|`-separated alternatives of a build string starting at p2; returns the new index.
pub fn f_1c013510(m: &mut Mem, p1: u32, p2: u32, p3: u32) -> u32 {
    let rule = p1;
    if rule == 0 {
        return p2;
    }
    let mut i = p2;
    let mut n = p3 as i32;
    let at = |m: &Mem, i: u32| m.r8(rule.wrapping_add(i));
    while n > 0 {
        match at(m, i) {
            b'\'' => skip_quoted(m, rule, &mut i, b'\''),
            b'`' => {
                f_1c014d60(m, 0x1c06_44d4, rule, i);
                skip_quoted(m, rule, &mut i, b'`');
            }
            b'/' => {
                f_1c014d60(m, 0x1c06_4508, rule, i);
                n = -1;
                i = i.wrapping_sub(1);
            }
            b'|' => n -= 1,
            _ => {}
        }
        i = i.wrapping_add(1);
    }
    i
}

fn skip_quoted(m: &Mem, rule: u32, i: &mut u32, q: u8) {
    let mut al = m.r8(rule.wrapping_add(*i).wrapping_add(1));
    *i = i.wrapping_add(1);
    while al != q {
        if al == b'\\' {
            *i = i.wrapping_add(1);
        }
        al = m.r8(rule.wrapping_add(*i).wrapping_add(1));
        *i = i.wrapping_add(1);
    }
}

/// Skip to the closing `/` of a build string from p2; returns its index.
pub fn f_1c013620(m: &mut Mem, p1: u32, p2: u32) -> u32 {
    let rule = p1;
    if rule == 0 {
        return p2;
    }
    let mut i = p2;
    let mut al = m.r8(rule.wrapping_add(i));
    while al != b'/' {
        if al == b'\'' {
            skip_quoted(m, rule, &mut i, b'\'');
        } else if al == b'`' {
            f_1c014d60(m, 0x1c06_4540, rule, i);
            skip_quoted(m, rule, &mut i, b'`');
        }
        al = m.r8(rule.wrapping_add(i).wrapping_add(1));
        i = i.wrapping_add(1);
    }
    i
}

/// Copy p5 matched characters (and their attributes) from input to output at the state's
/// current input/output positions.
pub fn f_1c013690(m: &mut Mem, p1: u32, p2: u32, p3: u32, p4: u32, p5: u32, p6: u32) -> u32 {
    let (text, in_attrs, out, out_attrs, n, st) = (p1, p2, p3, p4, p5 as i32, p6);
    let mut i: i32 = 0;
    while i < n {
        let src = m.s(st + ST_IN_BASE) + i + m.s(st + ST_IN_LEN);
        let dst = m.s(st + ST_OUT_BASE) + i + m.s(st + ST_OUT_LEN);
        let b = m.r8(text.wrapping_add(src as u32));
        m.w8(out.wrapping_add(dst as u32), b);
        let d = (m.r16(st + ST_OUT_LEN) as u16).wrapping_add(m.r16(st + ST_OUT_BASE) as u16).wrapping_add(i as u16);
        let s = (m.r16(st + ST_IN_LEN) as u16).wrapping_add(i as u16).wrapping_add(m.r16(st + ST_IN_BASE) as u16);
        f_1c011750(m, out_attrs, d as u32, in_attrs, s as u32);
        i += 1;
    }
    0
}

/// Parse `0xHH..` (p2 hex digits) at p1; ax = -1 if malformed.
pub fn f_1c013700(m: &mut Mem, p1: u32, p2: u32) -> u32 {
    let mut eax: u32 = 0;
    let mut edx: u32 = 2;
    if m.r8(p1) != b'0' || m.r8(p1 + 1) != b'x' {
        return eax | 0xffff;
    }
    let end = p2.wrapping_add(2) as i32;
    if end <= 2 {
        return eax;
    }
    loop {
        let c = m.r8(p1.wrapping_add(edx)) as u32;
        let v = if c <= 0x39 { c.wrapping_sub(0x30) } else { c.wrapping_sub(0x37) };
        if (v as i16) < 0 || (v as i16) > 0xf {
            return (eax & 0xffff_0000) | 0xffff;
        }
        eax = (eax << 4).wrapping_add(v);
        edx = edx.wrapping_add(1);
        if (edx as i16 as i32) >= end {
            return eax;
        }
    }
}

/// Decimal value of at most p2 leading digits of p1.
pub fn f_1c013760(m: &mut Mem, p1: u32, p2: u32) -> u32 {
    let n = sx16(p2) as i32;
    let mut v: u32 = 0;
    let mut k: i32 = 0;
    while k < n {
        let d = (m.r8(p1.wrapping_add(k as u32)) as i32) - 0x30;
        if !(0..=9).contains(&d) {
            break;
        }
        v = v.wrapping_mul(10).wrapping_add(d as u32);
        k += 1;
    }
    v
}

/// Decimal value of the leading digits of p1; *p2 (u16) = digit count.
pub fn f_1c013790(m: &mut Mem, p1: u32, p2: u32) -> u32 {
    let mut v: u32 = 0;
    let mut n: u32 = 0;
    loop {
        let d = (m.r8(p1.wrapping_add(n)) as i32) - 0x30;
        if !(0..=9).contains(&d) {
            break;
        }
        v = v.wrapping_mul(10).wrapping_add(d as u32);
        n += 1;
    }
    m.w16(p2, n as i32);
    v
}

/// Decimal value of the leading digits of p1.
pub fn f_1c0137d0(m: &mut Mem, p1: u32) -> u32 {
    let mut v: u32 = 0;
    let mut p = p1;
    loop {
        let d = (m.r8(p) as i32) - 0x30;
        if !(0..=9).contains(&d) {
            return v;
        }
        v = v.wrapping_mul(10).wrapping_add(d as u32);
        p = p.wrapping_add(1);
    }
}

/// Number of decimal digits of (short)p1 (1 for zero).
pub fn f_1c013800(m: &mut Mem, p1: u32) -> u32 {
    let _ = m;
    let mut v = p1 as i16;
    if v == 0 {
        return 1;
    }
    let mut n: u16 = 0;
    while v != 0 {
        v /= 10;
        n += 1;
    }
    n as u32
}
