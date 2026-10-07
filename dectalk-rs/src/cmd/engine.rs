//! Standalone CMD thread: own init (FUN_1c003460 / FUN_1c005810) and a resumable main loop.

use std::panic::{self, AssertUnwindSafe};

use super::*;
use crate::lts::map_dict;
use crate::ph::mem::{PipeEof, PipeWrite};

pub const CTX: u32 = 0x1200_0000;
pub const KERNEL: u32 = 0x1201_0000;
pub const CMD: u32 = 0x1202_0000;
/// Pipe ids CMD writes to (kernel+0xd0/0xd4/0xdc/0xe0) and reads from (kernel+0xd8).
pub const LTS_PIPE: u32 = 0x4c54_5300;
pub const PH_PIPE: u32 = 0x5048_0000;
pub const TEXT_PIPE: u32 = 0x434d_4400;
pub const VTM_PIPE: u32 = 0x5654_4d00;
pub const INDEX_PIPE: u32 = 0x4958_0000;
const MAIN_TABLE: u32 = 0x2000_0004;
const USER_TABLE: u32 = 0x3000_0004;

pub struct Cmd {
    pub m: Mem,
    run: i32,
}

impl Cmd {
    pub fn new(main_dict: &[u8], user_dict: Option<&[u8]>, rate: u32) -> Self {
        let mut m = Mem::new();
        m.map("ctx", CTX, &[0; 0xe8]);
        m.map("kernel", KERNEL, &[0; 0x488]);
        m.map("cmd", CMD, &[0; CMD_SIZE]);
        m.w32(CTX + 8, KERNEL);
        m.w32(CTX + 0xc, CMD);
        // Output to a wave file, error callbacks armed.
        m.w32(CTX + 0x84, 4);
        m.w32(CTX + 0x88, 1);
        m.w32(CTX + 0x44, 1);
        let (t, n) = map_dict(&mut m, "dict_main", main_dict, MAIN_TABLE);
        m.w32(KERNEL, t);
        m.w32(KERNEL + 0x1c, n);
        if let Some(u) = user_dict {
            let (t, n) = map_dict(&mut m, "dict_user", u, USER_TABLE);
            m.w32(KERNEL + 0x38, t);
            m.w32(KERNEL + 0x54, n);
        }
        for (off, pipe) in [(0xd0, LTS_PIPE), (0xd4, PH_PIPE), (0xd8, TEXT_PIPE), (0xdc, VTM_PIPE), (0xe0, INDEX_PIPE)] {
            m.w32(KERNEL + off, pipe);
        }
        m.w32(KERNEL + 0x118, LTS_PIPE);
        m.w32(KERNEL + 0x134, PH_PIPE);
        // LTS and PH threads already registered (FUN_1c024e90 bits 1 and 2).
        m.w32(KERNEL + 0xfc, 3);
        m.w32(KERNEL + 0xf8, 0xffff);
        m.w32(KERNEL + 0x200, 0x100);
        m.w32(KERNEL + 0x220, 0x00b4_0000);
        m.w32(KERNEL + 0x22c, rate);
        m.w32(KERNEL + 0x484, CTX);
        let params = m.malloc(41 * 4);
        m.w32(CMD + 0x2e0, params);
        let esc = m.malloc(0x5a);
        m.w32(CMD + 0x518, esc);
        f_1c005810(&mut m, CTX);
        m.w16(CMD + 0x51c, 0);
        f_1c005a60(&mut m, KERNEL);
        Cmd { m, run: 0 }
    }

    pub fn push_text(&mut self, bytes: &[u8]) {
        self.m.pipe_in.extend(bytes.iter().map(|&b| b as u16));
    }

    /// Process all queued input; returns every pipe write (and drain marker) in order.
    pub fn run(&mut self) -> Vec<PipeWrite> {
        crate::lts::engine::quiet_pipe_eof();
        let run = &mut self.run;
        let m = &mut self.m;
        let res = panic::catch_unwind(AssertUnwindSafe(|| cmd_loop(m, CTX, run)));
        let Err(e) = res;
        if e.downcast_ref::<PipeEof>().is_none() {
            panic::resume_unwind(e);
        }
        std::mem::take(&mut self.m.pipe_log)
    }
}
