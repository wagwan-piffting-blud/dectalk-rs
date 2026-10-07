//! `[:command]` handlers (command table 0x1c02e388, handler slot +0x10) and their helpers.
//! Parsed arguments: numeric arg i at cmd+i*4, string arg i pointer at cmd+0x280+i*4,
//! "arg i absent" flag at cmd+0x2a8+i*4, arg count at cmd+0x2d0.
//!
//! There is no audio device in this port: wave-out and wave-file calls are the `*_noop`
//! helpers at the bottom of this file.

use super::*;

const CMD_TABLE_CODE: u32 = 0x1c02_e394;
const CHAR_FOLD: u32 = 0x1c06_3d94;
const VOLUME_STEPS: u32 = 0x1c06_c9c0;
const DTMF_COL: u32 = 0x1c02_eff4;
const DTMF_ROW: u32 = 0x1c02_f014;
const MULAW_TO_PCM: u32 = 0x1c02_f048;
/// Stand-in for the FILE* fopen("dbglog.txt", "w") would store at kernel+0x47c.
const DBGLOG_FILE: u32 = 1;

fn str_arg(m: &Mem, cmd: u32, i: u32) -> u32 {
    m.r32(cmd.wrapping_add(i.wrapping_mul(4)) + 0x280)
}

/// Send `[hdr, (u16)cmd[0]]` to the CMD->LTS pipe (kernel+0xd0).
fn send_arg0(m: &mut Mem, ctx: u32, hdr: i32) -> u32 {
    let kernel = m.r32(ctx + 8);
    let cmd = m.r32(ctx + 0xc);
    let sp = m.sp();
    let msg = m.alloca(4);
    m.w16(msg, hdr);
    m.w16(msg + 2, m.s(cmd));
    let pipe = m.r32(kernel + 0xd0);
    f_1c005c20(m, kernel, pipe, msg, 2);
    m.set_sp(sp);
    0
}

/// Push a word-boundary space through the text path with cmd+0x51c temporarily `mark`.
fn flush_space(m: &mut Mem, ctx: u32, cmd: u32, mark: i32) {
    let saved = m.r16(cmd + 0x51c) as i32;
    m.w16(cmd + 0x51c, mark);
    f_1c003850(m, ctx, 0x20);
    m.w16(cmd + 0x51c, saved);
}

/// [:rate n]: `[0x3f00, n]` to LTS.
pub fn f_1c002aa0(m: &mut Mem, p1: u32) -> u32 {
    send_arg0(m, p1, 0x3f00)
}

/// [:latin n]: `[0x3f0f, n]` to LTS.
pub fn f_1c002b60(m: &mut Mem, p1: u32) -> u32 {
    send_arg0(m, p1, 0x3f0f)
}

/// [:name x] / [:np] .. [:nv]: `[0x3f05, voice]` to LTS; the voice comes from the table entry's
/// code (0x8000|voice) or, for [:name] (code 0xc9), from the voice-name keyword.
pub fn f_1c002ae0(m: &mut Mem, p1: u32) -> u32 {
    let ctx = p1;
    let cmd = m.r32(ctx + 0xc);
    let kernel = m.r32(ctx + 8);
    let idx = m.r32(cmd + 0x2e8);
    let code = m.r32(CMD_TABLE_CODE.wrapping_add(idx.wrapping_mul(0x14)));
    let voice = if code == 0xc9 {
        let s = str_arg(m, cmd, 0);
        f_1c005ba0(m, 0x1c02_e2a0, s) as u16
    } else {
        (code & 0x7fff) as u16
    };
    if voice >= 10 {
        return 1;
    }
    let sp = m.sp();
    let msg = m.alloca(4);
    m.w16(msg, 0x3f05);
    m.w16(msg + 2, voice as i32);
    let pipe = m.r32(kernel + 0xd0);
    f_1c005c20(m, kernel, pipe, msg, 2);
    m.set_sp(sp);
    0
}

/// [:comma n] / [:cp n]: `[0x3f01, n]` to LTS.
pub fn f_1c002ba0(m: &mut Mem, p1: u32) -> u32 {
    send_arg0(m, p1, 0x3f01)
}

/// [:period n] / [:pp n]: `[0x3f02, n]` to LTS.
pub fn f_1c002be0(m: &mut Mem, p1: u32) -> u32 {
    send_arg0(m, p1, 0x3f02)
}

/// [:volume op n [n]]: sync, then set/up/down the left, right or both channel volumes.
pub fn f_1c002590(m: &mut Mem, p1: u32) -> u32 {
    let ctx = p1;
    let cmd = m.r32(ctx + 0xc);
    let s = str_arg(m, cmd, 0);
    let op = f_1c005ba0(m, 0x1c02_e1fc, s);
    if op == 0xffff {
        return 1;
    }
    let amount = m.r32(cmd + 4);
    let r = f_1c002200(m, ctx);
    if r == 0xb {
        return r;
    }
    // (mode, left, right): set/up/down x both/left/right.
    let (mode, left, right) = match op {
        0 => (0, 1, 1),
        1 => (1, 1, 1),
        2 => (2, 1, 1),
        3 => (0, 1, 0),
        4 => (1, 1, 0),
        5 => (2, 1, 0),
        6 => (0, 0, 1),
        7 => (1, 0, 1),
        8 => (2, 0, 1),
        9 => {
            let r = m.r32(cmd + 8);
            f_1c025170(m, ctx, amount, r);
            return 0;
        }
        _ => return 2,
    };
    f_1c025090(m, ctx, amount, mode, left, right);
    0
}

/// [:vs n]: set both channel volumes.
pub fn f_1c002570(m: &mut Mem, p1: u32) -> u32 {
    let cmd = m.r32(p1 + 0xc);
    let v = m.r32(cmd);
    f_1c025090(m, p1, v, 0, 1, 1);
    0
}

/// [:index type n [n]]: queue a 6-byte index mark at the current text position (cmd+0xef4 +
/// n*6, 0x82 placeholder in the text buffer), or send it straight to LTS in punctuation
/// "pass" / skip "all" modes. `query` only flushes; `pause` is a mark of type 2.
pub fn f_1c002c20(m: &mut Mem, p1: u32) -> u32 {
    let ctx = p1;
    let cmd = m.r32(ctx + 0xc);
    let kernel = m.r32(ctx + 8);
    let s = str_arg(m, cmd, 0);
    let k = f_1c005ba0(m, 0x1c02_e2cc, s);
    let sp = m.sp();
    let msg = m.alloca(8);
    // movsx of the 16-bit result against 0xffff: never true, the switch range check catches it.
    if sx16(k) != 0xffff {
        let t = if k as u16 == 3 {
            m.w16(msg + 4, 2);
            0x14
        } else {
            m.w16(msg + 4, 0);
            k.wrapping_add(0x14)
        };
        let arg1 = m.s(cmd + 4);
        let case = sx16(t).wrapping_sub(0x14);
        let hdr = match case {
            0 => Some(0x5f08),
            1 => Some(0x5f09),
            2 => {
                flush_space(m, ctx, cmd, 0xfff);
                f_1c024e60(m);
                m.set_sp(sp);
                return 0;
            }
            4..=7 => {
                m.w16(msg + 4, m.s(cmd + 8));
                Some([0x5f13, 0x5f14, 0x5f15, 0x5f16][(case - 4) as usize])
            }
            _ => None,
        };
        if let Some(hdr) = hdr {
            m.w16(msg, hdr);
            m.w16(msg + 2, arg1);
            if m.s(cmd + 0x528) >= 0x12c {
                let saved = m.r16(cmd + 0x51c) as i32;
                m.w16(cmd + 0x51c, 0x20);
                f_1c003850(m, ctx, 0x20);
                m.w16(cmd + 0x51c, 0x20);
                f_1c003850(m, ctx, 0x20);
                m.w16(cmd + 0x51c, saved);
            }
            if m.r32(cmd + 0x500) == 3 || m.r32(cmd + 0x504) == 4 {
                let pipe = m.r32(kernel + 0xd0);
                f_1c005c20(m, kernel, pipe, msg, 3);
            } else {
                let n = m.s(cmd + 0x528);
                m.w8(cmd.wrapping_add(n as u32) + 0x530, 0x82);
                let at = cmd.wrapping_add((n + 0x27e).wrapping_mul(6) as u32);
                m.w32(at, m.r32(msg));
                m.w16(at + 4, m.s(msg + 4));
                m.w16(cmd + 0x528, n + 1);
                m.w16(cmd + 0x52a, m.s(cmd + 0x52a) + 1);
            }
            m.set_sp(sp);
            return 0;
        }
    }
    flush_space(m, ctx, cmd, 0xfff);
    m.set_sp(sp);
    1
}

/// [:error mode]: cmd+0x4fc = ignore/text/escape/speak/tone (0..4).
pub fn f_1c001fa0(m: &mut Mem, p1: u32) -> u32 {
    let cmd = m.r32(p1 + 0xc);
    let s = str_arg(m, cmd, 0);
    let k = f_1c005ba0(m, 0x1c02_e19c, s);
    if k == 0xffff {
        return 1;
    }
    if k <= 4 {
        m.w32(cmd + 0x4fc, k);
    }
    0
}

/// [:phoneme ...]: kernel+0x210 bits: asky/arpabet (2), speak/silent (4), on/off (1).
pub fn f_1c001a70(m: &mut Mem, p1: u32) -> u32 {
    let ctx = p1;
    let cmd = m.r32(ctx + 0xc);
    let kernel = m.r32(ctx + 8);
    let mut i: i32 = 0;
    while i < m.i(cmd + 0x2d0) {
        let s = str_arg(m, cmd, i as u32);
        let k = f_1c005ba0(m, 0x1c02_e138, s);
        if k == 0xffff {
            return 1;
        }
        let v = m.r32(kernel + 0x210);
        let v = match k {
            0 => v | 2,
            1 => v & !2,
            2 => v | 4,
            3 => v & !4,
            4 => v | 1,
            5 => v & !1,
            _ => return 4,
        };
        m.w32(kernel + 0x210, v);
        i += 1;
    }
    0
}

/// [:log what on|off|set]: select log categories (kernel+0x204 bits) and open/close log.txt;
/// `dbglog` alone opens/closes dbglog.txt instead.
pub fn f_1c001b40(m: &mut Mem, p1: u32) -> u32 {
    let ctx = p1;
    let cmd = m.r32(ctx + 0xc);
    let kernel = m.r32(ctx + 8);
    let r = f_1c002200(m, ctx);
    if r == 0xb {
        return r;
    }
    let mut bits: u32 = 0;
    let mut i: i32 = 0;
    while i < m.i(cmd + 0x2d0) {
        let s = str_arg(m, cmd, i as u32);
        let k = f_1c005ba0(m, 0x1c02_e154, s);
        if k == 0xffff {
            return 1;
        }
        match i {
            0 => {
                if k > 6 {
                    return 4;
                }
                bits |= 1 << k;
            }
            1 => {
                match k {
                    7 => {
                        if bits | 0x40 == 0x40 {
                            f_1c001cc0(m, ctx);
                        } else if f_1c001d10(m, ctx) != 0 {
                            m.w32(kernel + 0x204, m.r32(kernel + 0x204) | bits);
                        }
                    }
                    8 => {
                        if bits | 0x40 == 0x40 {
                            f_1c001cf0(m, ctx);
                        } else {
                            let v = !bits & m.r32(kernel + 0x204);
                            if v != 0 {
                                m.w32(kernel + 0x204, v);
                            } else {
                                f_1c001db0(m, ctx);
                            }
                        }
                    }
                    9 => {
                        if f_1c001d10(m, ctx) != 0 {
                            m.w32(kernel + 0x204, bits);
                        }
                    }
                    _ => return 4,
                }
                bits = 0;
            }
            _ => return 4,
        }
        i += 1;
    }
    0
}

/// [:mode feature on|off|set]: `[0x5f04, 0/1/2, feature bit]` to LTS, then sync.
pub fn f_1c002890(m: &mut Mem, p1: u32) -> u32 {
    let ctx = p1;
    let cmd = m.r32(ctx + 0xc);
    let kernel = m.r32(ctx + 8);
    let sp = m.sp();
    let msg = m.alloca(8);
    m.w16(msg, 0x5f04);
    m.w16(msg + 2, 0);
    m.w16(msg + 4, 0);
    let mut i: u32 = 0;
    let mut ret = 0;
    while i < m.r32(cmd + 0x2d0) {
        let s = str_arg(m, cmd, i);
        let k = f_1c005ba0(m, 0x1c02_e25c, s);
        if k == 0xffff {
            ret = 1;
            break;
        }
        if i == 0 {
            let (at, bit) = match k {
                0 => (4, 4),
                1 => (4, 8),
                2 => (4, 0x10),
                3 => (4, 0x40),
                4 => (4, 0x80),
                5 => (5, 1),
                6 => (5, 2),
                7 => (5, 4),
                8 => (5, 0x10),
                _ => {
                    ret = 4;
                    break;
                }
            };
            m.w8(msg + at, m.r8(msg + at) | bit);
        } else if i == 1 {
            let op = match k {
                9 => 0,
                10 => 1,
                11 => 2,
                _ => {
                    ret = 4;
                    break;
                }
            };
            m.w16(msg + 2, op);
            let pipe = m.r32(kernel + 0xd0);
            f_1c005c20(m, kernel, pipe, msg, 3);
            f_1c002200(m, ctx);
        } else {
            ret = 4;
            break;
        }
        i += 1;
    }
    m.set_sp(sp);
    ret
}

/// [:say unit]: kernel+0x208 text mode clause/word/letter/filtered_letter/line/syllable
/// (0/1/2/0x10/4/8); the letter modes sync first.
pub fn f_1c001ed0(m: &mut Mem, p1: u32) -> u32 {
    let ctx = p1;
    let cmd = m.r32(ctx + 0xc);
    let kernel = m.r32(ctx + 8);
    let s = str_arg(m, cmd, 0);
    let k = f_1c005ba0(m, 0x1c02_e180, s);
    if k == 0xffff {
        return 1;
    }
    let mode = match k {
        0 => 0,
        1 => 1,
        2 | 3 => {
            let r = f_1c002200(m, ctx);
            if r == 0xb {
                return r;
            }
            if k == 2 { 2 } else { 0x10 }
        }
        4 => 4,
        5 => 8,
        _ => return 0,
    };
    m.w32(kernel + 0x208, mode);
    0
}

/// [:punctuation none|some|all|pass]: cmd+0x500 = 0..3.
pub fn f_1c002300(m: &mut Mem, p1: u32) -> u32 {
    let cmd = m.r32(p1 + 0xc);
    let s = str_arg(m, cmd, 0);
    let k = f_1c005ba0(m, 0x1c02_e1cc, s);
    if k == 0xffff {
        return 1;
    }
    if k > 3 {
        return 2;
    }
    m.w32(cmd + 0x500, k);
    0
}

/// [:skip none|email|punct|rule|all|cpg]: cmd+0x504 = 0..5.
pub fn f_1c002390(m: &mut Mem, p1: u32) -> u32 {
    let cmd = m.r32(p1 + 0xc);
    let s = str_arg(m, cmd, 0);
    let k = f_1c005ba0(m, 0x1c02_e1e0, s);
    if k == 0xffff {
        return 1;
    }
    if k > 5 {
        return 2;
    }
    m.w32(cmd + 0x504, k);
    0
}

/// [:pause ms]: pause the audio device, wait ms (or until halted), resume. Device-only.
pub fn f_1c002030(m: &mut Mem, p1: u32) -> u32 {
    let cmd = m.r32(p1 + 0xc);
    if m.r32(cmd) != 0 {
        f_1c026550(m);
        // GetTickCount/Sleep(<=10) loop until ms elapsed or kernel+0x174 set: omitted.
        f_1c026580(m);
    }
    0
}

/// [:play file]: drain, then stream a .wav file (PCM or mu-law, expanded to 16-bit) to the
/// audio device. File access and device output are no-ops here.
pub fn f_1c005d40(m: &mut Mem, p1: u32) -> u32 {
    let ctx = p1;
    let kernel = m.r32(ctx + 8);
    let cmd = m.r32(ctx + 0xc);
    if m.r32(ctx + 0x84) != 0 {
        return 0;
    }
    if f_1c002200(m, ctx) == 0xb {
        return 0xb;
    }
    let buf = m.malloc(0x4000);
    f_1c0282a0(m, ctx, 0xffff_ffff);
    let dev = m.r32(ctx + 0xd0);
    wave_out_reset_noop(m, dev);
    let hdr = m.malloc(0x12);
    let sp = m.sp();
    let slots = m.alloca(8);
    m.w32(slots, 0);
    let name = str_arg(m, cmd, 0);
    let h = f_1c0060c0(m, name, slots, slots + 4, ctx);
    let fmt = m.r32(slots);
    let err = m.r32(slots + 4);
    m.set_sp(sp);
    let free_all = |m: &mut Mem| {
        m.free(hdr);
        if fmt != 0 {
            m.free(fmt);
        }
        m.free(buf);
    };
    if h == 0 {
        free_all(m);
        return err;
    }
    let mut mulaw = false;
    if m.r16(fmt) == 7 {
        m.w16(fmt, 1);
        mulaw = true;
        m.w32(fmt + 8, m.r32(fmt + 8) << 1);
        m.w16(fmt + 0xc, m.s(fmt + 0xc) << 1);
        m.w16(fmt + 0xe, 0x10);
    }
    let dev = m.r32(ctx + 0xd0);
    wave_out_set_format_noop(m, dev, hdr);
    let mut r = wave_out_open_noop(m, dev, fmt);
    while r == 4 {
        r = wave_out_open_noop(m, dev, fmt);
    }
    let align = m.r16(fmt + 0xc) as u16 as u32;
    if r != 0 || align > 0x2000 {
        wave_file_close_noop(m, h);
        free_all(m);
        f_1c028150(m, ctx, 0xa, 1);
        return 0xa;
    }
    let align = align.max(1);
    let chunk = 0x2000 / align * align;
    loop {
        let mut more = true;
        let mut n = wave_file_read_noop(m, h, buf, chunk);
        if n != chunk || m.r32(kernel + 0x174) != 0 {
            n = n / align * align;
            more = false;
        }
        if (n as i32) > 0 && m.r32(kernel + 0x174) == 0 {
            if mulaw {
                for k in (0..n).rev() {
                    let b = m.r8(buf + k) as u32;
                    m.w16(buf + k * 2, m.s(MULAW_TO_PCM + b * 2));
                }
                n <<= 1;
            }
            let dev = m.r32(ctx + 0xd0);
            wave_out_write_noop(m, dev, buf, n);
        } else if n == 0xffff_ffff {
            f_1c028150(m, ctx, 0xb, 1);
            break;
        }
        if !more {
            break;
        }
    }
    wave_file_close_noop(m, h);
    let dev = m.r32(ctx + 0xd0);
    wave_out_reset_noop(m, dev);
    m.w32(ctx + 0x7c, 0);
    wave_out_drain_noop(m, dev);
    while wave_out_open_noop(m, dev, fmt) != 0 {}
    free_all(m);
    0
}

/// [:resume]: sync, then resume the audio device.
pub fn f_1c0020b0(m: &mut Mem, p1: u32) -> u32 {
    let r = f_1c002200(m, p1);
    if r == 0xb {
        return r;
    }
    f_1c026580(m);
    0
}

/// [:dial digits]: sync, drain, then per DTMF key a two-tone message plus a 100 ms gap to
/// the VTM pipe; `-`, `,` and space are gaps only.
pub fn f_1c003110(m: &mut Mem, p1: u32) -> u32 {
    let ctx = p1;
    let kernel = m.r32(ctx + 8);
    let cmd = m.r32(ctx + 0xc);
    let r = f_1c002200(m, ctx);
    if r == 0xb {
        return r;
    }
    f_1c0282a0(m, ctx, 0xffff_ffff);
    let mut s = str_arg(m, cmd, 0);
    let sp = m.sp();
    let msg = m.alloca(12);
    let send = |m: &mut Mem, words: [i32; 6]| {
        for (k, w) in words.iter().enumerate() {
            m.w16(msg + k as u32 * 2, *w);
        }
        let pipe = m.r32(kernel + 0xdc);
        f_1c005c20(m, kernel, pipe, msg, 6);
    };
    loop {
        let c = m.r8(s);
        if c == 0 {
            break;
        }
        let key = match c {
            b'0'..=b'9' => Some((c - b'0') as u32),
            b'*' => Some(0xa),
            b'#' => Some(0xb),
            b'A' | b'a' => Some(0xc),
            b'B' | b'b' => Some(0xd),
            b'C' | b'c' => Some(0xe),
            b'D' | b'd' => Some(0xf),
            b'-' | b',' | b' ' => None,
            _ => {
                m.set_sp(sp);
                return 1;
            }
        };
        if let Some(key) = key {
            let col = m.s(DTMF_COL + key * 2);
            let row = m.s(DTMF_ROW + key * 2);
            send(m, [2, 100, col, 0x4e7a, row, 0x3184]);
        }
        send(m, [2, 100, 1000, 0, 1000, 0]);
        s += 1;
    }
    m.set_sp(sp);
    0
}

/// [:tone freq ms]: sync, drain, then `[2, ms, freq, 0x7fff, 1000, 0]` to the VTM pipe;
/// 2 if freq exceeds half the sample rate (kernel+0x22c).
pub fn f_1c003070(m: &mut Mem, p1: u32) -> u32 {
    let ctx = p1;
    let kernel = m.r32(ctx + 8);
    let cmd = m.r32(ctx + 0xc);
    let r = f_1c002200(m, ctx);
    if r == 0xb {
        return r;
    }
    f_1c0282a0(m, ctx, 0xffff_ffff);
    if m.r32(cmd) > m.r32(kernel + 0x22c) >> 1 {
        return 2;
    }
    let sp = m.sp();
    let msg = m.alloca(12);
    m.w16(msg, 2);
    m.w16(msg + 2, m.s(cmd + 4));
    m.w16(msg + 4, m.s(cmd));
    m.w16(msg + 6, 0x7fff);
    m.w16(msg + 8, 1000);
    m.w16(msg + 10, 0);
    let pipe = m.r32(kernel + 0xdc);
    f_1c005c20(m, kernel, pipe, msg, 6);
    m.set_sp(sp);
    0
}

/// [:pronounce alternate|name|primary ...]: `[0x3f04, 3/4/5]` to LTS per argument.
pub fn f_1c002a00(m: &mut Mem, p1: u32) -> u32 {
    let ctx = p1;
    let cmd = m.r32(ctx + 0xc);
    let kernel = m.r32(ctx + 8);
    // The message lives in the ctx argument slot: an unmatched keyword (index > 2) keeps
    // the high half of the ctx pointer as the value word.
    let sp = m.sp();
    let msg = m.alloca(4);
    m.w32(msg, ctx);
    m.w16(msg, 0x3f04);
    let mut i: u32 = 0;
    while i < m.r32(cmd + 0x2d0) {
        let s = str_arg(m, cmd, i);
        let k = f_1c005ba0(m, 0x1c02_e290, s);
        if k == 0xffff {
            m.set_sp(sp);
            return 1;
        }
        match k {
            0 => m.w16(msg + 2, 3),
            1 => m.w16(msg + 2, 4),
            2 => m.w16(msg + 2, 5),
            _ => {}
        }
        let pipe = m.r32(kernel + 0xd0);
        f_1c005c20(m, kernel, pipe, msg, 2);
        i += 1;
    }
    m.set_sp(sp);
    0
}

/// [:pitch n]: kernel+0x250 = n (0 when omitted).
pub fn f_1c003040(m: &mut Mem, p1: u32) -> u32 {
    let kernel = m.r32(p1 + 8);
    let cmd = m.r32(p1 + 0xc);
    let v = if m.r32(cmd + 0x2a8) == 1 { 0 } else { m.r32(cmd) };
    m.w32(kernel + 0x250, v);
    0
}

/// [:dv param value ...] / [:define_voice]: `[0x5f06, param-1, value]` to LTS, or `[0x1f07]`
/// for `save` (which takes no value).
pub fn f_1c002e40(m: &mut Mem, p1: u32) -> u32 {
    let ctx = p1;
    let cmd = m.r32(ctx + 0xc);
    let kernel = m.r32(ctx + 8);
    if m.r32(cmd + 0x2d0) == 0 {
        return 0;
    }
    let s = str_arg(m, cmd, 0);
    let k = f_1c005ba0(m, 0x1c02_e300, s) as u16;
    if k == 0xffff {
        return 1;
    }
    let sp = m.sp();
    let msg = m.alloca(8);
    let count = if k == 0 {
        if m.r32(cmd + 0x2ac) == 0 {
            m.set_sp(sp);
            return 2;
        }
        m.w16(msg, 0x1f07);
        1
    } else {
        if m.r32(cmd + 0x2ac) == 1 {
            m.set_sp(sp);
            return 2;
        }
        m.w16(msg, 0x5f06);
        m.w16(msg + 2, k.wrapping_sub(1) as i32);
        m.w16(msg + 4, m.s(cmd + 4));
        3
    };
    let pipe = m.r32(kernel + 0xd0);
    f_1c005c20(m, kernel, pipe, msg, count);
    m.set_sp(sp);
    0
}

/// [:debug hex]: sync, kernel+0x478 = value, sync.
pub fn f_1c0033c0(m: &mut Mem, p1: u32) -> u32 {
    let kernel = m.r32(p1 + 8);
    let cmd = m.r32(p1 + 0xc);
    f_1c002200(m, p1);
    m.w16(kernel + 0x478, m.s(cmd));
    f_1c002200(m, p1);
    0
}

/// [:setv n]: replay stored string n (0..9) as input (cmd+0x510/0x512/0x514).
pub fn f_1c002480(m: &mut Mem, p1: u32) -> u32 {
    let cmd = m.r32(p1 + 0xc);
    let n = m.r32(cmd);
    if n > 9 {
        return 2;
    }
    m.w16(cmd + 0x510, 0);
    m.w16(cmd + 0x512, n as i32);
    m.w16(cmd + 0x514, 1);
    0
}

/// [:loadv n]: read raw input bytes up to and including the next `]` from the text pipe and
/// store them as string n (cmd+0x28 + n*0x3c). The 0x3c-byte buffer is unbounded, as in the DLL.
pub fn f_1c0024b0(m: &mut Mem, p1: u32) -> u32 {
    let kernel = m.r32(p1 + 8);
    let cmd = m.r32(p1 + 0xc);
    let n = m.r32(cmd);
    if n > 9 {
        return 2;
    }
    m.w16(cmd + 0x510, 0);
    m.w16(cmd + 0x512, n as i32);
    let sp = m.sp();
    let buf = m.alloca(0x3c);
    let mut i = 0u32;
    loop {
        let pipe = m.r32(kernel + 0xd8);
        m.pipe_read_bytes(pipe, buf + i, 1);
        let c = m.r8(buf + i);
        i += 1;
        if c == 0x5d {
            break;
        }
    }
    m.w8(buf + i, 0);
    let dst = cmd.wrapping_add((m.s(cmd + 0x512) * 0x3c) as u32) + 0x28;
    let mut k = 0u32;
    loop {
        let c = m.r8(buf + k);
        m.w8(dst + k, c);
        if c == 0 {
            break;
        }
        k += 1;
    }
    m.set_sp(sp);
    0
}

/// [:gender masculine|neuter|feminine]: kernel+0x47a = 1/2/3.
pub fn f_1c0033f0(m: &mut Mem, p1: u32) -> u32 {
    let kernel = m.r32(p1 + 8);
    let cmd = m.r32(p1 + 0xc);
    let s = str_arg(m, cmd, 0);
    let k = f_1c005ba0(m, 0x1c02_e2f0, s);
    if k > 2 {
        return 1;
    }
    m.w16(kernel + 0x47a, k as i32 + 1);
    0
}

/// [:log dbglog on]: open dbglog.txt as kernel+0x47c; 1 if open. File I/O omitted.
pub fn f_1c001cc0(m: &mut Mem, p1: u32) -> u32 {
    let kernel = m.r32(p1 + 8);
    m.w32(kernel + 0x47c, DBGLOG_FILE);
    1
}

/// [:log dbglog off]: fclose(kernel+0x47c) if set (the pointer is left in place). I/O omitted.
pub fn f_1c001cf0(m: &mut Mem, p1: u32) -> u32 {
    let _ = m.r32(m.r32(p1 + 8) + 0x47c);
    0
}

/// Keyword lookup: index of the entry in the NULL-terminated table `p1` equal to the
/// case-folded string `p2`, else of the only entry `p2` is a prefix of; 0xffff otherwise.
pub fn f_1c005ba0(m: &mut Mem, p1: u32, p2: u32) -> u32 {
    let table = p1;
    let s = p2;
    if s == 0 || m.r32(table) == 0 {
        return 0xffff;
    }
    let mut idx = 0u32;
    let mut hits = 0u32;
    let mut last = 0u32;
    let mut ent = table;
    loop {
        let mut a = m.r32(ent);
        let mut b = s;
        loop {
            let c = m.r8(a);
            if c != m.r8(CHAR_FOLD + m.r8(b) as u32) {
                break;
            }
            if c == 0 {
                return idx;
            }
            a += 1;
            b += 1;
        }
        if m.r8(b) == 0 {
            last = idx;
            hits += 1;
        }
        ent += 4;
        idx += 1;
        if m.r32(ent) == 0 {
            break;
        }
    }
    if hits == 1 { last } else { 0xffff }
}

/// Open wave file `p1` and read its 'fmt ' chunk into a malloc'd block (*p2), leaving the
/// file at the 'data' chunk; *p3 = error (7 open, 6 malloc, 8 format). p4 = ctx.
pub fn f_1c0060c0(m: &mut Mem, p1: u32, p2: u32, p3: u32, p4: u32) -> u32 {
    m.w32(p3, 0);
    let h = wave_file_open_noop(m, p1);
    if h == 0 {
        f_1c028150(m, p4, 7, 1);
        m.w32(p3, 7);
        return 0;
    }
    // RIFF/'WAVE'/'fmt '/'data' chunk walk: unreachable while file access is a no-op.
    let _ = p2;
    wave_file_close_noop(m, h);
    f_1c028150(m, p4, 8, 1);
    m.w32(p3, 8);
    0
}

/// Volume table step (0..0x63, clamped) -> device volume.
fn vol_step(m: &Mem, i: u32) -> u32 {
    let i = if i > 0x63 { 0x63 } else { i };
    m.r32(VOLUME_STEPS + i * 4)
}

/// FUN_1c025030: device volume -> nearest step by binary search.
fn vol_index(m: &Mem, v: u32) -> u32 {
    if v > 0xffff {
        return 0x63;
    }
    let mut lo: u32 = 0;
    let mut hi: u32 = 0x63;
    let mut mid;
    loop {
        mid = hi.wrapping_add(lo) >> 1;
        let t = vol_step(m, mid);
        if v < t {
            hi = mid.wrapping_sub(1);
        } else if v > t {
            lo = mid + 1;
        } else {
            break;
        }
        if lo > hi {
            break;
        }
    }
    mid
}

/// FUN_1c025130: apply set (0) / up (1, capped 0x63) / down (2, floored 0) to step `cur`.
fn vol_adjust(cur: u32, amount: u32, mode: u32) -> u32 {
    match mode {
        0 => amount,
        1 => {
            let v = cur.wrapping_add(amount);
            if v > 0x63 { 0x63 } else { v }
        }
        2 => {
            let v = cur.wrapping_sub(amount);
            if (v as i32) < 0 { 0 } else { v }
        }
        _ => cur,
    }
}

/// Adjust the device volume: p2 amount, p3 mode (set/up/down), p4/p5 apply to left/right.
pub fn f_1c025090(m: &mut Mem, p1: u32, p2: u32, p3: u32, p4: u32, p5: u32) -> u32 {
    let dev = m.r32(p1 + 0xd0);
    let cur = wave_out_get_volume_noop(m, dev);
    let mut left = cur & 0xffff;
    let mut right = cur >> 16;
    if p4 != 0 {
        let i = vol_adjust(vol_index(m, left), p2, p3);
        left = vol_step(m, i);
    }
    if p5 != 0 {
        let i = vol_adjust(vol_index(m, right), p2, p3);
        right = vol_step(m, i);
    }
    wave_out_set_volume_noop(m, dev, (right << 16) | left)
}

/// Set left/right device volume steps (each 0..0x63); returns p2 when out of range.
pub fn f_1c025170(m: &mut Mem, p1: u32, p2: u32, p3: u32) -> u32 {
    if (p2 as i32) < 0 || (p2 as i32) > 0x63 || (p3 as i32) < 0 || (p3 as i32) > 0x63 {
        return p2;
    }
    let v = (vol_step(m, p3) << 16) | vol_step(m, p2);
    let dev = m.r32(p1 + 0xd0);
    wave_out_set_volume_noop(m, dev, v)
}

/// Pause the audio device (ctx+0xd0). Device-only: no-op.
pub fn f_1c026550(m: &mut Mem) -> u32 {
    let _ = m;
    0
}

/// Resume the audio device (ctx+0xd0). Device-only: no-op.
pub fn f_1c026580(m: &mut Mem) -> u32 {
    let _ = m;
    0
}

/// Index-query hook (bare `ret`).
pub fn f_1c024e60(m: &mut Mem) -> u32 {
    let _ = m;
    0
}

fn wave_out_get_volume_noop(_m: &mut Mem, _dev: u32) -> u32 {
    0
}

fn wave_out_set_volume_noop(_m: &mut Mem, _dev: u32, _v: u32) -> u32 {
    0
}

fn wave_out_reset_noop(_m: &mut Mem, _dev: u32) {}

fn wave_out_set_format_noop(_m: &mut Mem, _dev: u32, _hdr: u32) {}

fn wave_out_open_noop(_m: &mut Mem, _dev: u32, _fmt: u32) -> u32 {
    0
}

fn wave_out_write_noop(_m: &mut Mem, _dev: u32, _buf: u32, _bytes: u32) {}

fn wave_out_drain_noop(_m: &mut Mem, _dev: u32) {}

/// mmioOpenA: no file access in the standalone port, so every open fails.
fn wave_file_open_noop(_m: &mut Mem, _name: u32) -> u32 {
    0
}

fn wave_file_read_noop(_m: &mut Mem, _h: u32, _buf: u32, _n: u32) -> u32 {
    0
}

fn wave_file_close_noop(_m: &mut Mem, _h: u32) {}
