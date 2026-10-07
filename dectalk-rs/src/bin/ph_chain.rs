//! End-to-end check of the standalone PH engine + VTM: feed the LTS->PH symbol stream from a
//! DT_PIPEDUMP file, diff every VTM message against the DLL's, then diff PCM against its WAV.

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::process::ExitCode;

use dectalk_rs::ph::engine::Engine;

fn u32_at(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(b[o..o + 4].try_into().unwrap())
}

/// Word streams per kernel pipe offset, one Vec per write.
fn pipe_writes(path: &str) -> BTreeMap<u32, Vec<Vec<u16>>> {
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
                    let w = (0..cnt).map(|k| u16::from_le_bytes([d[i + 13 + k * 2], d[i + 14 + k * 2]])).collect();
                    raw.push((ptr, w));
                }
                i += 13 + size * cnt;
            }
            t => panic!("bad tag {t}"),
        }
    }
    let mut out: BTreeMap<u32, Vec<Vec<u16>>> = BTreeMap::new();
    for (p, w) in raw {
        if let Some(&off) = pipes.get(&p) {
            out.entry(off).or_default().push(w);
        }
    }
    out
}

/// Split a flat VTM word stream into messages (kind word + fixed payload).
fn split_msgs(words: &[u16]) -> Vec<Vec<u16>> {
    let mut v = Vec::new();
    let mut i = 0;
    while i < words.len() {
        let n = match words[i] & 0xff {
            0 => 21,
            1 => 25,
            2 => 6,
            7 => 3,
            _ => 1,
        };
        v.push(words[i..(i + n).min(words.len())].to_vec());
        i += n;
    }
    v
}

fn strip(mut w: Vec<u16>) -> Vec<u16> {
    // Words the original never initializes.
    match (w[0] & 0xff, w.len()) {
        (0, 21) => w[20] = 0,
        (1, 25) => {
            w[0x15] = 0;
            w[0x18] = 0;
        }
        _ => {}
    }
    w
}

fn main() -> ExitCode {
    let a: Vec<String> = env::args().collect();
    let base = a.get(1).cloned().unwrap_or_else(|| concat!(env!("CARGO_MANIFEST_DIR"), "/../oracle/test1").into());
    let streams = pipe_writes(&format!("{base}.pipe"));
    let symbols: Vec<u16> = streams.get(&0x134).into_iter().flatten().flatten().copied().collect();
    let oracle_words: Vec<u16> = streams.get(&0xdc).into_iter().flatten().flatten().copied().collect();
    // Sync/flush words (8, 0xb) also arrive from the API thread, and tones (2) come straight from
    // CMD; compare only what PH produces: frames (0) and speaker definitions (1).
    let speech = |w: &Vec<u16>| matches!(w[0] & 0xff, 0 | 1);
    let all = split_msgs(&oracle_words);
    let tones = all.iter().filter(|w| w[0] & 0xff == 2).count();
    let want: Vec<Vec<u16>> = all.into_iter().filter(speech).map(strip).collect();

    let rate = if a.iter().any(|s| s == "--8k") { 8000 } else { 11025 };
    let mut e = Engine::new(rate);
    if a.iter().any(|s| s == "--oracle-vtm") {
        // Synthesize the DLL's own VTM message stream (speech + CMD tones).
        for w in split_msgs(&oracle_words) {
            e.m.pipe_write_words(0, &w);
        }
        e.synthesize();
        let wav = fs::read(format!("{base}.wav")).expect("read wav");
        let pcm: Vec<i16> = wav[44..].chunks_exact(2).map(|c| i16::from_le_bytes([c[0], c[1]])).collect();
        let m = e.pcm.len().min(pcm.len());
        let pbad = (0..m).find(|&k| e.pcm[k] != pcm[k]);
        println!("oracle-vtm pcm: rust {} oracle {}, first mismatch {:?}", e.pcm.len(), pcm.len(), pbad);
        return if pbad.is_none() && e.pcm.len() == pcm.len() { ExitCode::SUCCESS } else { ExitCode::FAILURE };
    }
    e.push_symbols(&symbols);
    e.run();
    let got: Vec<Vec<u16>> = e.vtm_messages().map(|w| strip(w.to_vec())).filter(speech).collect();

    let n = got.len().min(want.len());
    let bad = (0..n).find(|&k| got[k] != want[k]);
    if env::var_os("SPEAKERS").is_some() {
        for k in (0..n).filter(|&k| want[k][0] & 0xff == 1) {
            println!("speaker msg {k}: {} want {:?}", if got[k] == want[k] { "ok " } else { "BAD" }, &want[k][1..8]);
        }
    }
    println!("speech messages: rust {} oracle {}, first mismatch {:?} ({tones} CMD tone messages not modeled)", got.len(), want.len(), bad);
    if let Some(k) = bad {
        for j in k.saturating_sub(2)..(k + 2).min(n) {
            println!("  [{j}] got  {:?}\n  [{j}] want {:?}", got[j], want[j]);
        }
    }

    if rate == 8000 {
        return if bad.is_none() && got.len() == want.len() { ExitCode::SUCCESS } else { ExitCode::FAILURE };
    }
    e.synthesize();
    let wav = fs::read(format!("{base}.wav")).expect("read wav");
    let pcm: Vec<i16> = wav[44..].chunks_exact(2).map(|c| i16::from_le_bytes([c[0], c[1]])).collect();
    let m = e.pcm.len().min(pcm.len());
    let pbad = (0..m).find(|&k| e.pcm[k] != pcm[k]);
    println!("pcm samples: rust {} oracle {}, first mismatch {:?}", e.pcm.len(), pcm.len(), pbad);
    let ok = bad.is_none() && pbad.is_none() && got.len() == want.len() && e.pcm.len() == pcm.len();
    if ok { ExitCode::SUCCESS } else { ExitCode::FAILURE }
}
