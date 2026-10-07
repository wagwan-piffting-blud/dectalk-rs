//! DTparser rule-syntax walkers (skip/validate a rule's states and character-type specifiers)
//! and the set/range matcher (`{a,b}<n-m>` groups) driving FUN_1c0130f0.
//!
//! Rule text: a NUL-terminated byte string addressed as `rule + sx16(idx)`.
//!
//! Parse state (0x18 bytes, copied whole by FUN_1c014dc0):
//!   +0x00 i16  text base          +0x02 i16  text offset (base+offset = current text position)
//!   +0x04 i16  aux position       +0x06 i16  aux offset / alt-state (0 none, 2 one alt, -1 many)
//!   +0x08 i16  rule index         +0x0a i16  status (0 fail/stop, 1 ok, 3 set exhausted, 4 error)
//!   +0x0c i16  match flag (1 / -1 = matched-to-end)
//!   +0x0e i16  carried            +0x10 i16, +0x12 i16 (never initialised by the original)
//!   +0x14 u32  carried
//! The alt-state struct (FUN_1c015c00 p7) uses +0x00 (chosen set member) and +0x06 as above.

use super::*;

const FAIL_WORD_A: u32 = 0x1c06_469c;
const FAIL_WORD_B: u32 = 0x1c06_46bc;

fn idx(m: &Mem, st: u32) -> i32 {
    m.s(st + 8)
}

fn set_idx(m: &mut Mem, st: u32, v: i32) {
    m.w16(st + 8, v);
}

fn at(m: &Mem, s: u32, i: i32) -> u8 {
    m.r8(s.wrapping_add(i as u32))
}

fn rule_error(m: &mut Mem, msg: u32, rule: u32, pos: i32, st: u32) -> u32 {
    let r = f_1c014d60(m, msg, rule, pos as u32);
    m.w16(st + 0xa, 4);
    r
}

/// FUN_1c013790 through a scratch out-slot: (value, digit count).
fn read_number(m: &mut Mem, addr: u32) -> (i32, i32) {
    let sp = m.sp();
    let slot = m.alloca(4);
    let v = f_1c013790(m, addr, slot);
    let len = m.s(slot);
    m.set_sp(sp);
    (v as u16 as i16 as i32, len)
}

/// Messages for the shared `open n[,n|-n|*|+]... close` repeat-count walker.
struct RangeMsgs {
    no_open: u32,
    star: u32,
    plus: u32,
    range: u32,
    syntax: u32,
}

/// Tail shared by FUN_1c014660 / FUN_1c014840 / FUN_1c014a10: idx sits on the opening bracket.
fn skip_repeat_range(m: &mut Mem, rule: u32, st: u32, open: u8, close: u8, msgs: &RangeMsgs) -> u32 {
    let i = idx(m, st);
    if at(m, rule, i) != open {
        return rule_error(m, msgs.no_open, rule, i, st);
    }
    set_idx(m, st, i + 1);
    loop {
        let i = idx(m, st);
        if at(m, rule, i) == close {
            set_idx(m, st, i + 1);
            return i as u32;
        }
        match at(m, rule, i) {
            b'*' => {
                set_idx(m, st, i + 1);
                let i = idx(m, st);
                if at(m, rule, i) != close {
                    return rule_error(m, msgs.star, rule, i, st);
                }
            }
            b'+' => {
                set_idx(m, st, i + 1);
                let i = idx(m, st);
                if at(m, rule, i) != close {
                    return rule_error(m, msgs.plus, rule, i, st);
                }
            }
            _ => {
                let (_, len) = read_number(m, rule.wrapping_add(i as u32));
                set_idx(m, st, i + len);
                let i = idx(m, st);
                let c = at(m, rule, i);
                if c == b',' {
                    set_idx(m, st, i + 1);
                } else if c == b'-' {
                    // The original tests for '*'/'+' here against the '-' it just read: dead code.
                    set_idx(m, st, i + 1);
                    let (_, len) = read_number(m, rule.wrapping_add(sx16((i + 1) as u32)));
                    set_idx(m, st, idx(m, st) + len);
                    let j = idx(m, st);
                    let c = at(m, rule, j);
                    if c != close && c != b',' {
                        return rule_error(m, msgs.range, rule, j, st);
                    }
                } else if c != close {
                    return rule_error(m, msgs.syntax, rule, i, st);
                }
            }
        }
    }
}

/// par_skip_states: walk one rule state list of kind `kind` up to terminator `term`.
/// kind 0 = whole rule (to NUL), 8 = save state `N)`, 9 = macro `Rn...`, 10 = numbered state.
pub fn f_1c0140a0(m: &mut Mem, p1: u32, p2: u32, p3: u32, p4: u32) -> u32 {
    let rule = p1;
    let st = p2;
    let kind = p3 as u16 as i16;
    let term: u8;
    let mut skip_body = false;
    match kind {
        0 => term = 0,
        8 => {
            let i = idx(m, st) + 1;
            set_idx(m, st, i);
            let i = i as i16 as i32;
            let c = at(m, rule, i);
            if !c.is_ascii_digit() {
                return rule_error(m, 0x1c06_45c0, rule, i, st);
            }
            set_idx(m, st, i + 2);
            term = b')';
        }
        10 => {
            let sp = m.sp();
            let slot = m.alloca(4);
            m.w32(slot, p2);
            f_1c013790(m, rule.wrapping_add(idx(m, st) as u32), slot);
            let n = m.r32(slot).wrapping_add(1);
            m.set_sp(sp);
            set_idx(m, st, idx(m, st).wrapping_add(n as i32));
            term = p4 as u8;
        }
        9 => {
            let i = idx(m, st);
            if at(m, rule, i) != b'R' {
                return rule_error(m, 0x1c06_45e4, rule, i, st);
            }
            let i = (i + 1) as i16 as i32;
            set_idx(m, st, i);
            let (_, len) = read_number(m, rule.wrapping_add(i as u32));
            set_idx(m, st, idx(m, st) + len);
            term = p4 as u8;
            skip_body = true;
        }
        _ => term = p4 as u8,
    }
    if !skip_body {
        loop {
            let c = at(m, rule, idx(m, st));
            if c == term {
                break;
            }
            let t = f_1c011fa0(m, c as u32);
            let eax;
            if t as u16 == 0 {
                let a = f_1c012100(m, at(m, rule, idx(m, st)) as u32);
                if a as u16 == 0 {
                    let i = idx(m, st);
                    return rule_error(m, 0x1c06_4634, rule, i, st);
                }
                let i = idx(m, st) + 1;
                set_idx(m, st, i);
                let i = i as i16 as i32;
                if a as u16 != 8 {
                    if at(m, rule, i) != b'/' {
                        return rule_error(m, 0x1c06_4610, rule, i, st);
                    }
                    set_idx(m, st, i + 1);
                }
                eax = f_1c0140a0(m, rule, st, a, b'/' as u32);
            } else {
                let mut t = t;
                if t as u16 == 0x78 {
                    set_idx(m, st, idx(m, st) + 1);
                    t = f_1c011fa0(m, at(m, rule, idx(m, st)) as u32);
                }
                eax = f_1c0143f0(m, rule, st, t);
            }
            if m.r16(st + 0xa) == 4 {
                return eax;
            }
        }
    }
    let i = idx(m, st);
    if at(m, rule, i) != term {
        return rule_error(m, 0x1c06_4674, rule, i, st);
    }
    if kind != 0 {
        set_idx(m, st, i + 1);
    }
    f_1c0142a0(m, rule, st, p3)
}

/// par_skip_states tail: consume the action part following a state of kind `kind`
/// (kinds 2/4/5/6/12: a `|`-separated type list up to '/'; kind 10: `FAIL|` / `FAIL/` or states).
pub fn f_1c0142a0(m: &mut Mem, p1: u32, p2: u32, p3: u32) -> u32 {
    let rule = p1;
    let st = p2;
    let sel = sx16(p3).wrapping_sub(2);
    match p3 as u16 as i16 {
        2 | 4 | 5 | 6 | 12 => {
            let mut hi = sel;
            loop {
                let i = idx(m, st);
                let c = at(m, rule, i);
                if c == b'/' {
                    break;
                }
                if c == b'|' {
                    set_idx(m, st, i + 1);
                }
                let t = f_1c011fa0(m, at(m, rule, idx(m, st)) as u32);
                f_1c0143f0(m, rule, st, t);
                let i = idx(m, st);
                if at(m, rule, i) == b'|' {
                    set_idx(m, st, i + 1);
                }
                hi = idx(m, st) as u32;
            }
            set_idx(m, st, idx(m, st) + 1);
            (hi & !0xff) | b'/' as u32
        }
        10 => {
            let i = idx(m, st);
            if m.r32(rule.wrapping_add(i as u32)) == m.r32(FAIL_WORD_A) {
                set_idx(m, st, i + 5);
            } else {
                f_1c0140a0(m, rule, st, 1, b'|' as u32);
                let i = idx(m, st);
                if at(m, rule, i) == 0 {
                    return rule_error(m, 0x1c06_46a4, rule, i, st);
                }
            }
            let i = idx(m, st);
            if m.r32(rule.wrapping_add(i as u32)) == m.r32(FAIL_WORD_B) {
                set_idx(m, st, i + 5);
                return (i + 5) as u32;
            }
            f_1c0140a0(m, rule, st, 1, b'/' as u32);
            let i = idx(m, st);
            if at(m, rule, i) == 0 {
                return rule_error(m, 0x1c06_46c4, rule, i, st);
            }
            i as u32
        }
        _ => sel,
    }
}

/// par_skip_char_type: skip one character-type specifier `kind` (as classified by FUN_1c011fa0).
pub fn f_1c0143f0(m: &mut Mem, p1: u32, p2: u32, p3: u32) -> u32 {
    let rule = p1;
    let st = p2;
    let kind = sx16(p3) as i32;
    match kind {
        0x20 | 0x40 | 0x80 | 0x100 | 0x200 | 0x400 | 0x800 | 0x1000 | 0x2000 | 0x4000 | 2 | 4 | 8 | 0x10 => {
            f_1c014660(m, rule, st)
        }
        1 => {
            let i = idx(m, st);
            if at(m, rule, i + 1) == b'[' || at(m, rule, i + 2) == b'[' {
                f_1c014840(m, rule, st)
            } else {
                f_1c014660(m, rule, st)
            }
        }
        0x26b7 => f_1c014a10(m, rule, st),
        0x26b9 | 0x26bd => {
            let close = if kind == 0x26b9 { b'\'' } else { b'`' };
            set_idx(m, st, idx(m, st) + 1);
            let mut e = m.r16(st + 8) as u16 as u32;
            while at(m, rule, e as u16 as i16 as i32) != close {
                if at(m, rule, e as u16 as i16 as i32) == b'\\' {
                    e = e.wrapping_add(1);
                    m.w16(st + 8, e as i32);
                }
                e = e.wrapping_add(1);
                m.w16(st + 8, e as i32);
            }
            e = e.wrapping_add(1);
            m.w16(st + 8, e as i32);
            e
        }
        0x26bf => {
            let r = f_1c013700(m, rule.wrapping_add(idx(m, st) as u32), 2);
            if (r as u16 as i16) < 0 {
                let i = idx(m, st);
                return rule_error(m, 0x1c06_46dc, rule, i, st);
            }
            set_idx(m, st, idx(m, st) + 4);
            r
        }
        0x26c3 => {
            let i = idx(m, st) + 1;
            set_idx(m, st, i);
            let i = i as i16 as i32;
            if !at(m, rule, i).is_ascii_digit() {
                return rule_error(m, 0x1c06_4700, rule, i, st);
            }
            set_idx(m, st, i + 1);
            i as u32
        }
        _ => {
            let i = idx(m, st);
            rule_error(m, 0x1c06_4724, rule, i, st)
        }
    }
}

/// Skip a `[x][~]<counts>` character-type specifier.
pub fn f_1c014660(m: &mut Mem, p1: u32, p2: u32) -> u32 {
    let rule = p1;
    let st = p2;
    set_idx(m, st, idx(m, st) + 1);
    if at(m, rule, idx(m, st)) == b'x' {
        set_idx(m, st, idx(m, st) + 1);
    }
    if at(m, rule, idx(m, st)) == b'~' {
        set_idx(m, st, idx(m, st) + 1);
    }
    let msgs = RangeMsgs {
        no_open: 0x1c06_4748,
        star: 0x1c06_4784,
        plus: 0x1c06_47a0,
        range: 0x1c06_47e0,
        syntax: 0x1c06_4804,
    };
    skip_repeat_range(m, rule, st, b'<', b'>', &msgs)
}

/// Skip a `[x][counts]` character-type specifier.
pub fn f_1c014840(m: &mut Mem, p1: u32, p2: u32) -> u32 {
    let rule = p1;
    let st = p2;
    set_idx(m, st, idx(m, st) + 1);
    if at(m, rule, idx(m, st)) == b'x' {
        set_idx(m, st, idx(m, st) + 1);
    }
    let msgs = RangeMsgs {
        no_open: 0x1c06_4824,
        star: 0x1c06_4860,
        plus: 0x1c06_487c,
        range: 0x1c06_48bc,
        syntax: 0x1c06_48e0,
    };
    skip_repeat_range(m, rule, st, b'[', b']', &msgs)
}

/// Skip a `{type,type,...}<counts>` set specifier.
pub fn f_1c014a10(m: &mut Mem, p1: u32, p2: u32) -> u32 {
    let rule = p1;
    let st = p2;
    let i = idx(m, st) + 1;
    set_idx(m, st, i);
    let i = i as i16 as i32;
    if at(m, rule, i) != b'{' {
        return rule_error(m, 0x1c06_4900, rule, i, st);
    }
    set_idx(m, st, i + 1);
    while at(m, rule, idx(m, st)) != b'}' {
        loop {
            let c = at(m, rule, idx(m, st));
            if c == b',' || c == b'}' {
                break;
            }
            // No status check here: an erroring member that does not advance spins, as in the original.
            let t = f_1c011fa0(m, c as u32);
            f_1c0143f0(m, rule, st, t);
        }
        let i = idx(m, st);
        if at(m, rule, i) != b'}' {
            set_idx(m, st, i + 1);
        }
    }
    set_idx(m, st, idx(m, st) + 1);
    let msgs = RangeMsgs {
        no_open: 0x1c06_4930,
        star: 0x1c06_496c,
        plus: 0x1c06_4988,
        range: 0x1c06_49c8,
        syntax: 0x1c06_49ec,
    };
    skip_repeat_range(m, rule, st, b'<', b'>', &msgs)
}

/// par_match_sets_and_ranges: match a `{set}<min-max,...>` group against the text, trying each
/// repeat range in turn. Returns characters consumed, -1 (text end / no match wanted), -2
/// (zero-width match) or 0 with st.status set.
pub fn f_1c0156e0(m: &mut Mem, p1: u32, p2: u32, p3: u32, p4: u32, p5: u32, p6: u32, p7: u32) -> u32 {
    let rule = p1;
    let text = p2;
    let st = p4;
    let verbose = p6;
    if st == 0 {
        return 0;
    }
    if rule == 0 || text == 0 {
        m.w16(st + 0xa, 4);
        return 0;
    }
    let sp = m.sp();
    let local = m.alloca(0x18);
    for k in (0..0x18).step_by(4) {
        m.w32(local + k, 0);
    }
    let base = m.s(st);
    let off = m.s(st + 2);
    m.w16(local, base + off);
    let mut text_pos = base + off;
    m.w16(local + 4, m.s(st + 6) + m.s(st + 4));
    let set_start = idx(m, st) + 1;
    m.w16(local + 0xa, 1);
    m.w32(local + 0x14, m.r32(st + 0x14));
    m.w16(local + 0xe, m.s(st + 0xe));
    m.w16(local + 8, set_start + 1);

    loop {
        let c = at(m, rule, idx(m, local));
        if c == b'}' || c == 0 {
            break;
        }
        let mut c = c;
        while c != b',' && c != b'}' && c != 0 {
            let t = f_1c011fa0(m, c as u32);
            f_1c0143f0(m, rule, local, t);
            c = at(m, rule, idx(m, local));
        }
        let i = idx(m, local);
        if at(m, rule, i) != b'}' {
            set_idx(m, local, i + 1);
        }
    }
    let mut pos = idx(m, local) + 1;
    set_idx(m, local, set_start);

    let bail = |m: &mut Mem, msg: u32, pos: i32| -> u32 {
        if verbose != 0 {
            f_1c014d60(m, msg, rule, pos as u32);
        }
        m.w16(st + 0xa, 4);
        m.set_sp(sp);
        0
    };

    if at(m, rule, pos) != b'<' {
        return bail(m, 0x1c06_4c4c, pos);
    }
    pos += 1;

    let mut acc: i32 = 0;
    let mut stop = false;
    let mut end = false;
    let mut min: i32 = -1;
    let mut result: i32 = -1;
    let mut done_count: i32 = 0;

    let mut c = at(m, rule, pos);
    if c != b'>' {
        loop {
            if end {
                break;
            }
            if c == b'>' {
                pos += 1;
            } else {
                loop {
                    if stop || end {
                        if !end {
                            pos += 1;
                        }
                        break;
                    }
                    let hi: i32;
                    match at(m, rule, pos) {
                        b'*' => {
                            if min == -1 {
                                min = 0;
                            }
                            pos += 1;
                            hi = i32::MAX;
                            if at(m, rule, pos) != b'>' {
                                return bail(m, 0x1c06_4c88, pos);
                            }
                            end = true;
                            pos += 1;
                        }
                        b'+' => {
                            if min < 1 {
                                min = 1;
                            }
                            pos += 1;
                            hi = i32::MAX;
                            if at(m, rule, pos) != b'>' {
                                return bail(m, 0x1c06_4cc0, pos);
                            }
                            end = true;
                            pos += 1;
                        }
                        _ => {
                            let (n, len) = read_number(m, rule.wrapping_add(pos as u32));
                            pos += len;
                            min = n;
                            match at(m, rule, pos) {
                                b',' => {
                                    hi = n;
                                    pos += 1;
                                }
                                b'>' => {
                                    hi = n;
                                    end = true;
                                    pos += 1;
                                }
                                b'-' => {
                                    pos += 1;
                                    let c2 = at(m, rule, pos);
                                    let mut top;
                                    if c2 == b'*' || c2 == b'+' {
                                        let a = at(m, rule, pos + 1);
                                        pos += 1;
                                        top = i32::MAX;
                                        if a != b'>' {
                                            return bail(m, 0x1c06_4cf4, pos);
                                        }
                                        end = true;
                                    } else {
                                        let (n2, len2) = read_number(m, rule.wrapping_add(pos as u32));
                                        pos += len2;
                                        top = n2;
                                        let a = at(m, rule, pos);
                                        if a == b'>' {
                                            end = true;
                                        } else if a != b',' {
                                            return bail(m, 0x1c06_4d38, pos);
                                        }
                                    }
                                    pos += 1;
                                    if top < n {
                                        top = n;
                                    }
                                    hi = top;
                                }
                                _ => return bail(m, 0x1c06_4d74, pos),
                            }
                        }
                    }

                    if m.r16(st + 0xc) != -1 {
                        if min == 0 {
                            result = -2;
                            if p7 == 1 {
                                stop = true;
                                if !end {
                                    pos += 1;
                                }
                                break;
                            }
                        }
                        if done_count < hi {
                            let mut n = done_count + 1;
                            loop {
                                let r = f_1c015c00(
                                    m,
                                    rule,
                                    text,
                                    set_start as u32,
                                    pos as u32,
                                    text_pos as u32,
                                    p3,
                                    p5,
                                    local,
                                    verbose,
                                );
                                let r = r as u16 as i16 as i32;
                                if r == -1 {
                                    stop = true;
                                    if result <= 0 {
                                        acc = -1;
                                    }
                                    break;
                                }
                                let status = m.r16(local + 0xa);
                                if r == 0 {
                                    if status == 1 {
                                        if acc == 0 {
                                            result = -2;
                                            stop = true;
                                            break;
                                        }
                                    } else if status == 0 {
                                        stop = true;
                                        break;
                                    }
                                } else {
                                    if r > 0 {
                                        text_pos += r;
                                        acc += r;
                                    }
                                    if status == 0 {
                                        stop = true;
                                        break;
                                    }
                                }
                                if n < min {
                                    if r == 0 {
                                        stop = true;
                                    }
                                } else {
                                    done_count = n;
                                    result = acc;
                                    if p7 == 1 {
                                        stop = true;
                                        break;
                                    }
                                }
                                let prev = n;
                                n += 1;
                                if prev >= hi {
                                    break;
                                }
                            }
                        }
                    }

                    if at(m, rule, pos) == b'>' {
                        if !end {
                            pos += 1;
                        }
                        break;
                    }
                }
            }
            c = at(m, rule, pos);
            if c == b'>' {
                break;
            }
        }
    }

    if at(m, rule, pos) == b'>' {
        pos += 1;
    }
    m.set_sp(sp);
    if result == -2 {
        set_idx(m, st, pos);
        return if acc == 0 { -2i32 as u32 } else { acc as u32 };
    }
    if result == -1 {
        if m.r16(st + 0xc) == 1 {
            m.w16(st + 0xc, -1);
        }
        m.w16(st + 0xa, 0);
    }
    if at(m, text, text_pos) != 0 || acc != 0 {
        set_idx(m, st, pos);
        return acc as u32;
    }
    let flag = m.r16(st + 0xc);
    set_idx(m, st, pos);
    if flag != 1 && flag != -1 {
        return 0xffff_ffff;
    }
    m.w16(st + 0xc, -1);
    m.w16(st + 0xa, 0);
    acc as u32
}

/// par_match_set: try each comma-separated alternative of the `{...}` set at rule index `p3`
/// against the text at `p5`; the first alternative whose types all match wins and is recorded in
/// the alt-state `p7`. Returns (as a sign-extended i16) the characters consumed, or -1.
/// `p4` (rule position after the range) is passed by the caller but never read.
pub fn f_1c015c00(m: &mut Mem, p1: u32, p2: u32, p3: u32, p4: u32, p5: u32, p6: u32, p7: u32, p8: u32, p9: u32) -> u32 {
    let _ = p4;
    let rule = p1;
    let text = p2;
    let alt = p7;
    let st = p8;
    let verbose = p9;
    if st == 0 {
        return 0;
    }
    if rule == 0 || text == 0 || p6 == 0 || alt == 0 {
        m.w16(st + 0xa, 4);
        return 0;
    }
    let sp = m.sp();
    let cur = m.alloca(0x18);
    let saved = m.alloca(0x18);
    for k in (0..0x18).step_by(4) {
        m.w32(cur + k, 0);
        m.w32(saved + k, 0);
    }
    m.w16(cur + 8, p3 as i32);
    m.w16(cur + 0xc, m.s(st + 0xc));
    m.w32(cur + 0x14, m.r32(st + 0x14));
    m.w16(cur, p5 as i32);
    m.w16(cur + 0xe, m.s(st + 0xe));
    m.w16(cur + 0xa, 1);
    f_1c014dc0(m, saved, cur);

    let i = idx(m, cur);
    if at(m, rule, i) != b'{' {
        if verbose != 0 {
            f_1c014d60(m, 0x1c06_4da4, rule, i as u32);
        }
        m.w16(st + 0xa, 4);
        m.set_sp(sp);
        return 0;
    }
    set_idx(m, cur, i + 1);

    let mut acc: u32 = 0;
    let mut member: u32 = 0;
    let mut done = false;
    let mut c = at(m, rule, idx(m, cur));
    if c != b'}' {
        loop {
            if c == 0 {
                break;
            }
            if done {
                m.set_sp(sp);
                return sx16(acc);
            }
            let mut matched = true;
            acc = 0;
            if c != b',' {
                loop {
                    let c = at(m, rule, idx(m, cur));
                    if c == b'}' || c == 0 {
                        break;
                    }
                    let t = f_1c011fa0(m, c as u32);
                    if t == 0 {
                        if verbose != 0 {
                            let i = idx(m, cur);
                            f_1c014d60(m, 0x1c06_4ddc, rule, i as u32);
                        }
                        m.w16(st + 0xa, 4);
                        m.set_sp(sp);
                        return 0;
                    }
                    let r = f_1c0130f0(m, rule, t, text, p6, cur, alt, 0, 0);
                    m.w16(cur, m.s(cur).wrapping_add(r as i32));
                    acc = acc.wrapping_add(r);
                    if r == 0xffff_ffff {
                        let flag = m.r16(st + 0xc);
                        if flag == 1 || flag == -1 {
                            m.w16(st + 0xc, -1);
                            m.w16(st + 0xa, 0);
                        } else if at(m, rule, idx(m, cur)) == b'}' {
                            m.w16(st + 0xa, 3);
                            m.set_sp(sp);
                            return 0xffff_ffff;
                        }
                    }
                    if m.r16(cur + 0xa) == 0 {
                        matched = false;
                        loop {
                            let c = at(m, rule, idx(m, cur));
                            if c == b',' || c == b'}' || c == 0 {
                                break;
                            }
                            let t = f_1c011fa0(m, c as u32);
                            f_1c0143f0(m, rule, cur, t);
                        }
                    }
                    if at(m, rule, idx(m, cur)) == b',' {
                        break;
                    }
                }
            }
            if matched {
                done = true;
                if m.r16(alt + 6) == 0 {
                    m.w16(alt + 6, 2);
                    m.w16(alt, member as i32);
                } else {
                    m.w16(alt + 6, -1);
                }
            } else {
                let i = idx(m, cur);
                if at(m, rule, i) != b'}' {
                    f_1c014dc0(m, cur, saved);
                    member = member.wrapping_add(1);
                    if at(m, rule, i) == b',' {
                        set_idx(m, cur, i + 1);
                    }
                }
            }
            c = at(m, rule, idx(m, cur));
            if c == b'}' {
                break;
            }
        }
    }
    if !done {
        m.w16(st + 0xa, 0);
    }
    m.set_sp(sp);
    sx16(acc)
}
