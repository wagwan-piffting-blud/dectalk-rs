//! Replays a dtoracle DT_PHDUMP file through the Rust PH stages, diffing ctx/kernel/PH/aux
//! state per stage call, and compares emitted frame messages with the DT_PIPEDUMP VTM stream.

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::process::ExitCode;

use dectalk_rs::ph::mem::Mem;
use dectalk_rs::ph::{self, AUX_SIZE, CTX_SIZE, KERNEL_SIZE, PH_SIZE};

const BLK: usize = CTX_SIZE + KERNEL_SIZE + PH_SIZE + AUX_SIZE;
const REC: usize = 2 + 20 + 2 * BLK;
// Heap pointer owned by frame_emit; the Rust heap uses different addresses.
const MASK_PH: [(usize, usize); 1] = [(0x282c, 0x2830)];
// Written concurrently by the VTM thread (busy flags).
const MASK_KERNEL: [(usize, usize); 2] = [(0xec, 0xf4), (0x224, 0x226)];
// Audio sample counters advanced by the VTM/output side.
const MASK_CTX: [(usize, usize); 2] = [(0x78, 0x80), (0x40, 0x50)];

struct Blocks<'a> {
    ctx: &'a [u8],
    kernel: &'a [u8],
    ph: &'a [u8],
    aux: &'a [u8],
}

fn split(b: &[u8]) -> Blocks<'_> {
    let (ctx, r) = b.split_at(CTX_SIZE);
    let (kernel, r) = r.split_at(KERNEL_SIZE);
    let (ph, aux) = r.split_at(PH_SIZE);
    Blocks { ctx, kernel, ph, aux }
}

fn u32_at(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(b[o..o + 4].try_into().unwrap())
}

/// VTM-pipe messages of one kind/length (0/21 = frame, 1/25 = speaker) from a DT_PIPEDUMP file.
fn oracle_msgs(path: &str, kind: u16, len: usize) -> Vec<Vec<u16>> {
    let d = fs::read(path).expect("read pipe dump");
    let mut pipes = BTreeMap::new();
    let mut writes = Vec::new();
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
                    let words = (0..cnt).map(|k| u16::from_le_bytes([d[i + 13 + k * 2], d[i + 14 + k * 2]])).collect();
                    writes.push((ptr, words));
                }
                i += 13 + size * cnt;
            }
            t => panic!("bad pipe tag {t}"),
        }
    }
    writes
        .into_iter()
        .filter(|(p, w): &(u32, Vec<u16>)| pipes.get(p) == Some(&0xdc) && w.len() == len && w[0] == kind)
        .map(|(_, w)| w)
        .collect()
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    let ph_path = args.get(1).cloned().unwrap_or_else(|| concat!(env!("CARGO_MANIFEST_DIR"), "/../oracle/test1.ph").into());
    let pipe_path = args.get(2).cloned().unwrap_or_else(|| concat!(env!("CARGO_MANIFEST_DIR"), "/../oracle/test1.pipe").into());
    let data = fs::read(&ph_path).expect("read ph dump");

    let mut m = Mem::new();
    let mut rust_msg: Option<u32> = None;
    let mut stats: BTreeMap<char, (usize, usize, usize)> = BTreeMap::new();
    let mut shown = 0;

    for (idx, rec) in data.chunks_exact(REC).enumerate() {
        let tag = rec[0] as char;
        let is_ph = rec[1] != 0;
        let ret = i32::from_le_bytes(rec[2..6].try_into().unwrap());
        let (ctx, kb, pb, ab) = (u32_at(rec, 6), u32_at(rec, 10), u32_at(rec, 14), u32_at(rec, 18));
        let pre = split(&rec[22..22 + BLK]);
        let post = split(&rec[22 + BLK..]);

        m.map("ctx", ctx, pre.ctx);
        m.map("kernel", kb, pre.kernel);
        m.map("ph", pb, pre.ph);
        m.map("aux", ab, pre.aux);
        if let Some(p) = rust_msg {
            m.w32(pb + 0x282c, p);
        }

        let arg = if is_ph { pb } else { ctx };
        let got_ret = match tag {
            'i' => { ph::clause_init(&mut m, arg); None }
            'f' => { ph::frames_init(&mut m, arg); None }
            'n' => Some(ph::phone_advance(&mut m, arg)),
            'e' => { ph::frame_emit(&mut m, arg); None }
            'g' => { ph::frame_tracks_interp(&mut m, arg); None }
            't' => { ph::f0::f0_frame(&mut m, arg); None }
            'r' => Some(ph::rules::clause_rules(&mut m, arg)),
            'a' => { ph::allophone::allophones(&mut m, arg); None }
            'b' => { ph::duration::durations(&mut m, arg); None }
            'c' => { ph::intonation::intonation(&mut m, arg); None }
            'p' => { ph::targets::phone_targets(&mut m, arg); None }
            'k' => { ph::speaker::apply_voice(&mut m, arg); None }
            _ => {
                stats.entry(tag).or_default().2 += 1;
                continue;
            }
        };
        rust_msg = Some(m.r32(pb + 0x282c));

        let mut diffs = Vec::new();
        if let Some(r) = got_ret {
            if r != ret {
                diffs.push(format!("ret got {r} want {ret}"));
            }
        }
        for (name, want) in [("ctx", post.ctx), ("kernel", post.kernel), ("ph", post.ph), ("aux", post.aux)] {
            let got = &m.region(name).bytes;
            let mut o = 0;
            while o < want.len() {
                let masked = (name == "ph" && MASK_PH.iter().any(|&(a, b)| o >= a && o < b))
                    || (name == "ctx" && MASK_CTX.iter().any(|&(a, b)| o >= a && o < b))
                    || (name == "kernel" && MASK_KERNEL.iter().any(|&(a, b)| o >= a && o < b));
                if !masked && got[o] != want[o] {
                    diffs.push(format!("{name}+0x{o:04x}: got {:02x} want {:02x}", got[o], want[o]));
                }
                o += 1;
            }
        }
        let e = stats.entry(tag).or_default();
        e.0 += 1;
        if !diffs.is_empty() {
            e.1 += 1;
            if shown < 6 {
                shown += 1;
                println!("record {idx} ({tag}) {} diffs:", diffs.len());
                for d in diffs.iter().take(16) {
                    println!("  {d}");
                }
            }
        }
    }

    for (tag, (run, bad, skipped)) in &stats {
        if *skipped > 0 {
            println!("stage {tag}: {skipped} skipped (not ported)");
        } else {
            println!("stage {tag}: {run} run, {bad} mismatched");
        }
    }

    // Words the original never writes (uninitialized malloc) and the VTM ignores.
    let check = |label: &str, kind: u16, len: usize, holes: &[usize]| -> bool {
        let strip = |mut w: Vec<u16>| {
            for &h in holes {
                w[h] = 0;
            }
            w
        };
        let want: Vec<Vec<u16>> = oracle_msgs(&pipe_path, kind, len).into_iter().map(strip).collect();
        let got: Vec<Vec<u16>> = m
            .pipe_log
            .iter()
            .filter(|w| w.words.len() == len && w.words[0] == kind)
            .map(|w| strip(w.words.clone()))
            .collect();
        let n = got.len().min(want.len());
        let first_bad = (0..n).find(|&k| got[k] != want[k]);
        println!("{label} messages: rust {} oracle {}, first mismatch {:?}", got.len(), want.len(), first_bad);
        if let Some(k) = first_bad {
            println!("  got  {:?}\n  want {:?}", got[k], want[k]);
            let per_word: Vec<usize> = (0..len).map(|w| (0..n).filter(|&k| got[k][w] != want[k][w]).count()).collect();
            println!("  mismatches per word index: {per_word:?}");
        }
        first_bad.is_none() && got.len() == want.len()
    };

    // Each check only applies when the stage producing those messages was dumped.
    let frames_ok = !stats.contains_key(&'e') || check("frame", 0, 21, &[20]);
    let speaker_ok = !stats.contains_key(&'k') || check("speaker", 1, 25, &[0x15, 0x18]);
    let ok = stats.values().all(|s| s.1 == 0) && frames_ok && speaker_ok;
    if ok { ExitCode::SUCCESS } else { ExitCode::FAILURE }
}
