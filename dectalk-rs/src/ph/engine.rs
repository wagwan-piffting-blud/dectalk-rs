//! Standalone PH thread (FUN_1c019ca0 / FUN_1c01dfa0) and VTM message consumer
//! (FUN_1c02a910): phoneme symbols in, PCM out. No dumps, no DLL.

use std::collections::VecDeque;

use super::mem::Mem;
use super::speaker::{apply_voice, init_arrays, save_user_voice, select_voice, set_voice_param};
use super::{
    allophone, bind_track_outputs, clause_init, duration, f0, frame_emit, frame_tracks_interp, frames_init,
    index_flush, intonation, phone_advance, rules, targets, AUX_SIZE, CTX_SIZE, KERNEL_SIZE, PH_SIZE,
};
use crate::vtm::Vtm;

const CTX_BASE: u32 = 0x1000_0000;
const KERNEL_BASE: u32 = 0x1001_0000;
const PH_BASE: u32 = 0x1002_0000;
const AUX_BASE: u32 = 0x1003_0000;
/// Opaque id for the PH->VTM pipe; only compared, never dereferenced.
pub const VTM_PIPE: u32 = 0x5654_4d00;

// Built-in voice tables (Paul, Betty, Harry, Frank, Dennis, Kit, Ursula, Rita, Wendy).
const VOICES_8K: [u32; 9] = [
    0x1c06_a244, 0x1c06_a288, 0x1c06_a2cc, 0x1c06_a310, 0x1c06_a464, 0x1c06_a354, 0x1c06_a398, 0x1c06_a3dc, 0x1c06_a420,
];
const VOICES_11K: [u32; 9] = [
    0x1c06_a4a8, 0x1c06_a4ec, 0x1c06_a530, 0x1c06_a574, 0x1c06_a6c8, 0x1c06_a5b8, 0x1c06_a5fc, 0x1c06_a640, 0x1c06_a684,
];

pub struct Engine {
    pub m: Mem,
    pub ctx: u32,
    pub kernel: u32,
    pub ph: u32,
    input: VecDeque<u16>,
    pub vtm: Vtm,
    rate: u32,
    msg_cursor: usize,
    pub pcm: Vec<i16>,
}

#[inline]
fn clamp(v: i32, lo: i32, hi: i32) -> i32 {
    // FUN_1c01ea10
    if lo <= v && v <= hi { v } else if lo <= v { hi } else { lo }
}

impl Engine {
    pub fn new(rate: u32) -> Self {
        let mut m = Mem::new();
        m.map("ctx", CTX_BASE, &[0u8; CTX_SIZE]);
        m.map("kernel", KERNEL_BASE, &[0u8; KERNEL_SIZE]);
        m.map("ph", PH_BASE, &vec![0u8; PH_SIZE]);
        m.map("aux", AUX_BASE, &[0u8; AUX_SIZE]);
        let (ctx, kernel, ph) = (CTX_BASE, KERNEL_BASE, PH_BASE);
        m.w32(ctx + 8, kernel);
        m.w32(ctx + 0x18, ph);
        m.w32(kernel + 0xdc, VTM_PIPE);
        m.w32(kernel + 0x22c, rate);

        // FUN_1c019ca0
        m.w32(ph + 0x11a8, ctx);
        m.w32(ph + 0x2848, AUX_BASE);
        for (k, t) in VOICES_8K.iter().enumerate() {
            m.w32(ph + 0x14b4 + k as u32 * 4, *t);
        }
        m.w32(ph + 0x14d8, ph + 0x14dc);
        for (k, t) in VOICES_11K.iter().enumerate() {
            m.w32(ph + 0x148c + k as u32 * 4, *t);
        }
        m.w32(ph + 0x14b0, ph + 0x14dc);

        // FUN_1c01dfa0 prologue
        m.w16(kernel + 0x222, 0xb4);
        bind_track_outputs(&mut m, ph);
        init_arrays(&mut m, ph);
        select_voice(&mut m, ctx, 0);
        save_user_voice(&mut m, ph);
        m.w32(ph + 0x2014, 0);
        m.w32(kernel + 0x24c, ph + 0x1446);
        m.w16(ph + 0x11ca, 0x73);
        m.w32(ph + 0x2018, 0x73);
        m.w16(ph + 0x1426, 1);

        Engine {
            m,
            ctx,
            kernel,
            ph,
            input: VecDeque::new(),
            vtm: Vtm::new(rate),
            rate,
            msg_cursor: 0,
            pcm: Vec::new(),
        }
    }

    pub fn push_symbols(&mut self, words: &[u16]) {
        self.input.extend(words.iter().copied());
    }

    /// FUN_1c01ea30: next complete symbol with its argument words, or None if incomplete.
    fn next_symbol(&mut self) -> Option<(u16, Vec<u16>)> {
        loop {
            let v = *self.input.front()?;
            let lo = v & 0xff;
            if 0x78 < lo && lo < 0x82 {
                self.input.pop_front();
                continue;
            }
            let nargs = ((v & 0x6000) >> 13) as usize;
            if self.input.len() < 1 + nargs {
                return None;
            }
            self.input.pop_front();
            let args = (0..nargs).map(|_| self.input.pop_front().unwrap()).collect();
            return Some((v & 0x9fff, args));
        }
    }

    fn sym_addr(&self, i: i32) -> u32 {
        self.ph.wrapping_add(0x11ca).wrapping_add((i * 2) as u32)
    }

    fn arr(&self, slot: u32, i: i32) -> u32 {
        self.m.r32(self.ph + slot).wrapping_add((i * 2) as u32)
    }

    fn append(&mut self, sym: i32, a1: i32, a2: i32) {
        let n = self.m.s(self.ph + 0x1426);
        let (s, a, b) = (self.sym_addr(n), self.arr(0x1428, n), self.arr(0x142c, n));
        self.m.w16(s, sym);
        self.m.w16(a, a1);
        self.m.w16(b, a2);
        self.m.w16(self.ph + 0x1426, n + 1);
    }

    /// Process all complete symbols currently queued.
    pub fn run(&mut self) {
        while let Some((sym, args)) = self.next_symbol() {
            self.handle_symbol(sym, &args);
        }
    }

    fn handle_symbol(&mut self, sym: u16, args: &[u16]) {
        let (kernel, ph) = (self.kernel, self.ph);
        let a1 = args.first().map_or(0, |&x| x as i16 as i32);
        let a2 = args.get(1).map_or(0, |&x| x as i16 as i32);
        let nargs = args.len() as i32;
        let symi = sym as i32;
        if sym == 0x1f0a || sym == 0x1f11 {
            self.flush_clause();
            self.m.pipe_write_words(VTM_PIPE, &[8]);
            return;
        }
        if (sym & 0xe0ff) == 0x78 {
            self.m.w32(ph + 0x283c, 1);
        }
        if matches!(sym, 0x1f08 | 0x1f09 | 0x1f13 | 0x1f14 | 0x1f15 | 0x1f16) {
            let n = self.m.s(ph + 0x1426);
            self.index_add(n, symi, a1, a2);
            return;
        }
        let n = self.m.s(ph + 0x1426);
        self.m.w32(ph + 0xcb4 + (n * 4) as u32, self.m.r32(ph + 0x118c));
        if (sym & 0x1f00) == 0x1f00 {
            if sym == 0x1f17 {
                let v = (a1 << 16).wrapping_add(a2) as u32;
                self.m.w32(ph + 0x118c, v);
                self.m.w32(ph + 0xcb4 + (n * 4) as u32, v);
                return;
            }
            if 1 < n {
                self.append(0x74, 0, 0);
                self.flush_clause();
            }
            let dbg = (self.m.r8(kernel + 0x204) & 2) != 0
                || ((self.m.r16(kernel + 0x478) as u16 & 0x2000) != 0 && (self.m.r16(kernel + 0x478) as u16 & 2) != 0);
            assert!(!dbg, "PH debug logging not supported");
            match sym {
                0x1f00 => self.m.w16(kernel + 0x222, clamp(a1, 0x4b, 600)),
                0x1f01 => self.m.w16(ph + 0x1532, duration::to_frames(clamp(a1, -0x28, 30000))),
                0x1f02 => self.m.w16(ph + 0x1534, duration::to_frames(clamp(a1, -0x17c, 30000))),
                0x1f03 => self.m.w8(ph + 0x1520, 1),
                0x1f05 => select_voice(&mut self.m, self.ctx, if a1 < 10 { a1 } else { 1 }),
                0x1f06 => set_voice_param(&mut self.m, ph, a1, a2),
                0x1f07 => save_user_voice(&mut self.m, ph),
                0x1f0e => {
                    if self.m.i(ph + 0x2014) == 0 {
                        let v = self.m.s(ph + 0x144c);
                        self.m.w32(ph + 0x2824, v as u32);
                        let off = self.m.i(kernel + 0x250);
                        set_voice_param(&mut self.m, ph, 3, off + v);
                        self.m.w32(ph + 0x2014, 1);
                    }
                }
                _ => {}
            }
            return;
        }
        let last = self.m.i(ph + 0x2020);
        if nargs == 0 && symi == 0 && self.m.s(self.sym_addr(last)) == 0 {
            return;
        }
        if 299 < n + 1 + nargs {
            self.append(0x73, 0, 0);
            self.flush_clause();
        }
        let mut s = symi & 0xe0ff;
        if 0x112 < self.m.s(ph + 0x1426) && s == 0x6f {
            s = 0x73;
        }
        if nargs != 0 {
            let n = self.m.s(ph + 0x1426);
            self.m.w32(ph + 0x2020, n as u32);
            self.append(s, a1, if nargs == 1 { 0 } else { a2 });
            let v = s as i16 as i32;
            if v < 0x71 || 0x76 < v {
                self.m.w16(ph + 0x201c, 0);
            }
            if 0x6e < v && v < 0x72 {
                self.m.w32(ph + 0x2018, v as u32);
            }
            return;
        }
        let v0 = s as i16 as i32;
        let mut v = v0;
        if 0x6e < v0 && v0 < 0x77 {
            let lastp = self.m.i(ph + 0x2020);
            let prev = self.m.s(self.sym_addr(lastp));
            if 0x6e < prev && prev < 0x77 {
                let prev_boundary = self.m.i(ph + 0x2018);
                if (v0 == 0x71 && (prev_boundary == 0x70 || prev_boundary == 0x71)) || v0 <= prev {
                    return;
                }
                if v0 == 0x70 && 0x19 < self.m.s(ph + 0x201c) {
                    v = 0x71;
                }
                if lastp != 0 {
                    let (a, b) = (self.arr(0x1428, lastp), self.arr(0x142c, lastp));
                    self.m.w16(a, 0);
                    self.m.w16(b, 0);
                    let n = self.m.s(ph + 0x1426);
                    for k in lastp + 1..n {
                        let x = self.m.s(self.sym_addr(k));
                        self.m.w16(self.sym_addr(k - 1), x);
                    }
                    self.m.w16(ph + 0x1426, n - 1);
                }
            }
        }
        self.m.w16(ph + 0x201c, self.m.s(ph + 0x201c) + 1);
        if v < 0x71 || 0x76 < v {
            self.m.w16(ph + 0x201c, 0);
        }
        if 0x6e < v && v < 0x72 {
            self.m.w32(ph + 0x2018, v as u32);
        }
        let n = self.m.s(ph + 0x1426);
        self.m.w32(ph + 0x2020, n as u32);
        self.append(v, 0, 0);
        if 0x72 < v && v < 0x77 {
            self.flush_clause();
        }
    }

    /// FUN_1c024b70: queue an index mark at the current clause position.
    fn index_add(&mut self, pos: i32, cmd: i32, a1: i32, a2: i32) {
        let node = self.m.malloc(0x94);
        for k in 0..0x94 / 4 {
            self.m.w32(node + k * 4, 0);
        }
        let head = self.m.r32(self.kernel + 0x17c);
        if head == 0 {
            self.m.w32(self.kernel + 0x17c, node);
        } else {
            let mut t = head;
            while self.m.r32(t) != 0 {
                t = self.m.r32(t);
            }
            self.m.w32(t, node);
        }
        for (k, v) in [(6, cmd), (7, a1), (4, 7), (5, pos), (8, a2), (9, pos), (10, pos), (11, 0)] {
            self.m.w32(node + k * 4, v as u32);
        }
    }

    /// FUN_1c01e8d0
    fn flush_clause(&mut self) {
        let (kernel, ph) = (self.kernel, self.ph);
        if 1 < self.m.s(ph + 0x1426) && self.m.i(kernel + 0x174) == 0 {
            assert!(self.m.i(kernel + 0x208) != 8, "clause mode 8 (FUN_1c01d610) not ported");
            self.clause_driver();
            if self.m.i(ph + 0x2014) != 0 {
                let v = self.m.i(ph + 0x2824);
                set_voice_param(&mut self.m, ph, 3, v);
                self.m.w32(ph + 0x2014, 0);
            }
        }
        index_flush(&mut self.m, kernel, 9999);
        self.m.w32(ph + 0x2020, 0);
        self.m.w16(ph + 0x11ca, 0x73);
        self.m.w32(ph + 0x2018, 0x73);
        self.m.w16(ph + 0x1426, 1);
        self.m.w16(ph + 0x201c, 0);
        self.m.w32(ph + 0x2010, 0);
        self.m.pipe_write_words(VTM_PIPE, &[0xb]);
    }

    /// FUN_1c0167a0
    fn clause_driver(&mut self) {
        let (ctx, kernel, ph) = (self.ctx, self.kernel, self.ph);
        let m = &mut self.m;
        clause_init(m, ph);
        if m.r8(ph + 0x1520) != 0 {
            m.w8(ph + 0x1520, 0);
            apply_voice(m, ctx);
        }
        if rules::clause_rules(m, ctx) == 0 {
            return;
        }
        allophone::allophones(m, ctx);
        m.w32(ph + 0x283c, 0);
        duration::durations(m, ctx);
        intonation::intonation(m, ctx);
        frames_init(m, ph);
        loop {
            m.w16(ph + 0x2ee, m.s(ph + 0x2ee) + 1);
            if m.s(ph + 0x11ac) <= m.s(ph + 0x2ee) {
                let pos = m.s(ph + 0x201c);
                if pos != -1 {
                    index_flush(m, kernel, pos as u32);
                }
                m.w16(ph + 0x201c, pos + 1);
                if phone_advance(m, ph) != 0 {
                    m.pipe_write_words(VTM_PIPE, &[0xb]);
                    return;
                }
                m.w16(ph + 0x2ee, m.s(ph + 0x2ee) - m.s(ph + 0x11ac));
                let i = m.s(ph + 0x201c);
                m.w16(ph + 0x11ac, m.s(ph + 0xa48 + (i * 2) as u32));
                targets::phone_targets(m, ctx);
            }
            f0::f0_frame(m, ctx);
            frame_tracks_interp(m, ctx);
            frame_emit(m, ctx);
        }
    }

    /// Drain queued VTM messages through the synthesizer (FUN_1c02a910's dispatch).
    pub fn synthesize(&mut self) {
        while self.msg_cursor < self.m.pipe_log.len() {
            let words = self.m.pipe_log[self.msg_cursor].words.clone();
            self.msg_cursor += 1;
            match words[0] & 0xff {
                0 => {
                    self.vtm.set_params(&words[1..21]);
                    self.vtm.synth_frame(self.rate);
                    self.pcm.extend(self.vtm.output());
                }
                1 => {
                    self.vtm.reset();
                    self.vtm.set_params(&words[1..25]);
                    self.vtm.apply_speaker();
                }
                2 => {
                    if let Some(s) = crate::vtm::tone::tone(self.rate, words[1], words[2], words[3], words[4], words[5]) {
                        self.pcm.extend(s);
                    }
                }
                _ => {}
            }
        }
    }

    /// Messages the PH stage has written to the VTM pipe so far.
    pub fn vtm_messages(&self) -> impl Iterator<Item = &[u16]> {
        self.m.pipe_log.iter().map(|w| w.words.as_slice())
    }
}
