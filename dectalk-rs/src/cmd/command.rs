//! FUN_1c004470: per-character clause accumulator. Appends the current input character
//! (cmd+0x51c) to the clause buffer (cmd+0x530), decides whether the clause is complete, and on
//! completion runs the e-mail / punctuation / normal text-normalization passes (FUN_1c010c00)
//! and emits the resulting characters and index records down the CMD->LTS pipe.

use super::*;

const CHAR_TYPE: u32 = 0x1c02_e000;
const CHAR_CLASS_W: u32 = 0x1c06_3b91;

const S_FROM: u32 = 0x1c02_edec;
const S_RETURN_PATH: u32 = 0x1c02_ee04;
const S_INTERNET: u32 = 0x1c02_ee24;
const S_MESSAGE_ID: u32 = 0x1c02_ee40;

fn ctype(m: &Mem, ch: u32) -> u8 {
    m.r8(CHAR_TYPE.wrapping_add(ch))
}

/// Is `ch` (a byte) a "space-like" character: type bit 0x80 or the 0x82 marker.
fn is_sep(m: &Mem, ch: u8) -> bool {
    ctype(m, ch as u32) & 0x80 != 0 || ch == 0x82
}

fn aborted(m: &Mem, kernel: u32) -> bool {
    m.r32(kernel + 0x170) != 0 || m.r32(kernel + 0x16c) == 1
}

fn strlen(m: &Mem, a: u32) -> u32 {
    let mut n = 0u32;
    while m.r8(a.wrapping_add(n)) != 0 {
        n += 1;
    }
    n
}

fn strcpy(m: &mut Mem, dst: u32, src: u32) {
    let n = strlen(m, src) + 1;
    m.memmove(dst, src, n);
}

fn zero(m: &mut Mem, a: u32, n: u32) {
    for k in 0..n {
        m.w8(a.wrapping_add(k), 0);
    }
}

/// CRT strncmp of a local byte buffer against a NUL-terminated string in Mem (result == 0 only).
fn strncmp_eq(m: &Mem, buf: &[u8], s: u32, n: usize) -> bool {
    for k in 0..n {
        let a = buf.get(k).copied().unwrap_or(0);
        let b = m.r8(s + k as u32);
        if a != b {
            return false;
        }
        if a == 0 {
            break;
        }
    }
    true
}

/// FUN_1c005c20 with a single word held in a scratch stack dword.
fn emit_word(m: &mut Mem, kernel: u32, w: u32) {
    let sp = m.sp();
    let slot = m.alloca(4);
    m.w32(slot, w);
    let pipe = m.r32(kernel + 0xd0);
    f_1c005c20(m, kernel, pipe, slot, 1);
    m.set_sp(sp);
}

/// Clear the per-pass result arrays; the dword wipe of cmd+0x3224..0x323c spares the word at 0x3234.
fn reset_pass_state(m: &mut Mem, cmd: u32) {
    let keep = m.s(cmd + 0x3234);
    zero(m, cmd + 0x3224, 0x18);
    m.w16(cmd + 0x3234, keep);
    zero(m, cmd + 0xb0c, 0x1f4);
    zero(m, cmd + 0x1aac, 0xbb8);
    zero(m, cmd + 0x2664, 0xbb8);
}

/// Re-measure the clause and classify each word start (FUN_1c0043b0 + FUN_1c0106d0) into cmd+0xd00.
fn tag_words(m: &mut Mem, kernel: u32, cmd: u32) {
    let n = strlen(m, cmd + 0x530) as i32;
    m.w16(cmd + 0x528, n);
    if n as i16 <= 0 {
        return;
    }
    let mut i: i32 = 0;
    loop {
        let ch = m.r8(cmd.wrapping_add(0x530).wrapping_add(i as u32));
        if ch != 0x82 {
            let word_start = i == 0 || is_sep(m, m.r8(cmd.wrapping_add(0x52f).wrapping_add(i as u32)));
            if word_start && ctype(m, ch as u32) & 0x80 == 0 {
                let p = f_1c0043b0(m, cmd.wrapping_add(0x530).wrapping_add(i as u32), cmd + 0x724, 0);
                let r = f_1c0106d0(m, kernel, p, 1);
                m.w8(cmd.wrapping_add(0xd00).wrapping_add(i as u32), r as u8);
                i = i.wrapping_add(strlen(m, cmd + 0x724) as i32);
            }
        }
        i = i.wrapping_add(1);
        if i >= m.s(cmd + 0x528) {
            break;
        }
    }
}

/// FUN_1c010c00 over the standard cmd-state buffers.
fn run_pass(m: &mut Mem, ctx: u32, kernel: u32, cmd: u32, flags: u32, pass: u32, last: u32) {
    let shift = 1u32.wrapping_shl(m.r32(kernel + 0xf8));
    f_1c010c00(
        m,
        ctx,
        cmd + 0x530,
        cmd + 0xb0c,
        cmd + 0x918,
        cmd + 0xd00,
        cmd + 0xef4,
        cmd + 0x1aac,
        cmd + 0x2664,
        shift,
        flags,
        pass,
        last,
        cmd + 0x3224,
    );
}

enum Flow {
    Next,
    Return,
    Abort,
}

/// Pass 0: e-mail header handling (only with kernel+0x200 bit 0x1000 and cmd+0x504 != 1).
fn pass_email(m: &mut Mem, ctx: u32, kernel: u32, cmd: u32, entry_flags: u32) -> Flow {
    let mut flags = (entry_flags & 0xffff) | 0x20;
    if m.s(cmd + 0x52e) == 1 {
        flags |= 0x10;
    }
    run_pass(m, ctx, kernel, cmd, flags, 0, 0);
    if m.r16(cmd + 0x528) == 0 {
        return Flow::Return;
    }
    if aborted(m, kernel) {
        return Flow::Abort;
    }
    strcpy(m, cmd + 0x530, cmd + 0x918);
    f_1c011710(m, cmd + 0xef4, 0, cmd + 0x2664, 0, 500);
    reset_pass_state(m, cmd);
    Flow::Next
}

/// Pass 1: punctuation (skipped when cmd+0x504 == 2).
fn pass_punct(m: &mut Mem, ctx: u32, kernel: u32, cmd: u32) -> Flow {
    tag_words(m, kernel, cmd);
    let mut flags = 1u32.wrapping_shl(m.r32(cmd + 0x500));
    if m.r32(kernel + 0x200) & 0x1000 != 0 {
        flags |= 0x20;
        if m.s(cmd + 0x52e) == 1 {
            flags |= 0x10;
        }
    }
    run_pass(m, ctx, kernel, cmd, flags, 1, 0);
    if m.r16(cmd + 0x528) == 0 {
        return Flow::Return;
    }
    if aborted(m, kernel) {
        return Flow::Abort;
    }
    strcpy(m, cmd + 0x530, cmd + 0x918);
    f_1c011710(m, cmd + 0xef4, 0, cmd + 0x2664, 0, 500);
    Flow::Next
}

/// Pass 2: normal text normalization (skipped when cmd+0x504 == 3).
fn pass_normal(m: &mut Mem, ctx: u32, kernel: u32, cmd: u32) -> Flow {
    reset_pass_state(m, cmd);
    if aborted(m, kernel) {
        return Flow::Abort;
    }
    zero(m, cmd + 0xd00, 0x1f4);
    tag_words(m, kernel, cmd);
    let flags = m.r32(kernel + 0x200) | 0x100;
    let overflow = (m.s(cmd + 0x3222) == 2) as u32;
    run_pass(m, ctx, kernel, cmd, flags, 2, overflow);
    if m.r16(cmd + 0x528) == 0 {
        return Flow::Return;
    }
    if aborted(m, kernel) {
        return Flow::Abort;
    }
    Flow::Next
}

/// Mail-header detection on the first 35 clause bytes (spaces and 0x82 markers squeezed out).
fn email_header_check(m: &mut Mem, cmd: u32) {
    let mut buf = [0u8; 36];
    buf[0] = m.r8(0x1c06_efe0);
    let mut o = 0usize;
    for k in 0..0x23u32 {
        let c = m.r8(cmd + 0x530 + k);
        if c != 0x20 && c != 0x82 {
            buf[o] = c;
            o += 1;
        }
    }
    if m.s(cmd + 0x52e) == 1 && buf[0] == 0xd && buf[1] == 0xa {
        m.w16(cmd + 0x52e, 0);
    }
    if m.s(cmd + 0x52e) == 0
        && (strncmp_eq(m, &buf, S_FROM, 4)
            || strncmp_eq(m, &buf, S_RETURN_PATH, 0xc)
            || strncmp_eq(m, &buf, S_INTERNET, 0xf)
            || strncmp_eq(m, &buf, S_MESSAGE_ID, 0xb))
    {
        m.w16(cmd + 0x52e, 1);
    }
    let len = m.s(cmd + 0x528) as u32;
    let last = m.r8(cmd.wrapping_add(0x52f).wrapping_add(len));
    if (last == 0xa || last == 0x20) && m.r8(cmd.wrapping_add(0x52e).wrapping_add(len)) == 0xd {
        m.w16(cmd + 0x3222, 1);
    }
}

/// Emit the finished clause (cmd+0x918, length cmd+0x322a) to LTS, then reset or carry over.
fn emit_clause(m: &mut Mem, ctx: u32, kernel: u32, cmd: u32) -> u32 {
    let n = m.s(cmd + 0x322a);
    let mut i: i32 = 0;
    let mut state = 1u32;
    while i < n && ctype(m, m.r8(cmd.wrapping_add(0x918).wrapping_add(i as u32)) as u32) & 0x80 != 0 {
        i += 1;
    }
    if m.r16(cmd + 0x52c) != 0 {
        emit_word(m, kernel, 0x20);
    }
    while i < m.s(cmd + 0x322a) {
        let rec = cmd.wrapping_add(0x2666).wrapping_add((i as u32).wrapping_mul(6));
        if aborted(m, kernel) {
            f_1c005980(m, ctx);
            return 0;
        }
        let ch = m.r8(cmd.wrapping_add(0x918).wrapping_add(i as u32));
        let mut check_index = true;
        if ch == 0x82 {
        } else if state == 1 {
            if ch == 0x80 {
                state = 2;
                check_index = false;
            } else {
                emit_word(m, kernel, ch as u32);
            }
        } else if state == 2 {
            if ch == 0x81 {
                state = 1;
                check_index = false;
            } else {
                let tab = m.r32(kernel + 0x460);
                let w = m.r16(tab.wrapping_add(ch as u32 * 4)) as u16 as u32;
                emit_word(m, kernel, w);
            }
        }
        if check_index && f_1c011780(m, cmd + 0x2664, i as u32) as u16 != 0 {
            if i > 0 && m.r8(cmd.wrapping_add(0x917).wrapping_add(i as u32)) != 0x20 {
                emit_word(m, kernel, 0x20);
            }
            let pipe = m.r32(kernel + 0xd0);
            f_1c005c20(m, kernel, pipe, rec - 2, 3);
        }
        i += 1;
    }
    let term = m.r16(cmd + 0x51c) as u16 as u32;
    if term == 0xb || term == 9 {
        emit_word(m, kernel, term);
    }
    if aborted(m, kernel) {
        f_1c005980(m, ctx);
        return 0;
    }
    if m.s(cmd + 0x3222) == 2 {
        // Clause overflowed: shift the unconsumed tail (from 0x3224+0x3226) to the buffer start.
        let mut src = m.s(cmd + 0x3224) + m.s(cmd + 0x3226);
        let mut dst: u32 = 0;
        let mut ch = m.r8(cmd.wrapping_add(0x530).wrapping_add(src as u32));
        while ch != 0 {
            m.w8(cmd + 0x530 + dst, ch);
            f_1c011750(m, cmd + 0xef4, dst, cmd + 0xef4, src as u32);
            ch = m.r8(cmd.wrapping_add(0x531).wrapping_add(src as u32));
            src += 1;
            dst += 1;
        }
        m.w16(cmd + 0x52c, 1);
        m.w16(cmd + 0x528, dst as i32);
        let sum16 = m.s(cmd + 0x3224).wrapping_add(m.s(cmd + 0x3226));
        let left = m.s(cmd + 0x3220).wrapping_sub(sum16);
        m.w16(cmd + 0x3220, left);
        let consumed = m.s(cmd + 0x3224) + m.s(cmd + 0x3226);
        let p = m.r32(cmd + 0x321c).wrapping_sub(consumed as u32);
        m.w32(cmd + 0x321c, p);
        let rest = 0x1f3u32.wrapping_sub(dst);
        zero(m, cmd + 0x530 + dst, rest);
        zero(m, cmd + 0xef4 + dst * 6, rest.wrapping_mul(6));
    } else {
        zero(m, cmd + 0x530, 0x1f4);
        m.w16(cmd + 0x52c, 0);
        m.w16(cmd + 0x528, 0);
        m.w16(cmd + 0x52a, 0);
        m.w8(cmd + 0x530, 0x20);
        m.w16(cmd + 0x3220, 0);
        zero(m, cmd + 0xef4, 0xbb8);
        zero(m, cmd + 0x1aac, 0xbb8);
        m.w32(cmd + 0x321c, cmd + 0x530);
    }
    zero(m, cmd + 0xd00, 0x1f4);
    let keep = m.s(cmd + 0x3234);
    zero(m, cmd + 0x3224, 0x18);
    m.w16(cmd + 0x3234, keep);
    zero(m, cmd + 0x918, 0x1f4);
    zero(m, cmd + 0xb0c, 0x1f4);
    zero(m, cmd + 0x1aac, 0xbb8);
    zero(m, cmd + 0x2664, 0xbb8);
    m.w16(cmd + 0x3222, 0);
    0
}

/// Normalize the incoming character (cmd+0x51c) and append it to the clause buffer; sets
/// cmd+0x3222 (1 = clause end, 2 = overflow) and tracks the current word start (cmd+0x321c).
fn accumulate(m: &mut Mem, kernel: u32, cmd: u32) {
    let c = m.r16(cmd + 0x51c) as u16 as u32;
    let mode = m.r32(cmd + 0x500);
    let mut keep_508 = false;
    if c <= 0xff && m.r8(CHAR_CLASS_W + c * 2) & 0x80 != 0 && mode == 1 {
        let email = m.r32(kernel + 0x200) & 0x1000 != 0;
        let prev = m.r32(cmd + 0x2ec);
        let paren = c == 0x29 || c == 0x28;
        let emoticon = email && paren && (prev == 0x5e || prev == 0x2d || prev == 0x3a);
        keep_508 = true;
        if !emoticon {
            m.w32(cmd + 0x508, c);
            if !matches!(c, 0x29 | 0x5d | 0x7d | 0x22 | 0x5c | 0x3e) {
                m.w16(cmd + 0x51c, 0x20);
            }
            if m.r32(kernel + 0x200) & 0x1000 != 0 && m.s(cmd + 0x52e) == 1 && m.s(cmd + 0x51c) == 0x22 {
                m.w16(cmd + 0x51c, 0x20);
            }
        }
    }
    if !keep_508 {
        m.w32(cmd + 0x508, 0);
    }
    let c = m.s(cmd + 0x51c);
    if c == 0 || c == 0x11 {
        m.w16(cmd + 0x51c, 0x20);
    }
    let c = m.s(cmd + 0x51c);
    if c == 0xfff {
        m.w16(cmd + 0x3222, 1);
        m.w16(cmd + 0x51c, 0x20);
    } else if c == 0xb {
        let len = m.s(cmd + 0x528);
        m.w16(cmd + 0x3222, 1);
        m.w8(cmd.wrapping_add(0x530).wrapping_add(len as u32), 0x20);
        m.w16(cmd + 0x528, len + 1);
    }
    let len = m.s(cmd + 0x528);
    let b = m.r8(cmd + 0x51c);
    m.w8(cmd.wrapping_add(0x530).wrapping_add(len as u32), b);
    m.w16(cmd + 0x528, len + 1);
    if m.s(cmd + 0x51c) == 0x11 {
        let len = m.s(cmd + 0x528);
        m.w8(cmd.wrapping_add(0x530).wrapping_add(len as u32), 0x20);
        m.w16(cmd + 0x528, len + 1);
    }
    if m.r32(kernel + 0x200) & 0x1000 == 0 {
        let at = cmd.wrapping_add(m.s(cmd + 0x528) as u32);
        let last = m.r8(at + 0x52f);
        if is_sep(m, last) && ctype(m, m.r8(at + 0x52e) as u32) & 0x40 != 0 {
            m.w16(cmd + 0x3222, 1);
            if m.r8(at + 0x52e) == b'.' {
                let c = m.r16(cmd + 0x51c) as u16 as u32;
                if ctype(m, c) & 0x80 != 0 || c == 0x82 {
                    let word = m.r32(cmd + 0x321c);
                    let p = f_1c0043b0(m, word, cmd + 0x724, 1);
                    if f_1c0106d0(m, kernel, p, 0) != 0 {
                        m.w16(cmd + 0x3222, 0);
                    }
                }
            }
        }
        m.w16(cmd + 0x52e, 0);
    } else {
        email_header_check(m, cmd);
    }
}

/// FUN_1c004470: feed one character into the clause buffer; flush and speak the clause when complete.
pub fn f_1c004470(m: &mut Mem, p1: u32) -> u32 {
    let ctx = p1;
    let cmd = m.r32(ctx + 0xc);
    let kernel = m.r32(ctx + 8);
    let entry_flags = m.r16(cmd + 0x3234) as u16 as u32;
    if aborted(m, kernel) {
        f_1c005980(m, ctx);
        return 0;
    }
    if m.r32(kernel + 0x200) & 0x1000 != 0 && m.s(cmd + 0x52e) == 1 && m.s(cmd + 0x51c) == 9 {
        m.w16(cmd + 0x51c, 0x20);
    }
    if m.r32(kernel + 0x200) & 0x400 == 0 {
        if m.s(cmd + 0x51c) == 9 {
            let prev = m.r32(cmd + 0x2ec);
            if ctype(m, prev) & 0x80 == 0 || m.r32(cmd + 0x508) != 0 {
                m.w16(cmd + 0x51c, 0xb);
            } else {
                m.w16(cmd + 0x51c, 0x20);
            }
        }
    } else {
        let c = m.s(cmd + 0x51c);
        if c == 9 || c == 0xa || c == 0xd {
            m.w16(cmd + 0x51c, 0xb);
        }
    }

    // Pass-through mode: forward the character straight to LTS.
    if m.r32(cmd + 0x500) == 3 || m.r32(cmd + 0x504) == 4 {
        for bad in [0, 0xfff, 0x11] {
            if m.s(cmd + 0x51c) == bad {
                m.w16(cmd + 0x51c, 0x20);
            }
        }
        let c = m.r16(cmd + 0x51c) as u16 as u32;
        emit_word(m, kernel, c);
        return 0;
    }

    accumulate(m, kernel, cmd);

    let len = m.r16(cmd + 0x528);
    let at = cmd.wrapping_add(len as i32 as u32);
    if is_sep(m, m.r8(at + 0x52e)) && !is_sep(m, m.r8(at + 0x52f)) {
        m.w32(cmd + 0x321c, at + 0x52f);
    }
    if m.s(cmd + 0x3222) == 0 {
        if len > 0x12c {
            m.w16(cmd + 0x3222, 2);
        }
        if m.s(cmd + 0x3222) == 0 {
            return 0;
        }
    }
    m.w8(at + 0x530, 0);

    if m.s(cmd + 0x52a) == 0 && m.s(cmd + 0x528) < 5 && m.r32(cmd + 0x500) != 2 {
        strcpy(m, cmd + 0x918, cmd + 0x530);
        let n = m.s(cmd + 0x528);
        m.w16(cmd + 0x322a, n);
        return emit_clause(m, ctx, kernel, cmd);
    }

    if m.r32(cmd + 0x504) != 1 && m.r32(kernel + 0x200) & 0x1000 != 0 {
        match pass_email(m, ctx, kernel, cmd, entry_flags) {
            Flow::Next => {}
            Flow::Return => return 0,
            Flow::Abort => {
                f_1c005980(m, ctx);
                return 0;
            }
        }
    }
    if m.r32(cmd + 0x504) != 2 {
        match pass_punct(m, ctx, kernel, cmd) {
            Flow::Next => {}
            Flow::Return => return 0,
            Flow::Abort => {
                f_1c005980(m, ctx);
                return 0;
            }
        }
    }
    if m.r32(cmd + 0x504) != 3 {
        match pass_normal(m, ctx, kernel, cmd) {
            Flow::Next => {}
            Flow::Return => return 0,
            Flow::Abort => {
                f_1c005980(m, ctx);
                return 0;
            }
        }
    }
    emit_clause(m, ctx, kernel, cmd)
}
