//! End-to-end check from the CMD->LTS stream: standalone Rust LTS (own init, dictionary file)
//! -> Rust PH -> VTM, diffing the LTS->PH symbols against DT_PIPEDUMP and the PCM against WAV.
//!
//! usage: lts_chain <base>   (reads <base>.pipe and <base>.wav)

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::process::ExitCode;

use dectalk_rs::lts::engine::Lts;
use dectalk_rs::ph::engine::Engine;

const MAIN_DICT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/dict/dtalk_nws_us.dic");
const USER_DICT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/dict/USER_NWS_US.DIC");

fn u32_at(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(b[o..o + 4].try_into().unwrap())
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

/// Split off the 0x000b sync words the API thread appends to a pipe at shutdown.
fn split_tail(mut v: Vec<u16>) -> (Vec<u16>, Vec<u16>) {
    let mut tail = Vec::new();
    while v.last() == Some(&0xb) {
        tail.push(v.pop().unwrap());
    }
    (v, tail)
}

fn main() -> ExitCode {
    let a: Vec<String> = env::args().collect();
    let base = a.get(1).cloned().unwrap_or_else(|| concat!(env!("CARGO_MANIFEST_DIR"), "/../oracle/test1").into());
    let streams = pipe_streams(&format!("{base}.pipe"));
    let (input, _) = split_tail(streams.get(&0x118).cloned().unwrap_or_default());
    let (want, tail) = split_tail(streams.get(&0x134).cloned().unwrap_or_default());

    let main = fs::read(MAIN_DICT).expect("main dict");
    let user = fs::read(USER_DICT).expect("user dict");
    let mut lts = Lts::new(&main, Some(&user));
    lts.push_input(&input);
    let mut got = lts.run();
    let n = got.len().min(want.len());
    let bad = (0..n).find(|&k| got[k] != want[k]);
    println!("lts symbols: rust {} oracle {} first mismatch {:?}", got.len(), want.len(), bad);
    if let Some(k) = bad {
        let lo = k.saturating_sub(6);
        println!("  got  {:04x?}", &got[lo..(k + 10).min(got.len())]);
        println!("  want {:04x?}", &want[lo..(k + 10).min(want.len())]);
    }

    got.extend(tail);
    let mut ph = Engine::new(11025);
    ph.push_symbols(&got);
    ph.run();
    ph.synthesize();
    let wav = fs::read(format!("{base}.wav")).expect("read wav");
    let pcm: Vec<i16> = wav[44..].chunks_exact(2).map(|c| i16::from_le_bytes([c[0], c[1]])).collect();
    let m = ph.pcm.len().min(pcm.len());
    let pbad = (0..m).find(|&k| ph.pcm[k] != pcm[k]);
    println!("pcm samples: rust {} oracle {} first mismatch {:?}", ph.pcm.len(), pcm.len(), pbad);
    let ok = bad.is_none() && got.len() - 3.min(got.len()) <= want.len() && pbad.is_none() && ph.pcm.len() == pcm.len();
    if ok { ExitCode::SUCCESS } else { ExitCode::FAILURE }
}
