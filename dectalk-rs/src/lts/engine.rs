//! Standalone LTS thread: own init (FUN_1c00fee0) and main loop (FUN_1c00be90).

use std::panic::{self, AssertUnwindSafe};

use super::*;
use crate::ph::mem::PipeEof;

pub const CTX: u32 = 0x1100_0000;
pub const KERNEL: u32 = 0x1101_0000;
pub const LTS: u32 = 0x1102_0000;
/// The main loop's stack frame: word bounds at +0x10/+0x14, node pool at +0x42c.
pub const FRAME: u32 = 0x1103_0000;
pub const IN_PIPE: u32 = 0x4c54_5300;
pub const OUT_PIPE: u32 = 0x5048_0000;
const MAIN_TABLE: u32 = 0x2000_0004;
const USER_TABLE: u32 = 0x3000_0004;
const FRAME_SIZE: usize = 0x1630;

/// Language descriptor fields copied into kernel+0x458..0x474 by FUN_1c024e90.
const LANG_US: [(u32, u32); 7] = [
    (0x458, 0x1c06_d39c),
    (0x45c, 0x79),
    (0x460, 0x1c06_d418),
    (0x464, 0x1c06_d618),
    (0x468, 0xf2),
    (0x470, 0x1c06_cf9c),
    (0x474, 0x1c06_d70c),
];

pub struct Lts {
    pub m: Mem,
}

impl Lts {
    pub fn new(main_dict: &[u8], user_dict: Option<&[u8]>) -> Self {
        let mut m = Mem::new();
        m.map("ctx", CTX, &[0; 0xe8]);
        m.map("kernel", KERNEL, &[0; 0x488]);
        m.map("lts", LTS, &[0; LTS_SIZE]);
        m.map("frame", FRAME, &[0; FRAME_SIZE]);
        m.w32(CTX + 8, KERNEL);
        m.w32(CTX + 0x10, LTS);
        let (t, n) = map_dict(&mut m, "dict_main", main_dict, MAIN_TABLE);
        m.w32(KERNEL, t);
        m.w32(KERNEL + 0x1c, n);
        if let Some(u) = user_dict {
            let (t, n) = map_dict(&mut m, "dict_user", u, USER_TABLE);
            m.w32(KERNEL + 0x38, t);
            m.w32(KERNEL + 0x54, n);
        }
        m.w32(KERNEL + 0x118, IN_PIPE);
        m.w32(KERNEL + 0x134, OUT_PIPE);
        m.w32(KERNEL + 0x200, 0x100);
        for (off, v) in LANG_US {
            m.w32(KERNEL + off, v);
        }
        m.w32(KERNEL + 0x484, CTX);
        lang_tables_init(&mut m);
        m.w16(LTS + 0x8c4, 0);
        m.w16(LTS + 0x436, 0x6f);
        m.w32(LTS + 0x8bc, 0);
        m.w32(LTS + 0x8c0, 0xffff_ffff);
        Lts { m }
    }

    pub fn push_input(&mut self, words: &[u16]) {
        self.m.pipe_in.extend(words.iter().copied());
    }

    /// Run the thread until it blocks on an empty input pipe; returns everything it wrote.
    pub fn run(&mut self) -> Vec<u16> {
        quiet_pipe_eof();
        let res = panic::catch_unwind(AssertUnwindSafe(|| main_loop(&mut self.m, CTX)));
        if let Err(e) = res {
            if e.downcast_ref::<PipeEof>().is_none() {
                panic::resume_unwind(e);
            }
        }
        let out = self.m.pipe_log.iter().filter(|w| w.pipe == OUT_PIPE).flat_map(|w| w.words.iter().copied()).collect();
        self.m.pipe_log.clear();
        out
    }
}

/// Keep the panic hook from reporting the unwind that ends a run on an empty input pipe.
pub fn quiet_pipe_eof() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let prev = panic::take_hook();
        panic::set_hook(Box::new(move |info| {
            if info.payload().downcast_ref::<PipeEof>().is_none() {
                prev(info);
            }
        }));
    });
}

/// FUN_1c00e7a0: letter-trigram language-guess descriptors (table, id) at 0x1c032270.
pub fn lang_tables_init(m: &mut Mem) {
    const DESC: [(u32, u32); 8] = [
        (0x1c05_9890, 7),
        (0x1c05_e810, 0),
        (0x1c03_c9a4, 3),
        (0x1c04_636c, 5),
        (0x1c03_2fdc, 1),
        (0x1c03_7cc0, 2),
        (0x1c05_4bac, 6),
        (0x1c04_1688, 4),
    ];
    for (k, (table, id)) in DESC.iter().enumerate() {
        let d = 0x1c03_2270 + k as u32 * 0x18;
        m.w32(d, *table);
        m.w32(d + 0x14, *id);
    }
}

/// FUN_1c00be90.
pub fn main_loop(m: &mut Mem, ctx: u32) {
    let kernel = m.r32(ctx + 8);
    let lts = m.r32(ctx + 0x10);
    let x = FRAME;
    let buf = lts + 0x914;
    let mut settle = 0;
    m.w32(lts + 0x8fc, 0);
    m.w32(lts + 0x404, 0);
    m.w32(lts + 0x90a, 0);
    m.w32(lts + 0x90e, 0);
    m.w16(lts + 0x912, 0);
    m.w32(lts + 0x900, 0);
    m.w32(lts + 0x904, 0);
    m.w16(lts + 0x908, 0);
    let mut node = x + 0x42c;
    while node < x + 0x162c {
        f_1c00be70(m, lts, node);
        node += 0x24;
    }
    m.w16(lts + 0x8c4, 0);
    m.w16(lts + 0x436, 0x6f);
    f_1c00f140(m, ctx);
    f_1c00c820(m, ctx, buf);
    m.w32(lts + 0x8bc, 0);
    m.w32(lts + 0x8c0, 0xffff_ffff);
    loop {
        if m.r16(kernel + 0x478) as u16 == 0x4fff {
            m.w16(kernel + 0x478, 0);
        }
        let n = m.r32(lts + 0x8fc);
        if n != 0 {
            m.w32(lts + 0x8fc, n - 1);
        }
        let f = m.r32(kernel + 0x20c);
        if f & 4 == 0 {
            settle = 0;
        } else if settle == 1 {
            settle = 0;
            m.w32(kernel + 0x20c, f & !4);
        } else {
            settle += 1;
        }
        let mut r = f_1c00caf0(m, ctx, lts);
        if r == 0 {
            m.w32(x + 0x14, buf);
            f_1c00cbc0(m, buf, x + 0x10, x + 0x28);
            if m.r32(x + 0x14) == m.r32(x + 0x10) {
                f_1c00c550(m, ctx, 0);
                f_1c00c820(m, ctx, buf);
                continue;
            }
            r = word_chain(m, ctx, kernel, lts);
        }
        match r {
            2 => {
                let (s, e) = (m.r32(x + 0x14), m.r32(x + 0x10));
                f_1c00ca00(m, ctx, s, e);
            }
            3 => {
                f_1c00ca50(m, ctx);
            }
            _ => {}
        }
    }
}

/// The per-word handler chain of FUN_1c00be90; the first nonzero result wins.
fn word_chain(m: &mut Mem, ctx: u32, kernel: u32, lts: u32) -> u32 {
    let x = FRAME;
    let buf = lts + 0x914;
    let start = |m: &Mem| m.r32(x + 0x14);
    let end = |m: &Mem| m.r32(x + 0x10);
    let (s, e) = (start(m), end(m));
    let r = f_1c00cbf0(m, ctx, s, e);
    if r != 0 {
        return r;
    }
    let r = f_1c00cc40(m, ctx, start(m), end(m));
    if r != 0 {
        return r;
    }
    let r = f_1c00cc90(m, kernel, start(m), end(m));
    if r != 0 {
        return r;
    }
    let r = f_1c00ccd0(m, ctx, start(m), end(m));
    if r != 0 {
        return r;
    }
    let e = end(m);
    let _ = e;
    f_1c00cde0(m, lts, x + 0x14);
    let s = start(m);
    f_1c00ce50(m, lts, s, x + 0x10);
    let (s, e, scan) = (start(m), end(m), m.r32(x + 0x28));
    let r = f_1c00cf30(m, ctx, s, e, buf, scan);
    if r != 0 {
        return r;
    }
    let s = start(m);
    let r = f_1c00cf90(m, ctx, s, x + 0x10);
    if r != 0 {
        return r;
    }
    let r = f_1c00d060(m, ctx, start(m), end(m));
    if r != 0 {
        return r;
    }
    let r = f_1c00d0e0(m, ctx, start(m), end(m));
    if r != 0 {
        return r;
    }
    f_1c00ed80(m, ctx, start(m), end(m));
    let saved: Vec<u8> = (0..0x400).map(|k| m.r8(buf + k)).collect();
    for (k, b) in saved.iter().enumerate() {
        m.w8(x + 0x2c + k as u32, *b);
    }
    f_1c00d2f0(m, start(m), end(m));
    let r = f_1c00d320(m, ctx, start(m), end(m));
    if r != 0 {
        return r;
    }
    let r = f_1c00d4a0(m, ctx, start(m), end(m), x + 0x20);
    if r != 0 {
        return r;
    }
    let r = f_1c00d540(m, ctx, m.r32(x + 0x20), end(m));
    if r != 0 {
        return r;
    }
    let r = f_1c00d8c0(m, ctx, start(m), end(m));
    if r != 0 {
        return r;
    }
    let r = f_1c00d9d0(m, ctx, m.r32(x + 0x20), end(m));
    if r != 0 {
        return r;
    }
    let r = f_1c00da20(m, ctx, start(m), end(m), m.r32(x + 0x20));
    if r != 0 {
        return r;
    }
    for k in 0..0x400 {
        let b = m.r8(x + 0x2c + k);
        m.w8(buf + k, b);
    }
    let r = f_1c00dd80(m, ctx, start(m), end(m));
    if r != 0 {
        return r;
    }
    f_1c00de20(m, ctx, start(m), end(m))
}
