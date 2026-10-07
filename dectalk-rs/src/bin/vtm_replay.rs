//! Replays a dtoracle VTM dump (event byte, u32 rate, pre-state, post-state) through the
//! Rust VTM and reports byte-level divergence from the original DLL.

use std::env;
use std::fs;
use std::process::ExitCode;

use dectalk_rs::vtm::{Vtm, PARAM_BASE, STATE_SIZE};

const REC_SIZE: usize = 1 + 4 + STATE_SIZE * 2;
const PARAM_END: usize = PARAM_BASE + 24 * 2;
const OUT_BASE: usize = 0x08;
const OUT_END: usize = OUT_BASE + 71 * 2;

fn main() -> ExitCode {
    let path = env::args()
        .nth(1)
        .unwrap_or_else(|| concat!(env!("CARGO_MANIFEST_DIR"), "/../oracle/test1.vtm").to_string());
    // --chain: carry Rust state across events, taking only the parameter block from the dump.
    let chain = env::args().any(|a| a == "--chain");
    let data = fs::read(&path).expect("read dump");
    let mut frames = 0usize;
    let mut bad = 0usize;
    let mut shown = 0usize;
    let mut carried: Option<Vtm> = None;

    for (i, rec) in data.chunks_exact(REC_SIZE).enumerate() {
        let kind = rec[0] as char;
        let rate = u32::from_le_bytes(rec[1..5].try_into().unwrap());
        let pre = &rec[5..5 + STATE_SIZE];
        let post = &rec[5 + STATE_SIZE..];
        let mut v = match (chain, carried.take()) {
            (true, Some(mut c)) => {
                c.s[PARAM_BASE..PARAM_END].copy_from_slice(&pre[PARAM_BASE..PARAM_END]);
                // The audio sink rewrites the sample buffer in place (mu-law at 8 kHz) between frames.
                c.s[OUT_BASE..OUT_END].copy_from_slice(&pre[OUT_BASE..OUT_END]);
                c
            }
            _ => Vtm::from_bytes(pre),
        };
        match kind {
            'F' => {
                v.synth_frame(rate);
                frames += 1;
            }
            'S' => v.apply_speaker(),
            'R' => v.reset(),
            _ => panic!("bad event {kind:?} at record {i}"),
        }
        let diffs: Vec<usize> = (0..STATE_SIZE).step_by(2).filter(|&o| v.s[o..o + 2] != post[o..o + 2]).collect();
        if !diffs.is_empty() {
            bad += 1;
            if shown < 5 {
                shown += 1;
                println!("record {i} ({kind}) differs at {} words:", diffs.len());
                for &o in diffs.iter().take(12) {
                    let got = i16::from_le_bytes([v.s[o], v.s[o + 1]]);
                    let want = i16::from_le_bytes([post[o], post[o + 1]]);
                    println!("  +0x{o:03x}: got {got:6} want {want:6}");
                }
            }
        }
        carried = Some(v);
    }
    let total = data.len() / REC_SIZE;
    println!("{total} events ({frames} frames), {bad} mismatched");
    if bad == 0 { ExitCode::SUCCESS } else { ExitCode::FAILURE }
}
