//! LTS input/output plumbing: the read-ahead input queue, word gathering, clause punctuation,
//! and symbol emission to the PH pipe.

use super::*;

const CHAR_CLASS: u32 = 0x1c03_244c;
const CHAR_FOLD: u32 = 0x1c02_fb80;
const CHAR_TYPE: u32 = 0x1c02_fd80;

fn class_of(m: &Mem, ch: u32) -> u32 {
    m.r8(CHAR_CLASS.wrapping_add(ch.wrapping_mul(2))) as u32
}

fn fold(m: &Mem, ch: u32) -> u32 {
    m.r8(CHAR_FOLD.wrapping_add(sx16(ch))) as u32
}

/// Start of a clause: check for a leading "spoken punctuation" word and reset form-class state.
pub fn f_1c00caf0(m: &mut Mem, p1: u32, p2: u32) -> u32 {
    let lts = p2;
    if m.r16(lts + 0x8c4) == 0 && m.r16(lts + 0x914) != 0 {
        m.w32(lts + 0x8bc, 1);
        m.w32(lts + 0x8c0, 0xffff_ffff);
        m.w32(lts + 0x4bc, 0);
        m.w32(lts + 0x4c0, 0);
        m.w16(lts + 0x8c4, 2);
        let mut p = lts + 0x914;
        while class_of(m, sx16(m.r16(p) as u32)) & 0x40 != 0 {
            p += 8;
        }
        if f_1c00c440(m, lts, p, 0x1c03_2f34) != 0 {
            m.w16(lts + 0x8c4, 1);
        }
        if f_1c00dfe0(m, p1) != 0 {
            return 3;
        }
        return 0;
    }
    if m.r16(lts + 0x914) != 0 {
        let n = m.r32(lts + 0x8bc);
        if n != 0x100 {
            m.w32(lts + 0x8bc, n + 1);
            m.w32(lts + 0x4bc + (n + 1) * 4, 0);
        }
    }
    0
}

/// Bounds of the gathered word: *p3 = start, *p2 = the terminating record.
pub fn f_1c00cbc0(m: &mut Mem, p1: u32, p2: u32, p3: u32) -> u32 {
    let mut p = p1;
    m.w32(p3, p);
    if m.r16(p) == 0 {
        m.w32(p2, p);
        return p;
    }
    loop {
        p += 8;
        m.w32(p3, p);
        if m.r16(p) == 0 {
            break;
        }
    }
    m.w32(p2, p);
    p
}

/// Read ahead one input item; true if it is a letter/digit-class character.
pub fn f_1c00f070(m: &mut Mem, p1: u32) -> u32 {
    let lts = m.r32(p1 + 0x10);
    f_1c00f190(m, p1);
    let w = m.r16(lts + 0x90c) as u16 as u32;
    if w & 0x1f00 == 0 {
        let c = class_of(m, w & 0xff) & 0xf;
        if c == 1 || c == 4 || c == 3 {
            return 1;
        }
    }
    0
}

/// Is the current item an index/bookmark command?
pub fn f_1c00f0c0(m: &mut Mem, p1: u32) -> u32 {
    let w = m.s(p1 + 2);
    matches!(w, 0x1f08 | 0x1f09 | 0x1f13 | 0x1f14 | 0x1f15 | 0x1f16) as u32
}

/// Is the current item white space?
pub fn f_1c00f100(m: &mut Mem, p1: u32) -> u32 {
    let w = m.r16(p1 + 2) as u16 as u32;
    if w & 0x1f00 != 0 {
        return 0;
    }
    matches!(w & 0xff, 0x20 | 0xa0 | 10 | 0xd | 0xc) as u32
}

/// Advance: move the read-ahead item (lts+0x90a/0x90c) into the current slot (lts+0x900/0x902).
pub fn f_1c00f140(m: &mut Mem, p1: u32) -> u32 {
    let lts = m.r32(p1 + 0x10);
    if m.r16(lts + 0x90a) == 0 {
        f_1c00f190(m, p1);
    }
    let n = m.s(lts + 0x90a);
    m.w16(lts + 0x90a, 0);
    m.w16(lts + 0x900, n);
    for k in 0..4 {
        let w = m.s(lts + 0x90c + k * 2);
        m.w16(lts + 0x902 + k * 2, w);
    }
    0
}

/// Fill the read-ahead slot from the CMD->LTS pipe, executing LTS-mode commands (0x1f04) inline.
pub fn f_1c00f190(m: &mut Mem, p1: u32) -> u32 {
    let kernel = m.r32(p1 + 8);
    let lts = m.r32(p1 + 0x10);
    let pipe = m.r32(kernel + 0x118);
    let slot = lts + 0x90c;
    let finish = |m: &mut Mem, w: i32| {
        m.w16(slot, w);
        m.w16(lts + 0x90a, 1);
    };
    loop {
        m.pipe_read(pipe, slot, 1);
        let w = m.r16(slot) as u16 as u32;
        let cmd = w & 0x1fff;
        if cmd == 0x1f12 {
            f_1c00f700(m, kernel, slot, 1);
            continue;
        }
        let nargs = (w >> 13) & 3;
        if cmd == 0x1f04 {
            let mut args = [0u16; 4];
            let sp = m.sp();
            let tmp = m.alloca(2);
            let mut aborted = false;
            for k in 0..nargs as usize {
                m.pipe_read(pipe, tmp, 1);
                args[k] = m.r16(tmp) as u16;
                if m.r32(kernel + 0x174) != 0 {
                    if args[k] == 0x1f0a || args[k] == 0xb {
                        m.set_sp(sp);
                        finish(m, args[k] as i32);
                        return 0;
                    }
                    aborted = true;
                    break;
                }
            }
            m.set_sp(sp);
            if aborted || m.r32(kernel + 0x174) != 0 {
                continue;
            }
            let arg = args[1] as u32;
            match args[0] {
                0 => {
                    let v = m.r32(kernel + 0x200) | arg;
                    m.w32(kernel + 0x200, v);
                }
                1 => {
                    let v = m.r32(kernel + 0x200) & !arg;
                    m.w32(kernel + 0x200, v);
                }
                2 => m.w32(kernel + 0x200, arg),
                3 => {
                    let v = m.r32(kernel + 0x20c) | 2;
                    m.w32(kernel + 0x20c, v);
                }
                4 => {
                    let v = m.r32(kernel + 0x20c) | 1;
                    m.w32(kernel + 0x20c, v);
                }
                5 => {
                    let v = m.r32(kernel + 0x20c) | 4;
                    m.w32(kernel + 0x20c, v);
                }
                _ => {}
            }
            continue;
        }
        m.w16(lts + 0x90a, (nargs + 1) as i32);
        m.w16(slot, (w & 0x9fff) as i32);
        for k in 1..=nargs {
            let dst = slot + k * 2;
            m.pipe_read(pipe, dst, 1);
            if m.r32(kernel + 0x174) != 0 {
                let a = m.r16(dst) as u16;
                if a == 0x1f0a || a == 0xb {
                    finish(m, a as i32);
                    return 0;
                }
            }
        }
        if m.r32(kernel + 0x174) == 0 {
            return 0;
        }
        let w = m.r16(slot) as u16;
        if w == 0x1f0a || w == 0xb {
            return 0;
        }
    }
}

/// Forward the current command item (with its argument count restored) to PH.
pub fn f_1c00f480(m: &mut Mem, p1: u32) -> u32 {
    let kernel = m.r32(p1 + 8);
    let lts = m.r32(p1 + 0x10);
    let n = m.s(lts + 0x900);
    let cur = lts + 0x902;
    let w = m.s(cur) | n.wrapping_mul(0x2000).wrapping_add(0xe000);
    m.w16(cur, w);
    f_1c00f700(m, kernel, cur, n as u32);
    0
}

/// Write `p3` words at `p2` to the LTS->PH pipe (kernel+0x134), also appending phoneme names to
/// the API's phoneme-capture buffer when one is attached (kernel+0x484).
pub fn f_1c00f700(m: &mut Mem, p1: u32, p2: u32, p3: u32) -> u32 {
    let kernel = p1;
    let count = sx16(p3) as i32;
    let cap = m.r32(kernel + 0x484);
    if cap != 0 && m.r32(cap + 0xd8) != 0 && count > 0 {
        for k in 0..count as u32 {
            let w = m.r16(p2 + k * 2) as u16 as u32;
            let ph = w & 0xff;
            let pos = m.r32(cap + 0xe0);
            if (w >> 8) & 0xff == 0x1e && ph < 0x6c && (ph > 100 || ph < 0x39) && pos < m.r32(cap + 0xdc).wrapping_sub(4) {
                let names = m.r32(kernel + 0x464);
                let buf = m.r32(cap + 0xd8);
                let c0 = m.r8(names + ph * 2);
                m.w8(buf + pos, c0);
                m.w32(cap + 0xe0, pos + 1);
                let c1 = m.r8(names + ph * 2 + 1);
                m.w8(buf + pos + 1, c1);
                m.w32(cap + 0xe0, pos + 2);
            }
        }
    }
    let dbg = m.r16(kernel + 0x478) as u16;
    let w = m.r16(p2) as u16;
    if dbg & 0x4000 == 0 || dbg & 0x800 == 0 || matches!(w, 0x1f0a | 0x1f0b | 0x1f0c | 0x1f0d) {
        let pipe = m.r32(kernel + 0x134);
        m.pipe_write(pipe, p2, count as u32);
    }
    0
}

/// Copy a word (8-byte records) up to its terminator, then terminate the copy.
pub fn f_1c00ef10(m: &mut Mem, p1: u32, p2: u32) -> u32 {
    let (mut dst, mut src) = (p1, p2);
    let mut ch = m.s(src);
    if ch == 0 {
        m.w16(dst, 0);
        m.w16(dst + 4, 0);
        m.w16(dst + 6, 0);
        return 0;
    }
    let mut last;
    loop {
        last = dst;
        src += 8;
        m.w16(last, ch);
        ch = m.s(src);
        dst = last + 8;
        if ch == 0 {
            break;
        }
    }
    m.w16(last + 8, 0);
    m.w16(last + 12, 0);
    m.w16(last + 14, 0);
    0
}

/// Emit each byte of a NUL-terminated symbol string.
pub fn f_1c00ef50(m: &mut Mem, p1: u32, p2: u32) -> u32 {
    let mut p = p2;
    loop {
        let c = m.r8(p) as i8 as i32;
        if c == 0 {
            break;
        }
        p += 1;
        f_1c00ef80(m, p1, c as u32);
    }
    0
}

/// Emit one phoneme/boundary symbol (0x1eXX), preceded by a form-class word marker when the
/// word number changed since the last emission.
pub fn f_1c00ef80(m: &mut Mem, p1: u32, p2: u32) -> u32 {
    let lts = m.r32(p1 + 0x10);
    let kernel = m.r32(p1 + 8);
    m.w16(lts + 0x436, p2 as i32);
    let n = m.r32(lts + 0x8bc);
    let sp = m.sp();
    if n != m.r32(lts + 0x8c0) && n != 0 {
        m.w32(lts + 0x8c0, n);
        let fc = m.r32(lts + 0x4bc + n * 4);
        let buf = m.alloca(6);
        m.w16(buf, 0x5f17);
        m.w16(buf + 2, (fc >> 16) as i32);
        m.w16(buf + 4, fc as i32);
        f_1c00f700(m, kernel, buf, 3);
    }
    let buf = m.alloca(2);
    m.w16(buf, p2.wrapping_add(0x1e00) as i32);
    f_1c00f700(m, kernel, buf, 1);
    m.set_sp(sp);
    0
}

/// Is the current item a period?
pub fn f_1c00ed60(m: &mut Mem, p1: u32) -> u32 {
    (m.r16(p1 + 0x902) == 0x2e) as u32
}

/// Ordinal suffix check on a number token: 1st/2nd/3rd/Nth (11th-13th take "th").
pub fn f_1c00ee10(m: &mut Mem, _p1: u32, p2: u32) -> u32 {
    let start = m.r32(p2);
    if start == 0 || m.r32(p2 + 8) != 0 || m.r32(p2 + 0x10) != 0 {
        return 0;
    }
    let p = m.r32(p2 + 4);
    let mut d = m.s(p.wrapping_sub(8));
    if d == 0xbc || d == 0xbd {
        return 0;
    }
    if p > start.wrapping_add(8) && m.r16(p.wrapping_sub(0x10)) == 0x31 {
        d = 0x30;
    }
    let (a, b) = match d {
        0x31 => (0x73, 0x74),
        0x32 => (0x6e, 0x64),
        0x33 => (0x72, 0x64),
        _ => (0x74, 0x68),
    };
    (m.s(p) == a && m.s(p + 8) == b) as u32
}

/// Four-digit year-like number (all digits, not starting "0" or "x00").
pub fn f_1c00eeb0(m: &mut Mem, p1: u32, p2: u32) -> u32 {
    let mut n = 0;
    let mut p = p1;
    while p != p2 {
        if m.r8(CHAR_TYPE.wrapping_add(sx16(m.r16(p) as u32))) & 0x10 == 0 {
            return 0;
        }
        p += 8;
        n += 1;
    }
    let last = m.r16(p2.wrapping_sub(8));
    if last == 0xbc || last == 0xbd || n != 4 {
        return 0;
    }
    if m.r16(p1) == 0x30 || (m.r16(p1 + 8) == 0x30 && m.r16(p1 + 0x10) == 0x30) {
        return 0;
    }
    1
}

/// Emit the "s" plural/possessive allophone chosen by the previous phoneme's voicing features.
pub fn f_1c00f600(m: &mut Mem, p1: u32) -> u32 {
    let lts = m.r32(p1 + 0x10);
    let prev = m.s(lts + 0x436);
    let feat = if prev < 0x39 { m.r16((0x1c03_2e18i64 + prev as i64 * 2) as u32) as u8 } else { 0 };
    if feat & 0x11 == 0x11 {
        f_1c00ef80(m, p1, 0x12);
        f_1c00ef80(m, p1, 0x2a);
    } else if feat & 9 == 1 {
        f_1c00ef80(m, p1, 0x29);
    } else {
        f_1c00ef80(m, p1, 0x2a);
    }
    0
}

pub fn f_1c00f670(_m: &mut Mem, p1: u32) -> u32 {
    matches!(p1, 1 | 5 | 9 | 0xf | 0x15 | 0x19) as u32
}

/// Debug trace of a word to output.log (logging only).
pub fn f_1c00ec00(_m: &mut Mem, _p1: u32, _p2: u32) -> u32 {
    0
}

/// Debug trace of a word range to output.log (logging only).
pub fn f_1c00ec70(_m: &mut Mem, _p1: u32, _p2: u32, _p3: u32) -> u32 {
    0
}

/// Match the word against a length-prefixed string list; returns a pointer past the matched
/// string (and tags the word's form class for "the", "a"/"an", "for"...), or 0.
pub fn f_1c00c440(m: &mut Mem, p1: u32, p2: u32, p3: u32) -> u32 {
    let mut len = m.r8(p3) as u32;
    let mut entry = p3 + 1;
    if len == 0 {
        return 0;
    }
    let first = fold(m, m.r16(p2) as u16 as u32);
    loop {
        let mut w = p2;
        let mut c = first;
        let mut t = entry;
        let mut hit = false;
        loop {
            let tb = m.r8(t) as u32;
            t += 1;
            if c != tb {
                break;
            }
            if c == 0 {
                hit = true;
                break;
            }
            w += 8;
            c = fold(m, m.r16(w) as u16 as u32);
        }
        if hit {
            let idx = p1 + 0x4bc + m.r32(p1 + 0x8bc) * 4;
            if first == 0x74 {
                m.w32(idx, 0x81_1000);
            }
            if fold(m, m.r16(p2) as u16 as u32) == 0x61 {
                let idx = p1 + 0x4bc + m.r32(p1 + 0x8bc) * 4;
                m.w32(idx, 0x82_0040);
            }
            if fold(m, m.r16(p2) as u16 as u32) == 0x66 {
                let idx = p1 + 0x4bc + m.r32(p1 + 0x8bc) * 4;
                m.w32(idx, 0x8_1002);
            }
            return t;
        }
        entry += len;
        len = m.r8(entry) as u32;
        entry += 1;
        if len == 0 {
            return 0;
        }
    }
}

/// If the current item is an index command, return the end of the gathered word buffer.
pub fn f_1c00c510(m: &mut Mem, p1: u32) -> u32 {
    if f_1c00f0c0(m, p1 + 0x900) == 0 {
        return 0;
    }
    let mut p = p1 + 0x91c;
    while m.r16(p) != 0 {
        p += 8;
    }
    p
}

fn form_dump_enabled(m: &Mem, kernel: u32) -> bool {
    let dbg = m.r16(kernel + 0x478) as u16;
    m.r8(kernel + 0x204) & 8 != 0 || (dbg & 0x4000 != 0 && dbg & 0x100 != 0)
}

/// Handle the punctuation/white space ending a word: emit the boundary symbol and advance.
pub fn f_1c00c550(m: &mut Mem, p1: u32, p2: u32) -> u32 {
    let lts = m.r32(p1 + 0x10);
    let kernel = m.r32(p1 + 8);
    let w = m.r16(lts + 0x902) as u16 as u32;
    if w & 0x1f00 != 0 {
        return 0;
    }
    let ch = w & 0xff;
    if p2 == 2 && (ch == 0xd || ch == 10) {
        f_1c00ef80(m, p1, 0x74);
        m.w16(lts + 0x8c4, 0);
        if form_dump_enabled(m, kernel) {
            f_1c009fa0(m, p1);
        }
        return 0;
    }
    match ch {
        9 | 0x2c | 0x3a | 0x3b => {
            m.w32(lts + 0x8bc, 1);
            m.w32(lts + 0x8c0, 0xffff_ffff);
            m.w32(lts + 0x4bc, 0);
            m.w32(lts + 0x4c0, 0);
            f_1c00ef80(m, p1, 0x73);
            f_1c00f140(m, p1);
        }
        10 | 0xc | 0xd | 0x20 | 0xa0 => {
            let sym = if m.r8(kernel + 0x200) & 0x10 == 0 { 0x6f } else { 0x73 };
            f_1c00ef80(m, p1, sym);
            f_1c00f140(m, p1);
        }
        0xb => {
            if form_dump_enabled(m, kernel) {
                f_1c009fa0(m, p1);
            }
            m.w32(lts + 0x8bc, 0);
            m.w32(lts + 0x4bc, 0);
            m.w32(lts + 0x4c0, 0);
            m.w32(lts + 0x8c0, 0xffff_ffff);
            let sp = m.sp();
            let buf = m.alloca(2);
            m.w16(buf, 0x1f0b);
            f_1c00f700(m, kernel, buf, 1);
            m.set_sp(sp);
            f_1c00f140(m, p1);
        }
        0x21 => {
            f_1c00ef80(m, p1, 0x76);
            f_1c00f140(m, p1);
            if form_dump_enabled(m, kernel) {
                f_1c009fa0(m, p1);
            }
            m.w16(lts + 0x8c4, 0);
        }
        0x2e | 0x3f => {
            let sym = if ch == 0x2e || m.r16(lts + 0x8c4) == 1 { 0x74 } else { 0x75 };
            f_1c00ef80(m, p1, sym);
            if form_dump_enabled(m, kernel) {
                f_1c009fa0(m, p1);
            }
            f_1c00f140(m, p1);
            m.w16(lts + 0x8c4, 0);
        }
        _ => {}
    }
    0
}

/// Gather the next word into `p2` (8-byte records): skip leading white space and index
/// commands (forwarding them to PH), then collect letter/digit-class characters.
pub fn f_1c00c820(m: &mut Mem, p1: u32, p2: u32) -> u32 {
    let lts = m.r32(p1 + 0x10);
    let cur = lts + 0x900;
    while f_1c00f100(m, cur) != 0 {
        f_1c00f140(m, p1);
        while f_1c00f0c0(m, cur) != 0 {
            f_1c00f480(m, p1);
            f_1c00f140(m, p1);
        }
    }
    let mut seen = false;
    let mut out = p2;
    let mut high = p2;
    let mut ch: u32 = 0;
    loop {
        while f_1c00f0c0(m, cur) != 0 {
            if out == p2 {
                f_1c00f480(m, p1);
            }
            f_1c00f140(m, p1);
        }
        let w = m.r16(lts + 0x902) as u16 as u32;
        let class;
        if w & 0x1f00 != 0 {
            class = 5;
        } else {
            ch = w & 0xff;
            class = class_of(m, ch) & 0xf;
            if class == 0 {
                f_1c00f140(m, p1);
                continue;
            }
            if class == 1 {
                if out != p2 {
                    out -= 8;
                }
                f_1c00f140(m, p1);
                continue;
            }
        }
        if out < high {
            if class != 3 && class != 4 {
                break;
            }
            if class_of(m, ch) & 0x10 == 0 {
                m.w16(out, ch as i32);
            }
            out += 8;
            f_1c00f140(m, p1);
            continue;
        }
        if out == p2 {
            if class == 5 {
                f_1c00f480(m, p1);
                f_1c00f140(m, p1);
                seen = true;
                continue;
            }
            if class != 4 && (class != 3 || seen) {
                break;
            }
            m.w16(out, ch as i32);
            m.w32(out + 4, 0);
            out += 8;
            high += 8;
            f_1c00f140(m, p1);
            seen = true;
            continue;
        }
        if class != 4 && (class != 3 || f_1c00f070(m, p1) == 0) {
            break;
        }
        if out < p2 + 0x3f8 {
            m.w16(out, ch as i32);
            m.w32(out + 4, 0);
            out += 8;
            high += 8;
        }
        f_1c00f140(m, p1);
    }
    m.w16(out, 0);
    m.w32(out + 4, 0);
    0
}

/// Spell-out path end: tag the word, speak it via 1c009b00, then finish the word boundary.
pub fn f_1c00ca00(m: &mut Mem, p1: u32, p2: u32, p3: u32) -> u32 {
    let lts = m.r32(p1 + 0x10);
    let idx = lts + 0x4bc + m.r32(lts + 0x8bc) * 4;
    m.w32(idx, 0x400);
    f_1c009b00(m, p1, p2, p3);
    if m.r32(lts + 0x1120) == 0x74 {
        f_1c00ef80(m, p1, 0x74);
    }
    f_1c00ca50(m, p1);
    0
}

/// Word finished: emit the pending boundary symbol, skip trailing white space, handle the
/// terminating punctuation and gather the next word.
pub fn f_1c00ca50(m: &mut Mem, p1: u32) -> u32 {
    let lts = m.r32(p1 + 0x10);
    let pending = m.r32(lts + 0x1120);
    if pending != 0x6f {
        let cur = lts + 0x900;
        if f_1c00f100(m, cur) != 0 {
            f_1c00ef80(m, p1, pending);
            while f_1c00f100(m, cur) != 0 {
                f_1c00f140(m, p1);
                while f_1c00f0c0(m, cur) != 0 {
                    f_1c00f480(m, p1);
                    f_1c00f140(m, p1);
                }
            }
        }
    }
    f_1c00c550(m, p1, 0);
    f_1c00c820(m, p1, lts + 0x914);
    0
}

/// Form-class dump to the log files (logging only).
pub fn f_1c009fa0(_m: &mut Mem, _p1: u32) -> u32 {
    0
}

/// Whole-buffer match against the 6-entry table at 0x1c05e578 (32-byte records: name, symbol
/// list at +8, form class at +0x1c); on a hit emit the symbols and return 3.
pub fn f_1c00dfe0(m: &mut Mem, p1: u32) -> u32 {
    let lts = m.r32(p1 + 0x10);
    let buf = lts + 0x914;
    let mut row = 0u32;
    while row < 0xc0 {
        let tab = 0x1c05_e578 + row;
        let mut p = buf;
        let mut k = 0;
        loop {
            let ch = m.r16(p);
            if ch == 0 {
                break;
            }
            if m.r8(tab + k) as i8 as u32 != fold(m, ch as u16 as u32) {
                break;
            }
            p += 8;
            k += 1;
        }
        let t = m.r8(tab + k);
        if t as i8 as u32 == fold(m, m.r16(p) as u16 as u32) && t == 0 {
            m.w32(lts + 0x4c0, m.r32(0x1c05_e594 + row));
            let mut s = 0x1c05_e580 + row;
            loop {
                let sym = m.r32(s);
                if sym == 0 {
                    break;
                }
                f_1c00ef80(m, p1, sym);
                s += 4;
            }
            m.w32(lts + 0x111c, 0x6f);
            m.w32(lts + 0x1120, 0x6f);
            return 3;
        }
        row += 0x20;
    }
    0
}
