use super::*;

const CMD_TABLE: u32 = 0x1c02_e388;
const CMD_COUNT: u32 = 0x1c02_e6bc;
const CHAR_CLASS: u32 = 0x1c02_e000;
const CHAR_FOLD: u32 = 0x1c06_3d94;

fn is_ws(ch: u32) -> bool {
    ch == 0x20 || ch == 9 || ch == 0xd || ch == 0xa
}

fn class_of(m: &Mem, ch: u32) -> u8 {
    m.r8(CHAR_CLASS.wrapping_add(ch))
}

fn fold(m: &Mem, ch: u32) -> u32 {
    m.r8(CHAR_FOLD.wrapping_add(ch)) as u32
}

/// Current format-string character of the matched command (table entry +4, indexed by cmd+0x4f4).
fn fmt_char(m: &Mem, cmd: u32) -> u8 {
    let idx = m.r32(cmd + 0x2e8);
    let fmt = m.r32(CMD_TABLE.wrapping_add(idx.wrapping_mul(0x14)) + 4);
    m.r8(fmt.wrapping_add(m.r32(cmd + 0x4f4)))
}

/// `repe cmpsb` with ecx=5: the command name's first five bytes against `s`.
fn name_is(m: &Mem, idx: u32, s: u32) -> bool {
    let name = m.r32(CMD_TABLE.wrapping_add(idx.wrapping_mul(0x14)));
    (0..5).all(|k| m.r8(name.wrapping_add(k)) == m.r8(s + k))
}

/// Push a word-boundary space through the text path with cmd+0x51c temporarily 0xfff.
fn flush_space(m: &mut Mem, ctx: u32, cmd: u32, mark: bool) {
    let saved = m.r16(cmd + 0x51c) as u16 as i32;
    m.w16(cmd + 0x51c, 0xfff);
    if mark {
        m.w16(cmd + 0x3240, 1);
    }
    f_1c003850(m, ctx, 0x20);
    if mark {
        m.w16(cmd + 0x3240, 0);
    }
    m.w16(cmd + 0x51c, saved);
}

fn emit_word(m: &mut Mem, kernel: u32, pipe: u32, w: u32) -> u32 {
    let sp = m.sp();
    let buf = m.alloca(4);
    m.w32(buf, w);
    let r = f_1c005c20(m, kernel, pipe, buf, 1);
    m.set_sp(sp);
    r
}

/// Indirect call through the command table's handler slot (entry +0x10).
fn call_handler(m: &mut Mem, target: u32, ctx: u32) -> u32 {
    match target {
        0x1c00_1a70 => f_1c001a70(m, ctx),
        0x1c00_1b40 => f_1c001b40(m, ctx),
        0x1c00_1ed0 => f_1c001ed0(m, ctx),
        0x1c00_1fa0 => f_1c001fa0(m, ctx),
        0x1c00_2030 => f_1c002030(m, ctx),
        0x1c00_20b0 => f_1c0020b0(m, ctx),
        0x1c00_2200 => f_1c002200(m, ctx),
        0x1c00_2300 => f_1c002300(m, ctx),
        0x1c00_2390 => f_1c002390(m, ctx),
        0x1c00_2480 => f_1c002480(m, ctx),
        0x1c00_24b0 => f_1c0024b0(m, ctx),
        0x1c00_2570 => f_1c002570(m, ctx),
        0x1c00_2590 => f_1c002590(m, ctx),
        0x1c00_2890 => f_1c002890(m, ctx),
        0x1c00_2a00 => f_1c002a00(m, ctx),
        0x1c00_2aa0 => f_1c002aa0(m, ctx),
        0x1c00_2ae0 => f_1c002ae0(m, ctx),
        0x1c00_2b60 => f_1c002b60(m, ctx),
        0x1c00_2ba0 => f_1c002ba0(m, ctx),
        0x1c00_2be0 => f_1c002be0(m, ctx),
        0x1c00_2c20 => f_1c002c20(m, ctx),
        0x1c00_2e40 => f_1c002e40(m, ctx),
        0x1c00_3040 => f_1c003040(m, ctx),
        0x1c00_3070 => f_1c003070(m, ctx),
        0x1c00_3110 => f_1c003110(m, ctx),
        0x1c00_33c0 => f_1c0033c0(m, ctx),
        0x1c00_33f0 => f_1c0033f0(m, ctx),
        0x1c00_5d40 => f_1c005d40(m, ctx),
        _ => panic!("unported command handler {target:#010x}"),
    }
}

/// Parser state 2: match the command name one character at a time against the command table.
pub fn f_1c001000(m: &mut Mem, p1: u32, p2: u32) -> u32 {
    let ctx = p1;
    let ch = p2;
    let cmd = m.r32(ctx + 0xc);
    let matches = m.r32(cmd + 0x2e4);
    let last = m.r32(cmd + 0x2e8);
    m.w32(cmd + 0x280, 0);
    if is_ws(ch) {
        if matches == 1 {
            let idx = m.r32(cmd + 0x2e8);
            if !name_is(m, idx, 0x1c02_e104) {
                flush_space(m, ctx, cmd, true);
            }
            f_1c0039e0(m, cmd, 4);
            f_1c0012c0(m, ctx, ch);
            return 0;
        }
        if m.r32(cmd + 0x2d4) == 0 {
            return 0;
        }
        flush_space(m, ctx, cmd, true);
        f_1c0018f0(m, ctx, 3);
        f_1c0039e0(m, cmd, 5);
        return 0;
    }
    m.w32(cmd + 0x2d4, m.r32(cmd + 0x2d4).wrapping_add(1));
    if ch == 0x5d {
        flush_space(m, ctx, cmd, true);
        f_1c001230(m, ctx, 0);
        return 0;
    }
    let mut i: i32 = 0;
    while i < m.i(CMD_COUNT) {
        let slot = m.r32(cmd + 0x2e0).wrapping_add((i as u32).wrapping_mul(4));
        let pos = m.r32(slot);
        if pos != 0xffff {
            let name = m.r32(CMD_TABLE.wrapping_add((i as u32).wrapping_mul(0x14)));
            if m.r8(name.wrapping_add(pos)) == m.r8(CHAR_FOLD.wrapping_add(ch)) {
                m.w32(slot, pos.wrapping_add(1));
                m.w32(cmd + 0x2e8, i as u32);
            } else {
                m.w32(cmd + 0x2e4, m.r32(cmd + 0x2e4).wrapping_sub(1));
                m.w32(slot, 0xffff);
            }
        }
        i += 1;
    }
    if m.r32(cmd + 0x2e4) != 0 {
        return 0;
    }
    if matches != 1 {
        flush_space(m, ctx, cmd, true);
        f_1c0018f0(m, ctx, 3);
        f_1c0039e0(m, cmd, 5);
        return 0;
    }
    // The only surviving candidate rejected this char: it is the command, and ch starts its args.
    m.w32(cmd + 0x2e8, last);
    m.w32(cmd + 0x2e4, 1);
    if !name_is(m, last, 0x1c02_e10c) {
        flush_space(m, ctx, cmd, true);
    }
    f_1c0039e0(m, cmd, 4);
    f_1c0012c0(m, ctx, ch);
    0
}

/// Command complete: run the matched handler and pick the next parser state (p2 on success).
pub fn f_1c001230(m: &mut Mem, p1: u32, p2: u32) -> u32 {
    let ctx = p1;
    let cmd = m.r32(ctx + 0xc);
    let code;
    if m.r32(cmd + 0x2e4) == 1 {
        let idx = m.r32(cmd + 0x2e8);
        let handler = m.r32(0x1c02_e398u32.wrapping_add(idx.wrapping_mul(0x14)));
        let r = call_handler(m, handler, ctx);
        if r == 0 {
            return f_1c001870(m, cmd, p2);
        }
        if r == 0xb {
            let eax = f_1c001870(m, cmd, 0);
            let parse = m.r32(cmd + 0x518);
            m.w16(parse, 0);
            return eax;
        }
        code = r;
    } else {
        code = 3;
    }
    f_1c0018f0(m, ctx, code);
    if m.r16(cmd + 0x51c) != 0x5d {
        return f_1c001870(m, cmd, 5);
    }
    f_1c001870(m, cmd, 0)
}

/// Parser state 4: accumulate one argument character according to the command's format string.
pub fn f_1c0012c0(m: &mut Mem, p1: u32, p2: u32) -> u32 {
    let ctx = p1;
    let mut ch = p2;
    let cmd = m.r32(ctx + 0xc);
    let f = fmt_char(m, cmd);
    if f == 0 {
        return f_1c001230(m, ctx, 1);
    }
    if f == b'*' {
        if ch == 0x3a {
            return f_1c001230(m, ctx, 2);
        }
        if is_ws(ch) {
            return f_1c001230(m, ctx, 6);
        }
        if ch == 0x5d {
            return f_1c001230(m, ctx, 0);
        }
        m.w32(cmd + 0x4f4, 0);
    }
    if m.r32(cmd + 0x2d4) == 0 {
        let arg = m.r32(cmd + 0x2d0);
        m.w32(cmd.wrapping_add(arg.wrapping_mul(4)), 0);
        let arg = m.r32(cmd + 0x2d0);
        m.w32(cmd.wrapping_add(arg.wrapping_mul(4)) + 0x2a8, 1);
        let quote = m.r32(cmd + 0x2dc);
        if quote == 0 || fmt_char(m, cmd) != b'a' {
            match ch {
                9 | 0xa | 0xd | 0x20 => return 0,
                0x2c => {
                    m.w32(cmd + 0x2d0, m.r32(cmd + 0x2d0).wrapping_add(1));
                    m.w32(cmd + 0x4f4, m.r32(cmd + 0x4f4).wrapping_add(1));
                    return 0;
                }
                0x2e => return f_1c001230(m, ctx, 1),
                0x3a => return f_1c001230(m, ctx, 2),
                0x5d => return f_1c001230(m, ctx, 0),
                _ => {}
            }
        }
        if quote == 0 {
            if ch == 0x22 {
                m.w32(cmd + 0x2dc, 0x22);
                return 0;
            }
            if ch == 0x3c {
                m.w32(cmd + 0x2dc, 0x3e);
                return 0;
            }
        }
    }
    m.w32(cmd + 0x2d4, m.r32(cmd + 0x2d4).wrapping_add(1));
    let arg = m.r32(cmd + 0x2d0);
    m.w32(cmd.wrapping_add(arg.wrapping_mul(4)) + 0x2a8, 0);
    let slot = |m: &Mem| cmd.wrapping_add(m.r32(cmd + 0x2d0).wrapping_mul(4));
    match fmt_char(m, cmd) {
        b'b' => {
            if ch == 0x30 || ch == 0x31 {
                let s = slot(m);
                m.w32(s, ch.wrapping_add(m.r32(s).wrapping_mul(2)).wrapping_sub(0x30));
                return 0;
            }
        }
        b'o' => {
            if (0x30..=0x37).contains(&ch) {
                let s = slot(m);
                m.w32(s, ch.wrapping_add(m.r32(s).wrapping_mul(8)).wrapping_sub(0x30));
                return 0;
            }
        }
        b'd' => {
            if m.r32(cmd + 0x2d4) == 1 && ch == 0x2d {
                m.w32(cmd + 0x2d8, 1);
                return 0;
            }
            if (0x30..=0x39).contains(&ch) {
                let s = slot(m);
                m.w32(s, ch.wrapping_add(m.r32(s).wrapping_mul(10)).wrapping_sub(0x30));
                return 0;
            }
        }
        b'h' => {
            if (0x30..=0x39).contains(&ch) {
                let s = slot(m);
                m.w32(s, (m.r32(s).wrapping_add(0x0fff_fffd) << 4).wrapping_add(ch));
                return 0;
            }
            ch = fold(m, ch);
            if (0x61..=0x66).contains(&ch) {
                let s = slot(m);
                m.w32(s, (m.r32(s) << 4).wrapping_add(ch).wrapping_sub(0x57));
                return 0;
            }
        }
        b'a' => {
            if m.r32(cmd + 0x2d4) == 1 {
                let arg = m.r32(cmd + 0x2d0);
                let at = m.r32(cmd + 0x4f0).wrapping_add(cmd).wrapping_add(0x2f0);
                m.w32(cmd.wrapping_add(arg.wrapping_mul(4)) + 0x280, at);
                let off = m.r32(cmd + 0x4f0) & 0x1ff;
                m.w8(cmd + 0x2f0 + off, 0);
            }
            let quote = m.r32(cmd + 0x2dc);
            let append = if quote != 0 {
                if ch == quote {
                    m.w32(cmd + 0x2dc, 0);
                    return 0;
                }
                true
            } else {
                !(ch == 0x20 || ch == 9 || ch == 0x5d || ch == 0x2c)
            };
            if append {
                let off = m.r32(cmd + 0x4f0) & 0x1ff;
                m.w8(cmd + 0x2f0 + off, ch as u8);
                let next = m.r32(cmd + 0x4f0).wrapping_add(1);
                m.w32(cmd + 0x4f0, next);
                m.w8(cmd + 0x2f0 + (next & 0x1ff), 0);
                return 0;
            }
            if ch != 0x5d {
                ch = 0x20;
            }
            m.w32(cmd + 0x4f0, m.r32(cmd + 0x4f0).wrapping_add(1));
        }
        _ => {}
    }
    if m.r32(cmd + 0x2d8) == 1 {
        let s = slot(m);
        m.w32(s, m.r32(s).wrapping_neg());
    }
    let quote = m.r32(cmd + 0x2dc);
    if quote != 0 && ch == quote {
        if m.r32(cmd + 0x2d4) != 1 {
            m.w32(cmd + 0x2d0, m.r32(cmd + 0x2d0).wrapping_add(1));
        }
        return f_1c001230(m, ctx, 1);
    }
    if m.r32(cmd + 0x2d4) != 1 {
        m.w32(cmd + 0x4f4, m.r32(cmd + 0x4f4).wrapping_add(1));
        m.w32(cmd + 0x2d0, m.r32(cmd + 0x2d0).wrapping_add(1));
    }
    m.w32(cmd + 0x2d4, 0);
    m.w32(cmd + 0x2d8, 0);
    let idx = m.r32(cmd + 0x2e8);
    match ch {
        9 | 0xa | 0xd | 0x20 => {
            let f = fmt_char(m, cmd);
            if f == 0 {
                return f_1c001230(m, ctx, 1);
            }
            if f == b'*' {
                return f_1c001230(m, ctx, 6);
            }
            return 0;
        }
        0x2c => {
            let f = fmt_char(m, cmd);
            if f != 0 && f != b'*' {
                return 0;
            }
            if name_is(m, idx, 0x1c02_e114) {
                flush_space(m, ctx, cmd, false);
            }
        }
        0x5d => return f_1c001230(m, ctx, 0),
        _ => {
            if name_is(m, idx, 0x1c02_e11c) {
                flush_space(m, ctx, cmd, false);
            }
        }
    }
    f_1c0018f0(m, ctx, 4);
    f_1c0039e0(m, cmd, 5);
    0
}

/// Report command error `p2` according to the error mode (cmd+0x4fc): log, speak, or tone.
pub fn f_1c0018f0(m: &mut Mem, p1: u32, p2: u32) -> u32 {
    let ctx = p1;
    let kernel = m.r32(ctx + 8);
    let cmd = m.r32(ctx + 0xc);
    let mut msg = m.r32(m.r32(kernel + 0x474).wrapping_add(p2.wrapping_mul(4)));
    match m.r32(cmd + 0x4fc) {
        0 => {}
        1 => {
            if m.r32(kernel + 0x204) & 1 == 0 {
                f_1c001d10(m, ctx);
                let v = if m.r32(ctx + 0x74) & 0x8000_0000 == 0 { 0 } else { 4 };
                m.w32(ctx + 0x84, v);
            }
            // fprintf(ctx+0x98, "[:error %s]") to the log: omitted, assumed to succeed.
            return 0;
        }
        3 => {
            f_1c002200(m, ctx);
            loop {
                let b = m.r8(msg);
                if b == 0 {
                    break;
                }
                msg += 1;
                let pipe = m.r32(kernel + 0xd0);
                emit_word(m, kernel, pipe, sx8(b as u32) & 0xffff);
            }
            let pipe = m.r32(kernel + 0xd0);
            emit_word(m, kernel, pipe, 0xb);
        }
        4 => {
            f_1c002200(m, ctx);
            f_1c005a80(m, ctx, 0, 0x2b9, 100);
            f_1c005b00(m);
        }
        _ => return 0,
    }
    if m.r32(kernel + 0x204) & 1 == 0 && m.r32(ctx + 0x84) == 3 {
        f_1c001db0(m, ctx);
    }
    0
}

/// Open the command log (log.txt) unless already open; 1 if the log is open afterwards.
pub fn f_1c001d10(m: &mut Mem, p1: u32) -> u32 {
    let ctx = p1;
    let state = m.r32(ctx + 0x84);
    if state != 3 {
        if state != 0 && state != 4 {
            return 0;
        }
        // fopen("log.txt") -> ctx+0x98 omitted; assumed to succeed.
        f_1c002200(m, ctx);
        m.w32(ctx + 0x84, 3);
    }
    1
}

/// Sync, then close the command log if open.
pub fn f_1c001db0(m: &mut Mem, p1: u32) -> u32 {
    let ctx = p1;
    let kernel = m.r32(ctx + 8);
    f_1c002200(m, ctx);
    if m.r32(ctx + 0x84) != 3 {
        return 0;
    }
    m.w32(ctx + 0x84, (m.r32(ctx + 0x74) >> 29) & 4);
    // fclose(ctx+0x98) omitted; assumed to succeed.
    m.w32(kernel + 0x204, 0);
    0
}

/// [:sync]: send a sync marker downstream and wait for it; 0xb if the kernel is halting.
pub fn f_1c002200(m: &mut Mem, p1: u32) -> u32 {
    let ctx = p1;
    let kernel = m.r32(ctx + 8);
    if m.r32(kernel + 0x174) == 0 {
        // ResetEvent(ctx+0x38) omitted.
        m.w32(kernel + 0x1e0, 0);
        let pipe = m.r32(kernel + 0xd0);
        emit_word(m, kernel, pipe, 0xb);
        let pipe = m.r32(kernel + 0xd0);
        emit_word(m, kernel, pipe, 0x1f0a);
        // WaitForSingleObject(ctx+0x38, INFINITE) for the downstream sync ack omitted.
    }
    if m.r32(kernel + 0x16c) != 0 { 0xb } else { 0 }
}

/// Parser state 0: route one text character according to the kernel text mode (+0x208).
pub fn f_1c003850(m: &mut Mem, p1: u32, p2: u32) -> u32 {
    let ctx = p1;
    let kernel = m.r32(ctx + 8);
    let cmd = m.r32(ctx + 0xc);
    if m.r32(kernel + 0x170) != 0 || m.r32(kernel + 0x16c) == 1 {
        f_1c005980(m, ctx);
        return 0;
    }
    let mut ch = p2;
    match m.r32(kernel + 0x208) {
        0 => {
            f_1c004470(m, ctx);
        }
        1 => {
            if class_of(m, p2 & 0xffff) & 0x80 != 0 {
                ch = 0xb;
                m.w16(cmd + 0x51c, 0xb);
            }
            f_1c004470(m, ctx);
        }
        2 => {
            if m.r16(cmd + 0x3240) == 0 {
                f_1c005b10(m, kernel, p2 & 0xffff);
            }
        }
        4 => {
            if p2 as u16 == 0xd || p2 as u16 == 0xa {
                ch = 0xb;
                m.w16(cmd + 0x51c, 0xb);
            }
            f_1c004470(m, ctx);
        }
        8 => {
            let pipe = m.r32(kernel + 0xd0);
            emit_word(m, kernel, pipe, p2);
        }
        0x10 => {
            if m.r16(cmd + 0x3240) == 0 && p2 as u16 >= 0x20 {
                f_1c005b10(m, kernel, p2 & 0xffff);
            }
        }
        _ => {}
    }
    if ch as u16 == 0xb {
        let pipe = m.r32(kernel + 0xd0);
        emit_word(m, kernel, pipe, 0x1f0a);
    }
    0
}

/// Parser state 3: escape-sequence (`[:` mode) character handling.
pub fn f_1c003ed0(m: &mut Mem, p1: u32, p2: u32) -> u32 {
    let ctx = p1;
    let ch = p2;
    let kernel = m.r32(ctx + 8);
    let cmd = m.r32(ctx + 0xc);
    if ch == 0xd || ch == 0xa || m.r32(kernel + 0x170) != 0 {
        return 0;
    }
    if m.r32(cmd + 0x2d0) != 0 && f_1c0041c0(m, ctx, ch) != 0 {
        return 0;
    }
    let pending = m.r32(cmd + 0x2dc);
    if pending == 0 {
        if ch == 0x3a {
            f_1c001870(m, cmd, 2);
            return 0;
        }
        if ch == 0x5d {
            f_1c001870(m, cmd, 0);
            return 0;
        }
        if m.r8(kernel + 0x210) & 2 == 0 {
            m.w32(cmd + 0x2dc, ch);
            return 0;
        }
        if f_1c004080(m, ctx, ch) != 0 {
            return 0;
        }
        f_1c0018f0(m, ctx, 5);
        f_1c0039e0(m, cmd, 5);
        return 0;
    }
    if ch == 0x3a {
        if f_1c0040e0(m, ctx, pending, 0x20) == 2 {
            f_1c004310(m, ctx);
            f_1c001870(m, cmd, 2);
            return 0;
        }
        f_1c0018f0(m, ctx, 5);
        f_1c0039e0(m, cmd, 5);
        return 0;
    }
    if ch == 0x5d {
        if pending != 0x20 {
            let r = f_1c0040e0(m, ctx, pending, 0x20) as i32;
            if r == 0 {
                f_1c0018f0(m, ctx, 5);
                f_1c0039e0(m, cmd, 0);
            } else if r > 0 && r <= 2 {
                f_1c004310(m, ctx);
                f_1c001870(m, cmd, 0);
                return 0;
            }
        }
        f_1c001870(m, cmd, 0);
        return 0;
    }
    match f_1c0040e0(m, ctx, pending, ch) {
        0 => {
            f_1c0018f0(m, ctx, 5);
            f_1c0039e0(m, cmd, 5);
        }
        1 => {
            let v = if f_1c0041c0(m, ctx, ch) != 0 { 0 } else { ch };
            m.w32(cmd + 0x2dc, v);
        }
        2 => m.w32(cmd + 0x2dc, 0),
        _ => {}
    }
    if m.r32(cmd + 0x2dc) != 0 {
        f_1c004310(m, ctx);
    }
    0
}

/// Look up a single-character escape code (kernel+0x458/+0x45c); store its index as the next arg.
pub fn f_1c004080(m: &mut Mem, p1: u32, p2: u32) -> u32 {
    let ctx = p1;
    let kernel = m.r32(ctx + 8);
    let cmd = m.r32(ctx + 0xc);
    let table = m.r32(kernel + 0x458);
    let n = m.i(kernel + 0x45c);
    let mut i: i32 = 0;
    while i < n {
        if p2 == m.r8(table.wrapping_add(i as u32)) as u32 {
            let arg = m.r32(cmd + 0x2d0);
            m.w32(cmd.wrapping_add(arg.wrapping_mul(4)), i as u32);
            m.w32(cmd + 0x2d0, m.r32(cmd + 0x2d0).wrapping_add(1));
            return 1;
        }
        i += 1;
    }
    0
}

/// Look up a two-character escape code pair (kernel+0x464/+0x468): 2 = exact, 1 = "c " prefix.
pub fn f_1c0040e0(m: &mut Mem, p1: u32, p2: u32, p3: u32) -> u32 {
    let ctx = p1;
    let kernel = m.r32(ctx + 8);
    let cmd = m.r32(ctx + 0xc);
    let n = m.i(kernel + 0x468);
    let table = m.r32(kernel + 0x464);
    let (c1, c2) = if m.r32(kernel + 0x46c) == 0 { (fold(m, p2), fold(m, p3)) } else { (p2, p3) };
    let mut i: i32 = 0;
    while i < n {
        let at = table.wrapping_add(i as u32);
        if c1 == m.r8(at) as u32 && c2 == m.r8(at + 1) as u32 {
            let arg = m.r32(cmd + 0x2d0);
            m.w32(cmd.wrapping_add(arg.wrapping_mul(4)), (i / 2) as u32);
            m.w32(cmd + 0x2d0, m.r32(cmd + 0x2d0).wrapping_add(1));
            return 2;
        }
        i += 2;
    }
    let mut i: i32 = 0;
    while i < n {
        let at = table.wrapping_add(i as u32);
        if c1 == m.r8(at) as u32 && m.r8(at + 1) == 0x20 {
            let arg = m.r32(cmd + 0x2d0);
            m.w32(cmd.wrapping_add(arg.wrapping_mul(4)), (i / 2) as u32);
            m.w32(cmd + 0x2d0, m.r32(cmd + 0x2d0).wrapping_add(1));
            return 1;
        }
        i += 2;
    }
    0
}

/// Escape-sequence numeric parameters (`<n,n,n>`); 1 if the character was consumed.
pub fn f_1c0041c0(m: &mut Mem, p1: u32, p2: u32) -> u32 {
    let ctx = p1;
    let ch = p2;
    let cmd = m.r32(ctx + 0xc);
    let nargs = m.r32(cmd + 0x2d0);
    let cur = cmd.wrapping_add(nargs.wrapping_mul(4)).wrapping_sub(4);
    if nargs == 1 {
        if ch == 0x5d {
            f_1c004310(m, ctx);
            f_1c001870(m, cmd, 0);
            return 1;
        }
        if ch != 0x3c {
            f_1c004310(m, ctx);
            return 0;
        }
        m.w32(cmd + 4, 0);
        m.w32(cmd + 0x2d4, 0);
        m.w32(cmd + 0x2d8, 0);
        m.w32(cmd + 0x2d0, m.r32(cmd + 0x2d0).wrapping_add(1));
        return 1;
    }
    if ch == 0x5d {
        f_1c001870(m, cmd, 0);
        return 1;
    }
    let count = m.r32(cmd + 0x2d4).wrapping_add(1);
    m.w32(cmd + 0x2d4, count);
    match ch {
        0x2c => {
            if m.r32(cmd + 0x2d8) != 0 {
                m.w32(cur, m.r32(cur).wrapping_neg());
            }
            let n = m.r32(cmd + 0x2d0);
            if n != 3 {
                m.w32(cmd.wrapping_add(n.wrapping_mul(4)), 0);
                m.w32(cmd + 0x2d4, 0);
                m.w32(cmd + 0x2d8, 0);
                m.w32(cmd + 0x2d0, m.r32(cmd + 0x2d0).wrapping_add(1));
                return 1;
            }
        }
        0x2d => {
            if count == 1 {
                m.w32(cmd + 0x2d8, 1);
                return 1;
            }
        }
        0x3e => {
            if m.r32(cmd + 0x2d8) != 0 {
                m.w32(cur, m.r32(cur).wrapping_neg());
            }
            f_1c004310(m, ctx);
            return 1;
        }
        _ => {
            if (0x30..=0x39).contains(&ch) {
                m.w32(cur, ch.wrapping_add(m.r32(cur).wrapping_mul(10)).wrapping_sub(0x30));
                return 1;
            }
        }
    }
    f_1c0018f0(m, ctx, 5);
    f_1c0039e0(m, cmd, 5);
    1
}

/// Emit the collected escape sequence (header word + args) to the CMD->LTS pipe, then reset.
pub fn f_1c004310(m: &mut Mem, p1: u32) -> u32 {
    let ctx = p1;
    let cmd = m.r32(ctx + 0xc);
    let kernel = m.r32(ctx + 8);
    let n = m.r32(cmd + 0x2d0);
    if n != 0 && m.r8(kernel + 0x210) & 4 != 0 {
        let hdr = (n.wrapping_add(0x7ffff) << 13) | 0x1e00;
        m.w32(cmd, m.r32(cmd) | hdr);
        let sp = m.sp();
        let buf = m.alloca(0x14.max(n.wrapping_mul(2)));
        let mut k = 0u32;
        while k < m.r32(cmd + 0x2d0) {
            m.w16(buf + k * 2, m.r32(cmd + k * 4) as i32);
            k += 1;
        }
        let pipe = m.r32(kernel + 0xd0);
        let count = m.r32(cmd + 0x2d0) & 0xffff;
        f_1c005c20(m, kernel, pipe, buf, count);
        m.set_sp(sp);
    }
    m.w32(cmd + 0x2d0, 0);
    m.w32(cmd + 0x2d8, 0);
    m.w32(cmd + 0x2d4, 0);
    0
}

/// Copy a token from p1 to p2 (skipping leading 0x80-class chars and 0x82 bytes); returns p2.
pub fn f_1c0043b0(m: &mut Mem, p1: u32, p2: u32, p3: u32) -> u32 {
    let src = p1;
    let dst = p2;
    let mut i = 0u32;
    let mut n = 0u32;
    while class_of(m, m.r8(src + i) as u32) & 0x80 != 0 {
        i += 1;
    }
    if p3 == 0 {
        loop {
            let c = m.r8(src + i);
            let cls = class_of(m, c as u32);
            let take = if cls & 0xc0 == 0 && c != 0 {
                true
            } else if c == 0x2d || c == 0x2e {
                true
            } else if cls & 0x20 == 0 {
                false
            } else {
                let next = m.r8(src + i + 1);
                next != 0 && class_of(m, next as u32) & 0xc0 == 0
            };
            if !take {
                break;
            }
            if c != 0x82 {
                m.w8(dst + n, c);
                n += 1;
            }
            i += 1;
        }
    } else {
        let mut p = src + i;
        loop {
            let c = m.r8(p);
            let take = (class_of(m, c as u32) & 0x80 == 0 && c != 0) || c == 0x2d;
            if !take {
                break;
            }
            if c != 0x82 {
                m.w8(dst + n, c);
                n += 1;
            }
            p += 1;
        }
    }
    m.w8(dst + n, 0);
    dst
}

/// Reset the text-normalizer buffers in the cmd state.
pub fn f_1c005980(m: &mut Mem, p1: u32) -> u32 {
    let cmd = m.r32(p1 + 0xc);
    let zero = |m: &mut Mem, at: u32, dwords: u32| {
        for k in 0..dwords {
            m.w32(at + k * 4, 0);
        }
    };
    m.w32(cmd + 0x2ec, 0);
    m.w16(cmd + 0x3222, 0);
    m.w16(cmd + 0x52c, 0);
    m.w16(cmd + 0x52e, 0);
    m.w16(cmd + 0x52a, 0);
    m.w16(cmd + 0x528, 0);
    zero(m, cmd + 0x530, 0x7d);
    zero(m, cmd + 0x724, 0xc);
    m.w16(cmd + 0x754, 0);
    zero(m, cmd + 0xb0c, 0x7d);
    zero(m, cmd + 0x918, 0x7d);
    zero(m, cmd + 0xd00, 0x7d);
    zero(m, cmd + 0xef4, 0x2ee);
    zero(m, cmd + 0x1aac, 0x2ee);
    zero(m, cmd + 0x2664, 0x2ee);
    m.w16(cmd + 0x3220, 0);
    m.w32(cmd + 0x321c, cmd + 0x531);
    m.w8(cmd + 0x530, 0x20);
    zero(m, cmd + 0x3224, 6);
    m.w16(cmd + 0x3224, 0);
    0
}

/// Tone: sync, wait for downstream drain, then send a kind-2 message [2, p4, p3, 0x7fff, 1000, 0]
/// to the VTM pipe. p2 is unused. Returns 0xb if the sync reported a halt.
pub fn f_1c005a80(m: &mut Mem, p1: u32, p2: u32, p3: u32, p4: u32) -> u32 {
    let _ = p2;
    let ctx = p1;
    let kernel = m.r32(ctx + 8);
    if f_1c002200(m, ctx) == 0xb {
        return 0xb;
    }
    f_1c0282a0(m, ctx, 0xffff_ffff);
    let sp = m.sp();
    let msg = m.alloca(12);
    m.w16(msg, 2);
    m.w16(msg + 2, p4 as i32);
    m.w16(msg + 4, p3 as i32);
    m.w16(msg + 6, 0x7fff);
    m.w16(msg + 8, 1000);
    m.w16(msg + 10, 0);
    let pipe = m.r32(kernel + 0xdc);
    f_1c005c20(m, kernel, pipe, msg, 6);
    m.set_sp(sp);
    0
}

/// No-op stub (xor eax, eax; ret).
pub fn f_1c005b00(m: &mut Mem) -> u32 {
    let _ = m;
    0
}

/// Spell a character: 0x1f03, then the word for each byte of its spelling string
/// (kernel+0x470[p2], mapped through kernel+0x460), then 0x1e73, on the kernel+0xd4 pipe.
pub fn f_1c005b10(m: &mut Mem, p1: u32, p2: u32) -> u32 {
    let kernel = p1;
    let pipe = m.r32(kernel + 0xd4);
    emit_word(m, kernel, pipe, 0x1f03);
    let mut s = m.r32(m.r32(kernel + 0x470).wrapping_add(p2.wrapping_mul(4)));
    loop {
        let b = m.r8(s);
        if b == 0 {
            break;
        }
        let w = m.r16(m.r32(kernel + 0x460).wrapping_add(b as u32 * 4)) as u16 as u32;
        let pipe = m.r32(kernel + 0xd4);
        emit_word(m, kernel, pipe, w);
        s += 1;
    }
    let pipe = m.r32(kernel + 0xd4);
    emit_word(m, kernel, pipe, 0x1e73);
    0
}
