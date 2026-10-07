//! Replays a dtoracle DT_LTSDUMP file through the Rust LTS handlers in order, diffing
//! ctx/kernel/lts/main-frame state and return values per call, then compares the concatenated
//! LTS->PH output with the DT_PIPEDUMP k+0x134 stream.
//!
//! usage: lts_replay <base>   (reads <base>.lts, <base>.pipe; dictionaries from ..\*.dic)

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::panic::{self, AssertUnwindSafe};
use std::process::ExitCode;

use dectalk_rs::lts::{self, Mem};
use dectalk_rs::ph::mem::PipeEof;

const SIZES: [usize; 4] = [0xe8, 0x488, 0x112c, 0x1630];
const NAMES: [&str; 4] = ["ctx", "kernel", "lts", "frame"];
// Thread-state words and the language table (0x454..0x478) set by the API thread during startup.
const MASK_KERNEL: [(usize, usize); 6] = [(0xf0, 0x100), (0x210, 0x214), (0x220, 0x22c), (0x238, 0x240), (0x250, 0x254), (0x454, 0x478)];
const MAIN_DICT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/dict/dtalk_nws_us.dic");
const USER_DICT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/dict/USER_NWS_US.DIC");

// Handlers whose EAX the main loop tests; the rest return garbage.
const RET_USED: [u32; 17] = [
    0xcaf0, 0xcbf0, 0xcc40, 0xcc90, 0xccd0, 0xcf30, 0xcf90, 0xd060, 0xd0e0, 0xd320, 0xd4a0, 0xd540,
    0xd8c0, 0xd9d0, 0xda20, 0xdd80, 0xde20,
];

struct Rec {
    rva: u32,
    args: [u32; 5],
    ret: u32,
    base: [u32; 4],
    pre: Vec<Vec<u8>>,
    post: Vec<Vec<u8>>,
}

fn u32_at(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(b[o..o + 4].try_into().unwrap())
}

fn records(path: &str) -> Vec<Rec> {
    let d = fs::read(path).expect("read lts dump");
    let blk: usize = SIZES.iter().sum();
    let mut out = Vec::new();
    let mut i = 0;
    // The last record may hold only its pre-state: that call blocked on the input pipe forever.
    while i + 45 + blk <= d.len() {
        assert_eq!(d[i], b'L');
        let h: Vec<u32> = (0..11).map(|k| u32_at(&d, i + 1 + k * 4)).collect();
        i += 45;
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
            base: [h[7], h[8], h[9], h[10]],
            pre,
            post,
        });
    }
    out
}

fn pipe_streams(path: &str) -> BTreeMap<u32, Vec<u16>> {
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
                if typ == 1 {
                    for k in 0..cnt {
                        raw.push((ptr, u16::from_le_bytes([d[i + 13 + k * 2], d[i + 14 + k * 2]])));
                    }
                }
                i += 13 + size * cnt;
            }
            t => panic!("bad pipe tag {t}"),
        }
    }
    let mut out: BTreeMap<u32, Vec<u16>> = BTreeMap::new();
    for (p, w) in raw {
        if let Some(&off) = pipes.get(&p) {
            out.entry(off).or_default().push(w);
        }
    }
    out
}

fn call(m: &mut Mem, rva: u32, a: [u32; 5]) -> u32 {
    match rva {
        0xf140 => lts::f_1c00f140(m, a[0]),
        0xc820 => lts::f_1c00c820(m, a[0], a[1]),
        0xcaf0 => lts::f_1c00caf0(m, a[0], a[1]),
        0xcbc0 => lts::f_1c00cbc0(m, a[0], a[1], a[2]),
        0xc550 => lts::f_1c00c550(m, a[0], a[1]),
        0xcbf0 => lts::f_1c00cbf0(m, a[0], a[1], a[2]),
        0xcc40 => lts::f_1c00cc40(m, a[0], a[1], a[2]),
        0xcc90 => lts::f_1c00cc90(m, a[0], a[1], a[2]),
        0xccd0 => lts::f_1c00ccd0(m, a[0], a[1], a[2]),
        0xcde0 => lts::f_1c00cde0(m, a[0], a[1]),
        0xce50 => lts::f_1c00ce50(m, a[0], a[1], a[2]),
        0xcf30 => lts::f_1c00cf30(m, a[0], a[1], a[2], a[3], a[4]),
        0xcf90 => lts::f_1c00cf90(m, a[0], a[1], a[2]),
        0xd060 => lts::f_1c00d060(m, a[0], a[1], a[2]),
        0xd0e0 => lts::f_1c00d0e0(m, a[0], a[1], a[2]),
        0xed80 => lts::f_1c00ed80(m, a[0], a[1], a[2]),
        0xd2f0 => lts::f_1c00d2f0(m, a[0], a[1]),
        0xd320 => lts::f_1c00d320(m, a[0], a[1], a[2]),
        0xd4a0 => lts::f_1c00d4a0(m, a[0], a[1], a[2], a[3]),
        0xd540 => lts::f_1c00d540(m, a[0], a[1], a[2]),
        0xd8c0 => lts::f_1c00d8c0(m, a[0], a[1], a[2]),
        0xd9d0 => lts::f_1c00d9d0(m, a[0], a[1], a[2]),
        0xda20 => lts::f_1c00da20(m, a[0], a[1], a[2], a[3]),
        0xdd80 => lts::f_1c00dd80(m, a[0], a[1], a[2]),
        0xde20 => lts::f_1c00de20(m, a[0], a[1], a[2]),
        0xca50 => lts::f_1c00ca50(m, a[0]),
        0xca00 => lts::f_1c00ca00(m, a[0], a[1], a[2]),
        _ => panic!("no dispatch for {rva:05x}"),
    }
}

fn diff(name: &str, got: &[u8], want: &[u8]) -> Vec<String> {
    let mut out = Vec::new();
    let mut o = 0;
    while o < want.len() {
        if got[o] != want[o] {
            let a = o & !3;
            out.push(format!(
                "  {name}+{a:#06x}: got {:08x} want {:08x}",
                u32_at(got, a),
                u32_at(want, a)
            ));
            o = a + 4;
        } else {
            o += 1;
        }
    }
    out
}

fn main() -> ExitCode {
    let a: Vec<String> = env::args().collect();
    let base = a.get(1).cloned().unwrap_or_else(|| concat!(env!("CARGO_MANIFEST_DIR"), "/../oracle/test1").into());
    let max_report: usize = a.get(2).and_then(|s| s.parse().ok()).unwrap_or(3);
    let recs = records(&format!("{base}.lts"));
    let streams = pipe_streams(&format!("{base}.pipe"));
    // Shutdown injects 0x000b sync words straight into both pipes.
    let mut input = streams.get(&0x118).cloned().unwrap_or_default();
    while input.last() == Some(&0xb) {
        input.pop();
    }
    let mut want_out = streams.get(&0x134).cloned().unwrap_or_default();
    while want_out.last() == Some(&0xb) {
        want_out.pop();
    }
    println!("{} records, {} input words, {} output words", recs.len(), input.len(), want_out.len());

    let mut m = Mem::new();
    let k0 = &recs[0].pre[1];
    let main = fs::read(MAIN_DICT).expect("main dict");
    let user = fs::read(USER_DICT).expect("user dict");
    lts::map_dict(&mut m, "dict_main", &main, u32_at(k0, 0));
    lts::map_dict(&mut m, "dict_user", &user, u32_at(k0, 0x38));
    m.pipe_in.extend(input.iter().copied());
    panic::set_hook(Box::new(|info| {
        if info.payload().downcast_ref::<PipeEof>().is_none() {
            eprintln!("{info}");
        }
    }));

    let mut bad = 0;
    let mut counts: BTreeMap<u32, (u32, u32)> = BTreeMap::new();
    for (n, r) in recs.iter().enumerate() {
        for k in 0..4 {
            m.map(NAMES[k], r.base[k], &r.pre[k]);
        }
        let res = panic::catch_unwind(AssertUnwindSafe(|| call(&mut m, r.rva, r.args)));
        let ent = counts.entry(r.rva).or_default();
        ent.0 += 1;
        let mut msgs = Vec::new();
        match res {
            Ok(_) if r.post.is_empty() => msgs.push("  returned, but the original blocked on input".into()),
            Err(e) if r.post.is_empty() && e.downcast_ref::<PipeEof>().is_some() => {}
            Ok(ret) => {
                if RET_USED.contains(&r.rva) && ret != r.ret {
                    msgs.push(format!("  ret: got {ret} want {}", r.ret));
                }
                // ctx and parts of the kernel are written concurrently by the API/PH/VTM threads.
                for k in 1..4 {
                    let (mut got, mut want) = (m.region(NAMES[k]).bytes.clone(), r.post[k].clone());
                    if k == 1 {
                        for &(lo, hi) in &MASK_KERNEL {
                            got[lo..hi].fill(0);
                            want[lo..hi].fill(0);
                        }
                    }
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
                for s in msgs.iter().take(12) {
                    println!("{s}");
                }
            }
        }
    }
    println!("per function (calls, bad):");
    for (rva, (c, b)) in &counts {
        println!("  {rva:05x}: {c:5} {b:5}");
    }
    let got_out: Vec<u16> = m.pipe_log.iter().flat_map(|w| w.words.iter().copied()).collect();
    let n = got_out.len().min(want_out.len());
    let first = (0..n).find(|&k| got_out[k] != want_out[k]);
    println!("output: rust {} oracle {} first mismatch {:?}", got_out.len(), want_out.len(), first);
    println!("  unread input {:04x?}", m.pipe_in);
    println!("  rust tail   {:04x?}", &got_out[got_out.len().saturating_sub(10)..]);
    println!("  oracle tail {:04x?}", &want_out[want_out.len().saturating_sub(10)..]);
    if let Some(k) = first {
        let lo = k.saturating_sub(4);
        println!("  got  {:04x?}", &got_out[lo..(k + 8).min(got_out.len())]);
        println!("  want {:04x?}", &want_out[lo..(k + 8).min(want_out.len())]);
    }
    if bad == 0 && first.is_none() && got_out.len() == want_out.len() { ExitCode::SUCCESS } else { ExitCode::FAILURE }
}
