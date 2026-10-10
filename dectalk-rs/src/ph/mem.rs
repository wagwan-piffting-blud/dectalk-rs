//! Flat 32-bit address space mirroring the original process layout, so PH code can be
//! transliterated with its pointer arithmetic intact and diffed against oracle dumps.

use std::sync::OnceLock;

use crate::tables;

fn dll_image() -> &'static [u8] {
    static IMG: OnceLock<Vec<u8>> = OnceLock::new();
    IMG.get_or_init(tables::image)
}

#[cfg(feature = "strict-tables")]
fn dll_covered() -> &'static [bool] {
    static COV: OnceLock<Vec<bool>> = OnceLock::new();
    COV.get_or_init(tables::covered)
}

const HEAP_BASE: u32 = 0x7000_0000;
const HEAP_STRIDE: u32 = 0x1000;

fn heap_span(size: u32) -> u32 {
    HEAP_STRIDE.max((size + 0xfff) & !0xfff)
}

pub struct Region {
    pub name: &'static str,
    pub base: u32,
    pub bytes: Vec<u8>,
}

pub struct PipeWrite {
    pub pipe: u32,
    pub words: Vec<u16>,
}

pub struct Mem {
    pub regions: Vec<Region>,
    heap_next: u32,
    heap_free: Vec<(u32, u32)>,
    pub pipe_log: Vec<PipeWrite>,
    pub pipe_in: std::collections::VecDeque<u16>,
    /// Refills `pipe_in` when it runs dry (called with the Mem so it can hand off `pipe_log`).
    pub source: Option<Box<InputSource>>,
    sp: u32,
}

impl Mem {
    pub fn new() -> Self {
        Mem {
            regions: vec![Region { name: "dll", base: tables::DATA_BASE, bytes: dll_image().to_vec() }],
            heap_next: HEAP_BASE,
            heap_free: Vec::new(),
            pipe_log: Vec::new(),
            pipe_in: Default::default(),
            source: None,
            sp: 0,
        }
    }

    pub fn map(&mut self, name: &'static str, base: u32, bytes: &[u8]) {
        self.regions.retain(|r| r.name != name);
        self.regions.push(Region { name, base, bytes: bytes.to_vec() });
    }

    pub fn region(&self, name: &str) -> &Region {
        self.regions.iter().find(|r| r.name == name).unwrap()
    }

    fn locate(&self, addr: u32, len: u32) -> (usize, usize) {
        for (i, r) in self.regions.iter().enumerate() {
            if addr >= r.base && addr.wrapping_sub(r.base) + len <= r.bytes.len() as u32 {
                let o = (addr - r.base) as usize;
                #[cfg(feature = "strict-tables")]
                if r.name == "dll" && dll_covered()[o..o + len as usize].iter().any(|&c| !c) {
                    panic!("access to uncovered dll data {addr:#010x} len {len}");
                }
                return (i, o);
            }
        }
        panic!("unmapped access {addr:#010x} len {len}");
    }

    pub fn r8(&self, a: u32) -> u8 {
        let (i, o) = self.locate(a, 1);
        self.regions[i].bytes[o]
    }

    pub fn r16(&self, a: u32) -> i16 {
        let (i, o) = self.locate(a, 2);
        let b = &self.regions[i].bytes;
        i16::from_le_bytes([b[o], b[o + 1]])
    }

    pub fn r32(&self, a: u32) -> u32 {
        let (i, o) = self.locate(a, 4);
        let b = &self.regions[i].bytes;
        u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
    }

    pub fn w8(&mut self, a: u32, v: u8) {
        let (i, o) = self.locate(a, 1);
        self.regions[i].bytes[o] = v;
    }

    pub fn w16(&mut self, a: u32, v: i32) {
        let (i, o) = self.locate(a, 2);
        self.regions[i].bytes[o..o + 2].copy_from_slice(&(v as i16).to_le_bytes());
    }

    pub fn w32(&mut self, a: u32, v: u32) {
        let (i, o) = self.locate(a, 4);
        self.regions[i].bytes[o..o + 4].copy_from_slice(&v.to_le_bytes());
    }

    /// Sign-extended 16-bit read, the common case in transliterated code.
    #[inline]
    pub fn s(&self, a: u32) -> i32 {
        self.r16(a) as i32
    }

    #[inline]
    pub fn i(&self, a: u32) -> i32 {
        self.r32(a) as i32
    }

    pub fn malloc(&mut self, size: u32) -> u32 {
        let span = heap_span(size);
        // Freed spans are reused: PH allocates one message per frame, and a bump pointer alone
        // wraps the 32-bit space after ~650k frames, landing new blocks on top of ctx/ph.
        let base = match self.heap_free.iter().rposition(|&(_, s)| s == span) {
            Some(i) => self.heap_free.swap_remove(i).0,
            None => {
                let base = self.heap_next;
                self.heap_next = base.checked_add(span).expect("guest heap exhausted");
                base
            }
        };
        self.regions.push(Region { name: "heap", base, bytes: vec![0xcd; size as usize] });
        base
    }

    pub fn free(&mut self, addr: u32) {
        if let Some(i) = self.regions.iter().position(|r| r.name == "heap" && r.base == addr) {
            let r = self.regions.remove(i);
            self.heap_free.push((r.base, heap_span(r.bytes.len() as u32)));
        }
    }

    /// FUN_1c021280 for word pipes: record instead of queueing.
    pub fn pipe_write(&mut self, pipe: u32, src: u32, count: u32) {
        let words = (0..count).map(|k| self.r16(src + k * 2) as u16).collect();
        self.pipe_log.push(PipeWrite { pipe, words });
    }

    pub fn pipe_write_words(&mut self, pipe: u32, words: &[u16]) {
        self.pipe_log.push(PipeWrite { pipe, words: words.to_vec() });
    }

    /// Next input element. On an empty queue the `source` (if any) is asked to refill it — it may
    /// block, like the original reader thread; without more input the reader's thread of control
    /// ends by unwinding with `PipeEof`, caught by the driver.
    fn next_input(&mut self) -> u16 {
        loop {
            if let Some(w) = self.pipe_in.pop_front() {
                return w;
            }
            let Some(mut src) = self.source.take() else {
                std::panic::resume_unwind(Box::new(PipeEof));
            };
            let more = src(self);
            self.source = Some(src);
            match more {
                Some(words) if !words.is_empty() => self.pipe_in.extend(words),
                _ => std::panic::resume_unwind(Box::new(PipeEof)),
            }
        }
    }

    /// FUN_1c0217b0 for word pipes.
    pub fn pipe_read(&mut self, _pipe: u32, dst: u32, count: u32) {
        for k in 0..count {
            let w = self.next_input();
            self.w16(dst + k * 2, w as i32);
        }
    }

    /// FUN_1c0217b0 for byte pipes (the API -> CMD text pipe).
    pub fn pipe_read_bytes(&mut self, _pipe: u32, dst: u32, count: u32) {
        for k in 0..count {
            let w = self.next_input();
            self.w8(dst + k, w as u8);
        }
    }

    /// FUN_1c0282a0: the writer waits until every downstream pipe is empty. Recorded as an
    /// empty write to `DRAIN_PIPE` so the pipeline driver can flush the later stages there.
    pub fn drain_downstream(&mut self) {
        self.pipe_log.push(PipeWrite { pipe: DRAIN_PIPE, words: Vec::new() });
    }

    pub fn memmove(&mut self, dst: u32, src: u32, n: u32) {
        let tmp: Vec<u8> = (0..n).map(|k| self.r8(src + k)).collect();
        for (k, b) in tmp.into_iter().enumerate() {
            self.w8(dst + k as u32, b);
        }
    }

    /// Scratch stack for locals whose address escapes into other functions.
    pub fn sp(&self) -> u32 {
        self.sp
    }

    pub fn set_sp(&mut self, sp: u32) {
        self.sp = sp;
    }

    pub fn alloca(&mut self, size: u32) -> u32 {
        let size = (size + 3) & !3;
        if self.sp == 0 {
            // sp is restored to 0 between calls; the region is reused, not remapped, so stale
            // contents persist exactly as they would on the original's stack.
            if !self.regions.iter().any(|r| r.name == "stack") {
                self.regions.push(Region { name: "stack", base: STACK_BASE, bytes: vec![0; STACK_SIZE as usize] });
            }
            self.sp = STACK_BASE + STACK_SIZE;
        }
        assert!(self.sp - STACK_BASE >= size, "scratch stack overflow");
        self.sp -= size;
        self.sp
    }
}

pub struct PipeEof;

pub type InputSource = dyn FnMut(&mut Mem) -> Option<Vec<u16>> + Send;

pub const DRAIN_PIPE: u32 = 0xffff_fffe;

const STACK_BASE: u32 = 0x0f00_0000;
const STACK_SIZE: u32 = 0x0100_0000;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heap_reuses_freed_space() {
        let mut m = Mem::new();
        for _ in 0..1_000_000 {
            let p = m.malloc(0x30);
            m.free(p);
        }
        assert_eq!(m.heap_next, HEAP_BASE + HEAP_STRIDE);
    }
}
