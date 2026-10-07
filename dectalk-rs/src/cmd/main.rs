//! CMD main loop, input reader (with ANSI escape-sequence parsing), init, and the runtime
//! shims CMD shares with the API layer (pipe writer, language registration, error callback).

use super::*;

const CHAR_FLAGS: u32 = 0x1c02_e000;
const PARAM_SLOTS: u32 = 41;

/// FUN_1c003500: fetch one input character and run it through the parser state machine.
pub fn f_1c003500(m: &mut Mem, p1: u32) -> u32 {
    let ctx = p1;
    let cmd = m.r32(ctx + 0xc);
    let kernel = m.r32(ctx + 8);
    let mut run: i32 = 0;
    m.w16(cmd + 0x51c, 0);
    f_1c005a60(m, kernel);
    cmd_loop(m, ctx, &mut run)
}

/// The body of FUN_1c003500. Input is read only at the top of an iteration, so after the
/// reader unwinds on an empty pipe the loop can be re-entered with the same `run` counter.
pub fn cmd_loop(m: &mut Mem, ctx: u32, run: &mut i32) -> ! {
    let cmd = m.r32(ctx + 0xc);
    let kernel = m.r32(ctx + 8);
    loop {
        if m.r16(kernel + 0x478) as u16 == 0x8fff {
            m.w16(kernel + 0x478, 0);
        }
        let prev = m.r16(cmd + 0x51c) as u16 as u32;
        m.w32(cmd + 0x2ec, prev);
        let ch = if m.r16(cmd + 0x514) == 1 { f_1c003a30(m, cmd) } else { f_1c003a80(m, ctx) };
        m.w16(cmd + 0x51c, ch as i32);
        let cur = m.r16(cmd + 0x51c) as u16 as u32;
        let mut reset = true;
        if m.r8(CHAR_FLAGS + cur) & 0x80 != 0 && m.r8(CHAR_FLAGS.wrapping_add(m.r32(cmd + 0x2ec))) & 0x80 != 0 {
            *run += 1;
            if *run as i16 > 0x28 {
                m.w16(cmd + 0x51c, 0xb);
            } else {
                reset = false;
            }
        }
        if reset {
            *run = 0;
        }
        let ch = m.r16(cmd + 0x51c) as u16 as u32;
        match m.r32(cmd + 0x4f8) {
            0 => {
                m.w32(cmd + 0x2e8, 0xffff);
                if ch == 0x5b {
                    f_1c0039e0(m, cmd, 1);
                } else {
                    f_1c003850(m, ctx, ch);
                }
            }
            1 => match ch {
                9 | 10 | 0xd | 0x20 => {}
                0x3a => {
                    f_1c001870(m, cmd, 2);
                }
                0x5b => {
                    f_1c003850(m, ctx, 0x5b);
                }
                0x5d => {
                    f_1c001870(m, cmd, 0);
                }
                _ => {
                    if m.r8(kernel + 0x210) & 1 == 0 {
                        m.w16(cmd + 0x51c, 0xfff);
                        f_1c003850(m, ctx, 0x20);
                        m.w16(cmd + 0x51c, ch as i32);
                        f_1c0039e0(m, cmd, 3);
                        let c = m.r16(cmd + 0x51c) as u16 as u32;
                        f_1c003ed0(m, ctx, c);
                    } else if m.r32(cmd + 0x2e8) == 0xffff {
                        m.w16(cmd + 0x51c, 0x5b);
                        f_1c003850(m, ctx, 0x5b);
                        m.w16(cmd + 0x51c, ch as i32);
                        f_1c003850(m, ctx, ch);
                        f_1c0039e0(m, cmd, 0);
                    } else {
                        f_1c0018f0(m, ctx, 3);
                        f_1c0039e0(m, cmd, 5);
                    }
                }
            },
            2 => {
                f_1c001000(m, ctx, ch);
            }
            3 => {
                f_1c003ed0(m, ctx, ch);
            }
            4 => {
                f_1c0012c0(m, ctx, ch);
            }
            5 => {
                if ch == 0x5d {
                    f_1c001870(m, cmd, 0);
                }
            }
            _ => {}
        }
    }
}

fn esc_reset(m: &mut Mem, blk: u32, state: u32) {
    m.w16(blk, state as i32);
    m.w8(blk + 3, 0);
    m.w16(blk + 6, 0);
    m.w16(blk + 4, 0);
    m.w8(blk + 2, 0);
}

/// FUN_1c003a80: read the next input byte, swallowing ANSI/C1 escape and control sequences.
/// Returns the character, or the sequence introducer (e.g. 0x9b) once a sequence completes
/// with its parameters left in the block at cmd+0x518.
pub fn f_1c003a80(m: &mut Mem, p1: u32) -> u32 {
    let ctx = p1;
    let cmd = m.r32(ctx + 0xc);
    let kernel = m.r32(ctx + 8);
    let sp = m.sp();
    let slot = m.alloca(4);
    loop {
        let ch = loop {
            let pipe = m.r32(kernel + 0xd8);
            m.pipe_read_bytes(pipe, slot, 1);
            let c = m.r8(slot) as u32;
            if !matches!(c, 0xff | 0x80 | 0x81 | 0x82) {
                break c;
            }
        };
        if m.r32(kernel + 0x174) != 0 {
            f_1c001870(m, cmd, 0);
            let blk = m.r32(cmd + 0x518);
            m.w16(blk, 0);
            m.set_sp(sp);
            return 0xb;
        }
        m.w32(kernel + 0x218, m.s(cmd + 0x323c) as u32);
        let api = m.r32(kernel + 0x16c);
        if api != 0 {
            if api != 1 && api != 2 {
                f_1c024f70(m, kernel);
                f_1c001870(m, cmd, 0);
                let blk = m.r32(cmd + 0x518);
                m.w16(blk, 0);
            }
            continue;
        }
        let blk = m.r32(cmd + 0x518);
        if ch == 0x1b {
            esc_reset(m, blk, ch);
            continue;
        }
        if matches!(ch, 0x90 | 0x9b | 0x9d | 0x9e | 0x9f) {
            m.w32(kernel + 0xb8, 1);
            esc_reset(m, blk, ch);
            continue;
        }
        let state = m.r16(blk) as u16 as u32;
        if state == 0 {
            m.set_sp(sp);
            return ch;
        }
        if (0x80..=0x9f).contains(&ch) || ch == 0x18 || ch == 0x1a {
            m.w16(blk, 0);
            m.set_sp(sp);
            return ch;
        }
        if ch < 0x20 || ch == 0x7f {
            m.set_sp(sp);
            return ch;
        }
        if ch <= 0x2f {
            let n = m.s(blk + 6);
            if n >= 0x14 {
                m.w8(blk + 2, 1);
            } else {
                m.w8(blk.wrapping_add(n as u32) + 0x44, ch as u8);
                m.w16(blk + 6, n + 1);
            }
            continue;
        }
        if state != 0x1b && (0x30..=0x3f).contains(&ch) {
            if m.r16(blk + 6) != 0 {
                m.w8(blk + 2, 1);
            }
            match ch {
                0x30..=0x39 => {
                    if m.r16(blk + 4) == 0 {
                        m.w16(blk + 8, 0);
                        m.w16(blk + 4, 1);
                    }
                    let n = m.s(blk + 4) as u32;
                    m.w8(blk.wrapping_add(n) + 0x2f, 0);
                    let p = blk.wrapping_add(n * 2) + 6;
                    let v = (m.s(p) as i16).wrapping_mul(10);
                    m.w16(p, v.wrapping_add(ch as i16 - 0x30) as i32);
                }
                0x3a | 0x3b => {
                    if ch == 0x3a {
                        m.w8(blk + 2, 1);
                    }
                    let n = m.s(blk + 4);
                    if n >= 0x14 {
                        m.w8(blk + 2, 1);
                    } else {
                        m.w8(blk.wrapping_add(n as u32) + 0x30, 1);
                        m.w16(blk.wrapping_add(n as u32 * 2) + 8, 0);
                        m.w16(blk + 4, n + 1);
                    }
                }
                0x3c | 0x3d => {
                    m.w8(blk + 2, 1);
                }
                _ => {
                    if m.r8(blk + 3) != 0 || m.r16(blk + 4) != 0 {
                        m.w8(blk + 2, 1);
                    }
                    m.w8(blk + 3, ch as u8);
                }
            }
            continue;
        }
        if state == 0x1b && m.r16(blk + 6) == 0 && (0x40..=0x5f).contains(&ch) {
            let c1 = ch + 0x40;
            if matches!(c1, 0x90 | 0x9b | 0x9d | 0x9e | 0x9f) {
                esc_reset(m, blk, c1);
                continue;
            }
            m.w32(kernel + 0xb8, 0);
            m.w16(blk, 0);
            m.set_sp(sp);
            return c1;
        }
        m.w8(blk + 0x58, ch as u8);
        let intro = m.s(blk) as u32;
        m.w16(blk, 0);
        if m.r8(blk + 2) == 0 {
            m.set_sp(sp);
            return intro;
        }
    }
}

/// FUN_1c003a30: replay the next character of a stored string (cmd+0x28 + n*0x3c).
pub fn f_1c003a30(m: &mut Mem, p1: u32) -> u32 {
    let cmd = p1;
    let row = (m.s(cmd + 0x512) * 0x3c) as u32;
    let idx = m.s(cmd + 0x510) as u32;
    if m.r8(cmd.wrapping_add(idx).wrapping_add(row) + 0x28) == 0 {
        m.w16(cmd + 0x514, 0);
        return 1;
    }
    let next = (idx as i32 + 1) as i16;
    m.w16(cmd + 0x510, next as i32);
    m.r8(cmd.wrapping_add(next as i32 as u32).wrapping_add(row) + 0x27) as i8 as i32 as u32
}

/// FUN_1c0039e0: switch the parser state (cmd+0x4f8).
pub fn f_1c0039e0(m: &mut Mem, p1: u32, p2: u32) -> u32 {
    let cmd = p1;
    m.w32(cmd + 0x2d4, 0);
    m.w32(cmd + 0x2d8, 0);
    if p2 == 4 {
        if m.r32(cmd + 0x4f8) == 4 {
            let n = m.r32(cmd + 0x2d0);
            m.w32(cmd + 0x2d0, n.wrapping_add(1));
        }
    } else if p2 == 5 && m.r32(cmd + 0x2ec) == 0x5d {
        m.w32(cmd + 0x4f8, 0);
        return 0;
    }
    m.w32(cmd + 0x4f8, p2);
    0
}

/// FUN_1c001870: reset the command-argument state and enter parser state `p2` (6 = keep).
pub fn f_1c001870(m: &mut Mem, p1: u32, p2: u32) -> u32 {
    let cmd = p1;
    if p2 != 6 {
        for k in 0..10 {
            m.w32(cmd + 0x2a8 + k * 4, 1);
        }
        let arr = m.r32(cmd + 0x2e0);
        for k in 0..PARAM_SLOTS {
            m.w32(arr + k * 4, 0);
        }
        m.w32(cmd + 0x2e4, PARAM_SLOTS);
        f_1c0039e0(m, cmd, p2);
        m.w32(cmd + 0x4f4, 0);
    }
    m.w32(cmd + 0x4f0, 0);
    m.w32(cmd + 0x2d0, 0);
    m.w32(cmd + 0x2d8, 0);
    m.w32(cmd + 0x2dc, 0);
    m.w32(cmd + 0x2d4, 0);
    0
}

/// FUN_1c005c20: write `p4` words at `p3` to pipe `p2`; a sync (0x1f0a/0x1f0b) is followed by
/// a space word (overwriting the source word).
pub fn f_1c005c20(m: &mut Mem, p1: u32, p2: u32, p3: u32, p4: u32) -> u32 {
    let kernel = p1;
    let dbg = m.r16(kernel + 0x478) as u16;
    let w = m.r16(p3) as u16;
    if dbg & 0x8000 == 0 || dbg & 0x800 == 0 || matches!(w, 0x1f0a | 0x1f0b | 0x1f0c | 0x1f0d) {
        m.pipe_write(p2, p3, sx16(p4));
        let w = m.r16(p3) as u16;
        if w == 0x1f0a || w == 0x1f0b {
            m.w16(p3, 0x20);
            m.pipe_write(p2, p3, 1);
        }
    }
    0
}

/// FUN_1c003e70: echo an input character to the log file (logging only).
pub fn f_1c003e70(_m: &mut Mem, _p1: u32, _p2: u32) -> u32 {
    0
}

/// FUN_1c005810: initialize the CMD state block.
pub fn f_1c005810(m: &mut Mem, p1: u32) -> u32 {
    let ctx = p1;
    let cmd = m.r32(ctx + 0xc);
    let kernel = m.r32(ctx + 8);
    m.w32(cmd + 0x4fc, 3);
    m.w32(cmd + 0x500, 1);
    m.w32(cmd + 0x504, 0);
    m.w32(cmd + 0x508, 0);
    m.w32(cmd + 0x2ec, 0);
    m.w16(cmd + 0x323c, 0);
    let blk = m.r32(cmd + 0x518);
    for k in 0..0x5a {
        m.w8(blk + k, 0);
    }
    m.w32(kernel + 0x210, 5);
    m.w32(kernel + 0x250, 0x23);
    m.w16(cmd + 0x514, 0);
    m.w16(kernel + 0x478, 0);
    m.w16(kernel + 0x47a, 0);
    m.w32(kernel + 0x47c, 0);
    for k in 0..10 {
        m.w8(cmd + 0x28 + k * 0x3c, 0);
    }
    for k in 0..10 {
        m.w32(cmd + k * 4, 0);
    }
    m.w16(cmd + 0x3222, 0);
    m.w16(cmd + 0x52c, 0);
    m.w16(cmd + 0x52e, 0);
    m.w16(cmd + 0x52a, 0);
    m.w16(cmd + 0x528, 0);
    let zero = |m: &mut Mem, off: u32, len: u32| {
        for k in 0..len {
            m.w8(cmd + off + k, 0);
        }
    };
    zero(m, 0x530, 0x7d * 4);
    zero(m, 0x724, 0xc * 4 + 2);
    zero(m, 0xb0c, 0x7d * 4);
    zero(m, 0x918, 0x7d * 4);
    zero(m, 0xd00, 0x7d * 4);
    zero(m, 0xef4, 0x2ee * 4);
    zero(m, 0x1aac, 0x2ee * 4);
    zero(m, 0x2664, 0x2ee * 4);
    m.w8(cmd + 0x530, 0x20);
    m.w32(cmd + 0x321c, cmd + 0x531);
    m.w16(cmd + 0x3220, 0);
    zero(m, 0x3224, 6 * 4);
    m.w16(cmd + 0x3224, 0);
    m.w16(cmd + 0x3240, 0);
    0
}

/// FUN_1c005a60: register the US English language descriptor.
pub fn f_1c005a60(m: &mut Mem, p1: u32) -> u32 {
    m.w32(p1 + 0x454, 0);
    f_1c029150(m, p1)
}

/// FUN_1c005cf0: free the CMD state.
pub fn f_1c005cf0(m: &mut Mem, p1: u32) -> u32 {
    for off in [0x518, 0x2e0] {
        let p = m.r32(p1 + off);
        if p != 0 {
            m.free(p);
        }
        m.w32(p1 + off, 0);
    }
    m.free(p1);
    0
}

/// FUN_1c029150: append a language node {next, id 0, 8 descriptor fields} to kernel+0x454
/// and load its descriptor into kernel+0x458..0x474.
pub fn f_1c029150(m: &mut Mem, p1: u32) -> u32 {
    let kernel = p1;
    const DESC: [u32; 8] = [0x1c06_d39c, 0x79, 0x1c06_d418, 0x1c06_d618, 0xf2, 0, 0x1c06_cf9c, 0x1c06_d70c];
    let node = m.malloc(0x28);
    m.w32(node, 0);
    m.w32(node + 4, 0);
    for (k, v) in DESC.iter().enumerate() {
        m.w32(node + 8 + k as u32 * 4, *v);
        m.w32(kernel + 0x458 + k as u32 * 4, *v);
    }
    let head = m.r32(kernel + 0x454);
    if head == 0 {
        m.w32(kernel + 0x454, node);
    } else {
        let mut last = head;
        while m.r32(last) != 0 {
            last = m.r32(last);
        }
        m.w32(last, node);
    }
    f_1c024e90(m, kernel, 0, 4)
}

/// FUN_1c0282a0: wait (up to p2 ms) until the downstream pipes are empty.
pub fn f_1c0282a0(m: &mut Mem, _p1: u32, _p2: u32) -> u32 {
    m.drain_downstream();
    0
}

/// FUN_1c021e20: pipe fill count (only used by the drain wait).
pub fn f_1c021e20(_m: &mut Mem, _p1: u32) -> u32 {
    0
}

/// FUN_1c024e90: mark thread `p2` as ready with `p3`; once all three pipeline threads are up
/// (mask 7), activate that language's pipes and descriptor.
pub fn f_1c024e90(m: &mut Mem, p1: u32, p2: u32, p3: u32) -> u32 {
    let kernel = p1;
    let slot = kernel + 0xfc + p2 * 4;
    let v = m.r32(slot) | p3;
    m.w32(slot, v);
    if v == 7 && (m.r32(kernel + 0xf8) == 0xffff || p3 == 0) {
        m.w32(kernel + 0xf8, p2);
        m.w32(kernel + 0xd0, m.r32(kernel + 0x118 + p2 * 4));
        m.w32(kernel + 0xd4, m.r32(kernel + 0x134 + p2 * 4));
        let mut node = m.r32(kernel + 0x454);
        while node != 0 {
            if m.r32(node + 4) == p2 {
                for k in 0..8 {
                    m.w32(kernel + 0x458 + k * 4, m.r32(node + 8 + k * 4));
                }
            }
            node = m.r32(node);
        }
    }
    0
}

/// FUN_1c024f70: acknowledge an API reset request.
pub fn f_1c024f70(m: &mut Mem, p1: u32) -> u32 {
    m.w32(p1 + 0x16c, 0);
    m.w32(p1 + 0x1e0, 0);
    0
}

pub fn f_1c024f90(_m: &mut Mem) -> u32 {
    0
}

pub fn f_1c024fa0(_m: &mut Mem) -> u32 {
    0
}

/// FUN_1c028150: report an error to the application once (callback not modeled).
pub fn f_1c028150(m: &mut Mem, p1: u32, p2: u32, p3: u32) -> u32 {
    if m.r32(p1 + 0x88) != 0 {
        m.w32(p1 + 0x88, 0);
        let arg = m.r32(p1 + 0x54);
        f_1c028840(m, p1, arg, p2, p3);
    }
    0
}

/// FUN_1c028840: application callback dispatch; only the audio-pause flag is kept.
pub fn f_1c028840(m: &mut Mem, p1: u32, _p2: u32, p3: u32, _p4: u32) -> u32 {
    if p3 == 0xc {
        m.w32(p1 + 200, 1);
    } else if p3 == 0xd {
        m.w32(p1 + 200, 0);
    }
    0
}

pub fn f_1c029140(_m: &mut Mem) -> u32 {
    0
}

pub fn f_1c010580(_m: &mut Mem, _p1: u32) -> u32 {
    0
}

pub fn f_1c010590(_m: &mut Mem, _p1: u32) -> u32 {
    0
}
