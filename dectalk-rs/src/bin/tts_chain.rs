//! Text -> full standalone Rust pipeline -> audio, diffed against the DLL's WAV for the same text.
//!
//! usage: tts_chain <base> ["literal text"] [--8k]
//!   default: 11025 Hz 16-bit, compares <base>.wav
//!   --8k:    8000 Hz mu-law, compares <base>_8k.wav
//! Text comes from <base>.txt unless given.

use std::env;
use std::fs;
use std::process::ExitCode;

use dectalk_rs::tts::Tts;
use dectalk_rs::ulaw;

const MAIN_DICT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/dict/dtalk_nws_us.dic");
const USER_DICT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/dict/USER_NWS_US.DIC");

fn main() -> ExitCode {
    let a: Vec<String> = env::args().filter(|s| !s.starts_with("--")).collect();
    let k8 = env::args().any(|s| s == "--8k");
    let base = a.get(1).cloned().unwrap_or_else(|| concat!(env!("CARGO_MANIFEST_DIR"), "/../oracle/test1").into());
    let text = match a.get(2) {
        Some(t) => t.as_bytes().to_vec(),
        None => fs::read(format!("{base}.txt")).expect("read text"),
    };
    let main = fs::read(MAIN_DICT).expect("main dict");
    let user = fs::read(USER_DICT).expect("user dict");
    let mut tts = Tts::new(&main, Some(&user), if k8 { 8000 } else { 11025 });
    tts.speak(&text);
    let got = tts.take_pcm();
    let (got, want): (Vec<i32>, Vec<i32>) = if k8 {
        let wav = fs::read(format!("{base}_8k.wav")).expect("read wav");
        let enc = ulaw::encode_all(&got);
        (enc.iter().map(|&b| b as i32).collect(), wav[44..].iter().map(|&b| b as i32).collect())
    } else {
        let wav = fs::read(format!("{base}.wav")).expect("read wav");
        let want = wav[44..].chunks_exact(2).map(|c| i16::from_le_bytes([c[0], c[1]]) as i32).collect();
        (got.iter().map(|&s| s as i32).collect(), want)
    };
    let n = got.len().min(want.len());
    let bad = (0..n).find(|&k| got[k] != want[k]);
    println!("samples: rust {} oracle {} first mismatch {:?}", got.len(), want.len(), bad);
    if let Some(k) = bad {
        let lo = k.saturating_sub(4);
        println!("  got  {:x?}", &got[lo..(k + 8).min(got.len())]);
        println!("  want {:x?}", &want[lo..(k + 8).min(want.len())]);
    }
    if bad.is_none() && got.len() == want.len() { ExitCode::SUCCESS } else { ExitCode::FAILURE }
}
