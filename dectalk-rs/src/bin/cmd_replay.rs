//! Replays a dtoracle DT_CMDDUMP file through the Rust CMD dispatch functions in order,
//! diffing ctx/kernel/cmd/params/escape state per call, then compares CMD's pipe output
//! (the LTS word stream and tone messages) with the DT_PIPEDUMP streams.
//!
//! usage: cmd_replay <base> [max_reports]   (reads <base>.cmd, <base>.pipe)

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::panic::{self, AssertUnwindSafe};
use std::process::ExitCode;

use dectalk_rs::cmd::{self, Mem};
use dectalk_rs::ph::mem::{PipeEof, DRAIN_PIPE};

const SIZES: [usize; 5] = [0xe8, 0x488, 0x3244, 0xa4, 0x5a];
const NAMES: [&str; 5] = ["ctx", "kernel", "cmd", "params", "esc"];
const HDR: usize = 1 + 12 * 4;
// Written concurrently by the API, LTS, PH and VTM threads.
const MASK_KERNEL: [(usize, usize); 6] = [(0xf0, 0x100), (0x200, 0x204), (0x20c, 0x210), (0x220, 0x22c), (0x238, 0x240), (0x250, 0x254)];
const MASK_CTX: [(usize, usize); 1] = [(0, 0xe8)];
const MAIN_DICT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/dict/dtalk_nws_us.dic");
const USER_DICT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/dict/USER_NWS_US.DIC");

struct Rec {
    rva: u32,
    args: [u32; 5],
    ret: u32,
    base: [u32; 5],
    pre: Vec<Vec<u8>>,
    post: Vec<Vec<u8>>,
}

fn u32_at(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(b[o..o + 4].try_into().unwrap())
}

fn records(path: &str) -> Vec<Rec> {
    let d = fs::read(path).expect("read cmd dump");
    let blk: usize = SIZES.iter().sum();
    let mut out = Vec::new();
    let mut i = 0;
    while i + HDR + blk <= d.len() {
        assert_eq!(d[i], b'C', "bad record at {i}");
        let h: Vec<u32> = (0..12).map(|k| u32_at(&d, i + 1 + k * 4)).collect();
        i += HDR;
        let complete = i + 2 * blk <= d.len();
        let mut take = || {
            SIZES
                .iter()
                .map(|&s| {
                    let v = d[i..i + s].to_vec();
                    i += s;
                    v
                })
                .collect::<Vec<_>>()
        };
        let pre = take();
        let post = if complete { take() } else { Vec::new() };
        out.push(Rec {
            rva: h[0],
            args: [h[1], h[2], h[3], h[4], h[5]],
            ret: h[6],
            base: [h[7], h[8], h[9], h[10], h[11]],
            pre,
            post,
        });
    }
    out
}

/// (pipe pointer -> kernel offset, per-offset list of writes)
fn pipe_writes(path: &str) -> (BTreeMap<u32, u32>, BTreeMap<u32, Vec<Vec<u16>>>) {
    let d = fs::read(path).expect("read pipe dump");
    let mut pipes = BTreeMap::new();
    let mut raw = Vec::new();
    let mut i = 0;
    while i < d.len() {
        match d[i] {
            b'P' => {
                pipes.insert(u32_at(&d, i + 5), u32_at(&d, i + 1));
                i += 9;
            }
            b'W' => {
                let (ptr, typ, cnt) = (u32_at(&d, i + 1), u32_at(&d, i + 5), u32_at(&d, i + 9) as usize);
                let size = match typ { 0 => 1, 1 => 2, 5 => 8, _ => 4 };
                let vals = (0..cnt)
                    .map(|k| match size {
                        1 => d[i + 13 + k] as u16,
                        _ => u16::from_le_bytes([d[i + 13 + k * size], d[i + 14 + k * size]]),
                    })
                    .collect();
                raw.push((ptr, vals));
                i += 13 + size * cnt;
            }
            t => panic!("bad pipe tag {t}"),
        }
    }
    let mut out: BTreeMap<u32, Vec<Vec<u16>>> = BTreeMap::new();
    for (p, w) in raw {
        if let Some(&off) = pipes.get(&p) {
            out.entry(off).or_default().push(w);
        }
    }
    (pipes, out)
}

fn call(m: &mut Mem, rva: u32, a: [u32; 5]) -> u32 {
    match rva {
        0x3a30 => cmd::f_1c003a30(m, a[0]),
        0x3a80 => cmd::f_1c003a80(m, a[0]),
        0x39e0 => cmd::f_1c0039e0(m, a[0], a[1]),
        0x3850 => cmd::f_1c003850(m, a[0], a[1]),
        0x1870 => cmd::f_1c001870(m, a[0], a[1]),
        0x18f0 => cmd::f_1c0018f0(m, a[0], a[1]),
        0x3ed0 => cmd::f_1c003ed0(m, a[0], a[1]),
        0x1000 => cmd::f_1c001000(m, a[0], a[1]),
        0x12c0 => cmd::f_1c0012c0(m, a[0], a[1]),
        _ => panic!("no dispatch for {rva:05x}"),
    }
}

fn diff(name: &str, got: &[u8], want: &[u8]) -> Vec<String> {
    let mut out = Vec::new();
    let mut o = 0;
    while o < want.len() {
        if got[o] != want[o] {
            let a = o & !3;
            let g = u32_at(&[got, &[0; 4]].concat(), a);
            let w = u32_at(&[want, &[0; 4]].concat(), a);
            out.push(format!("  {name}+{a:#06x}: got {g:08x} want {w:08x}"));
            o = a + 4;
        } else {
            o += 1;
        }
    }
    out
}

fn masked(v: &[u8], masks: &[(usize, usize)]) -> Vec<u8> {
    let mut v = v.to_vec();
    for &(lo, hi) in masks {
        v[lo..hi].fill(0);
    }
    v
}

fn first_diff(name: &str, got: &[u16], want: &[u16]) -> bool {
    let n = got.len().min(want.len());
    let bad = (0..n).find(|&k| got[k] != want[k]);
    println!("{name}: rust {} oracle {} first mismatch {:?}", got.len(), want.len(), bad);
    if let Some(k) = bad {
        let lo = k.saturating_sub(6);
        println!("  got  {:04x?}", &got[lo..(k + 10).min(got.len())]);
        println!("  want {:04x?}", &want[lo..(k + 10).min(want.len())]);
    }
    bad.is_none() && got.len() == want.len()
}

fn main() -> ExitCode {
    let a: Vec<String> = env::args().collect();
    let base = a.get(1).cloned().unwrap_or_else(|| concat!(env!("CARGO_MANIFEST_DIR"), "/../oracle/test1").into());
    let max_report: usize = a.get(2).and_then(|s| s.parse().ok()).unwrap_or(3);
    let recs = records(&format!("{base}.cmd"));
    // The pipe dump must come from the same run as the CMD dump (pipe pointers differ per run).
    let pipe_path = [format!("{base}c.pipe"), format!("{base}.pipe")].into_iter().find(|p| fs::metadata(p).is_ok()).unwrap();
    let (ptr_off, streams) = pipe_writes(&pipe_path);
    let mut input: Vec<u16> = streams.get(&0xd8).into_iter().flatten().flatten().copied().collect();
    while input.last() == Some(&0xb) {
        input.pop();
    }
    let mut want_lts: Vec<u16> = streams.get(&0x118).into_iter().flatten().flatten().copied().collect();
    while want_lts.last() == Some(&0xb) {
        want_lts.pop();
    }
    let want_tones: Vec<u16> =
        streams.get(&0xdc).into_iter().flatten().filter(|w| w.len() == 6 && w[0] == 2).flatten().copied().collect();
    println!("{} records, {} input bytes", recs.len(), input.len());
    if a.iter().any(|s| s == "--standalone") {
        // Fresh standalone CMD over the same input. The oracle stream starts with the API's
        // pipeline-reset words (1f03 1f0a 20) and one API sync (0b 1f0a 20).
        let main = fs::read(MAIN_DICT).expect("main dict");
        let user = fs::read(USER_DICT).expect("user dict");
        let mut c = dectalk_rs::cmd::engine::Cmd::new(&main, Some(&user), 11025);
        if let Some(spec) = a.iter().find_map(|s| s.strip_prefix("--transplant=")) {
            // e.g. --transplant=cmd:0x138:0x158,kernel:0x230:0x240
            use dectalk_rs::cmd::engine::{CMD, CTX, KERNEL};
            for part in spec.split(',') {
                let f: Vec<&str> = part.split(':').collect();
                let (lo, hi) = (u32::from_str_radix(&f[1][2..], 16).unwrap(), u32::from_str_radix(&f[2][2..], 16).unwrap());
                let (base, k) = match f[0] {
                    "ctx" => (CTX, 0),
                    "kernel" => (KERNEL, 1),
                    _ => (CMD, 2),
                };
                for o in lo..hi {
                    c.m.w8(base + o, recs[0].pre[k][o as usize]);
                }
            }
        }
        if a.iter().any(|s| s == "--init-diff") {
            use dectalk_rs::cmd::engine::{CMD, KERNEL};
            let blocks = [
                ("kernel", KERNEL, 0x488usize, 1usize),
                ("cmd", CMD, 0x3244, 2),
                ("params", c.m.r32(CMD + 0x2e0), 0xa4, 3),
                ("esc", c.m.r32(CMD + 0x518), 0x5a, 4),
            ];
            for (name, base, len, k) in blocks {
                let ours: Vec<u8> = (0..len as u32).map(|o| c.m.r8(base + o)).collect();
                for s in diff(name, &ours, &recs[0].pre[k]) {
                    println!("{s}");
                }
            }
        }
        let bytes: Vec<u8> = input.iter().map(|&w| w as u8).collect();
        c.push_text(&bytes);
        let log = c.run();
        let got: Vec<u16> = log
            .iter()
            .filter(|w| w.pipe == dectalk_rs::cmd::engine::LTS_PIPE)
            .flat_map(|w| w.words.iter().copied())
            .collect();
        // Startup/shutdown syncs depend on API thread timing; compare the text portion.
        let trim = |v: &[u16]| -> Vec<u16> {
            let sync = |w: &u16| matches!(*w, 0xb | 0x1f0a | 0x20 | 0x1f03);
            let lo = v.iter().position(|w| !sync(w)).unwrap_or(v.len());
            let hi = v.iter().rposition(|w| !sync(w)).map_or(lo, |k| k + 1);
            v[lo..hi].to_vec()
        };
        let ok = first_diff("standalone lts words", &trim(&got), &trim(&want_lts));
        return if ok { ExitCode::SUCCESS } else { ExitCode::FAILURE };
    }

    let mut m = Mem::new();
    m.pipe_in.extend(input.iter().copied());
    panic::set_hook(Box::new(|info| {
        if info.payload().downcast_ref::<PipeEof>().is_none() {
            eprintln!("{info}");
            if env::var_os("REPLAY_BT").is_some() {
                eprintln!("{}", std::backtrace::Backtrace::force_capture());
            }
        }
    }));

    let mut bad = 0;
    let mut counts: BTreeMap<u32, (u32, u32)> = BTreeMap::new();
    let k0 = &recs[0].pre[1];
    let main = fs::read(MAIN_DICT).expect("main dict");
    let user = fs::read(USER_DICT).expect("user dict");
    dectalk_rs::lts::map_dict(&mut m, "dict_main", &main, u32_at(k0, 0));
    dectalk_rs::lts::map_dict(&mut m, "dict_user", &user, u32_at(k0, 0x38));
    if a.iter().any(|s| s == "--rules") {
        // Replay each FUN_1c010c00 (text-rule engine) call from <base>.cmd.d in isolation.
        let d = fs::read(format!("{base}.cmd.d")).expect("read .cmd.d");
        let blk: usize = SIZES.iter().sum();
        let main = fs::read(MAIN_DICT).expect("main dict");
        let user = fs::read(USER_DICT).expect("user dict");
        let (mut i, mut n, mut bad) = (0usize, 0, 0);
        while i + 81 + 2 * blk <= d.len() {
            assert_eq!(d[i], b'D');
            let h: Vec<u32> = (0..20).map(|k| u32_at(&d, i + 1 + k * 4)).collect();
            i += 81;
            let mut take = || {
                SIZES
                    .iter()
                    .map(|&s| {
                        let v = d[i..i + s].to_vec();
                        i += s;
                        v
                    })
                    .collect::<Vec<_>>()
            };
            let pre = take();
            let post = take();
            let base_addrs = &h[15..20];
            let mut m = Mem::new();
            dectalk_rs::lts::map_dict(&mut m, "dict_main", &main, u32_at(&pre[1], 0));
            dectalk_rs::lts::map_dict(&mut m, "dict_user", &user, u32_at(&pre[1], 0x38));
            for k in 0..5 {
                m.map(NAMES[k], base_addrs[k], &pre[k]);
            }
            let g = &h[1..14];
            let res = panic::catch_unwind(AssertUnwindSafe(|| {
                cmd::f_1c010c00(&mut m, g[0], g[1], g[2], g[3], g[4], g[5], g[6], g[7], g[8], g[9], g[10], g[11], g[12])
            }));
            let mut msgs = Vec::new();
            match res {
                Ok(ret) if ret != h[14] => msgs.push(format!("  ret: got {ret:08x} want {:08x}", h[14])),
                Ok(_) => {}
                Err(_) => msgs.push("  panic".into()),
            }
            for k in 2..5 {
                msgs.extend(diff(NAMES[k], &m.region(NAMES[k]).bytes, &post[k]));
            }
            if !msgs.is_empty() {
                bad += 1;
                if bad <= max_report {
                    let cmdb = base_addrs[2];
                    let text: String = (0..80u32)
                        .map(|o| pre[2][(g[1] - cmdb + o) as usize])
                        .take_while(|&b| b != 0)
                        .map(|b| b as char)
                        .collect();
                    println!("rule call {n} pass {} flags {:x} text {:?}:", g[10], g[9], text);
                    for s in msgs.iter().take(12) {
                        println!("{s}");
                    }
                }
            }
            n += 1;
        }
        println!("rule calls: {n}, bad {bad}");
        return if bad == 0 { ExitCode::SUCCESS } else { ExitCode::FAILURE };
    }
    if a.iter().any(|s| s == "--loop") {
        // The Rust main loop from the DLL's initial state, over the full input.
        let r = &recs[0];
        for k in 0..5 {
            m.map(NAMES[k], r.base[k], &r.pre[k]);
        }
        let mut run = 0;
        let _ = panic::catch_unwind(AssertUnwindSafe(|| cmd::cmd_loop(&mut m, r.base[0], &mut run)));
        let got: Vec<u16> = m
            .pipe_log
            .iter()
            .filter(|w| matches!(ptr_off.get(&w.pipe), Some(0x118) | Some(0xd0)))
            .flat_map(|w| w.words.iter().copied())
            .collect();
        let trim = |v: &[u16]| -> Vec<u16> {
            let sync = |w: &u16| matches!(*w, 0xb | 0x1f0a | 0x20 | 0x1f03);
            let lo = v.iter().position(|w| !sync(w)).unwrap_or(v.len());
            let hi = v.iter().rposition(|w| !sync(w)).map_or(lo, |k| k + 1);
            v[lo..hi].to_vec()
        };
        let ok = first_diff("loop lts words", &trim(&got), &trim(&want_lts));
        return if ok { ExitCode::SUCCESS } else { ExitCode::FAILURE };
    }
    let carry = a.iter().any(|s| s == "--carry");
    for (n, r) in recs.iter().enumerate() {
        if n == 0 || !carry {
            for k in 0..5 {
                m.map(NAMES[k], r.base[k], &r.pre[k]);
            }
        } else {
            // Carry the Rust state, except what the (unhooked) main loop body and the other
            // threads write between calls.
            let (kernel, cmd) = (r.base[1], r.base[2]);
            if env::var_os("CARRY_NO_CMD").is_none() {
                for off in [0x2e8u32, 0x2ec, 0x51c] {
                    m.w32(cmd + off, u32_at(&r.pre[2], off as usize));
                }
            }
            let forced: &[(usize, usize)] = if a.iter().any(|s| s == "--strict") { &[(0x16c, 0x178)] } else { &MASK_KERNEL };
            for &(lo, hi) in forced.iter().chain([(0x16c, 0x178)].iter()) {
                for o in lo..hi {
                    m.w8(kernel + o as u32, r.pre[1][o]);
                }
            }
            if env::var_os("CARRY_NO_CTX").is_none() {
                m.map("ctx", r.base[0], &r.pre[0]);
            }
        }
        // The API thread flips its reset flags while the reader is blocked on the pipe; the
        // reader sees the values current when the byte arrives.
        if r.rva == 0x3a80 && !r.post.is_empty() {
            let kernel = r.base[1];
            for off in [0x16c, 0x170, 0x174] {
                m.w32(kernel + off, u32_at(&r.post[1], off as usize));
            }
        }
        let res = panic::catch_unwind(AssertUnwindSafe(|| call(&mut m, r.rva, r.args)));
        let ent = counts.entry(r.rva).or_default();
        ent.0 += 1;
        let mut msgs = Vec::new();
        match res {
            Ok(_) if r.post.is_empty() => msgs.push("  returned, but the original blocked on input".into()),
            Err(e) if r.post.is_empty() && e.downcast_ref::<PipeEof>().is_some() => {}
            Ok(ret) => {
                if r.rva == 0x3a80 || r.rva == 0x3a30 {
                    if ret as u16 != r.ret as u16 {
                        msgs.push(format!("  ret: got {ret:#x} want {:#x}", r.ret));
                    }
                }
                for k in 0..5 {
                    let masks: &[(usize, usize)] = match k {
                        0 => &MASK_CTX,
                        1 if a.iter().any(|s| s == "--strict") => &[(0xf0, 0x100), (0x238, 0x240)],
                        1 => &MASK_KERNEL,
                        _ => &[],
                    };
                    let got = masked(&m.region(NAMES[k]).bytes, masks);
                    let want = masked(&r.post[k], masks);
                    msgs.extend(diff(NAMES[k], &got, &want));
                }
            }
            Err(e) => {
                let why = e
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string()))
                    .unwrap_or_else(|| "pipe eof / unknown panic".into());
                msgs.push(format!("  panic: {why}"));
            }
        }
        if !msgs.is_empty() {
            ent.1 += 1;
            bad += 1;
            if bad <= max_report {
                println!("record {n} FUN_1c00{:04x} args {:08x?}:", r.rva, r.args);
                for s in msgs.iter().take(14) {
                    println!("{s}");
                }
                if msgs.len() > 14 {
                    println!("  ... {} more", msgs.len() - 14);
                }
            }
            // Resync to the oracle's post-state so one bug doesn't cascade.
            if !r.post.is_empty() && !carry {
                for k in 0..5 {
                    m.map(NAMES[k], r.base[k], &r.post[k]);
                }
            }
        }
    }
    println!("per function (calls, bad):");
    for (rva, (c, b)) in &counts {
        println!("  {rva:05x}: {c:6} {b:6}");
    }
    let mut got_lts = Vec::new();
    let mut got_tones = Vec::new();
    let mut drains = 0;
    for w in &m.pipe_log {
        if w.pipe == DRAIN_PIPE {
            drains += 1;
            continue;
        }
        match ptr_off.get(&w.pipe) {
            Some(0x118) | Some(0xd0) => got_lts.extend(&w.words),
            Some(0xdc) => got_tones.extend(&w.words),
            other => println!("  write to unexpected pipe {:08x} ({other:?}): {:04x?}", w.pipe, w.words),
        }
    }
    println!("drain waits: {drains}");
    let trim = |v: &[u16]| -> Vec<u16> {
        let sync = |w: &u16| matches!(*w, 0xb | 0x1f0a | 0x20 | 0x1f03);
        let lo = v.iter().position(|w| !sync(w)).unwrap_or(v.len());
        let hi = v.iter().rposition(|w| !sync(w)).map_or(lo, |k| k + 1);
        v[lo..hi].to_vec()
    };
    let ok_lts = first_diff("lts words (sync-trimmed)", &trim(&got_lts), &trim(&want_lts));
    let ok_tones = first_diff("tone words", &got_tones, &want_tones);
    if bad == 0 && ok_lts && ok_tones { ExitCode::SUCCESS } else { ExitCode::FAILURE }
}
