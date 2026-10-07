//! The complete standalone pipeline: text -> CMD -> LTS -> PH -> VTM -> 16-bit PCM.
//!
//! CMD and PH are resumable state machines driven on the caller's thread. LTS blocks deep inside
//! its word handlers when its input runs dry, so it runs on its own thread; its input source hands
//! back everything it wrote whenever it goes idle. A CMD drain wait (FUN_1c0282a0, used before
//! tones) flushes LTS and PH at that point, so tones land after the speech that precedes them.

use std::sync::mpsc::{channel, Receiver, Sender};
use std::thread::{self, JoinHandle};

use crate::cmd::engine::{self as cmd_engine, Cmd};
use crate::lts::engine::{self as lts_engine, Lts};
use crate::ph::engine::Engine;
use crate::ph::mem::DRAIN_PIPE;

pub struct Tts {
    cmd: Cmd,
    ph: Engine,
    to_lts: Option<Sender<Vec<u16>>>,
    from_lts: Receiver<Vec<u16>>,
    lts_thread: Option<JoinHandle<()>>,
    pending: Vec<u16>,
    pub rate: u32,
}

impl Tts {
    pub fn new(main_dict: &[u8], user_dict: Option<&[u8]>, rate: u32) -> Self {
        let (to_lts, lts_in) = channel::<Vec<u16>>();
        let (lts_out, from_lts) = channel::<Vec<u16>>();
        let mut lts = Lts::new(main_dict, user_dict);
        lts.m.source = Some(Box::new(move |m| {
            let out = m
                .pipe_log
                .drain(..)
                .filter(|w| w.pipe == lts_engine::OUT_PIPE)
                .flat_map(|w| w.words)
                .collect();
            lts_out.send(out).ok()?;
            lts_in.recv().ok()
        }));
        let lts_thread = thread::Builder::new()
            .name("dectalk-lts".into())
            .spawn(move || {
                lts.run();
            })
            .expect("spawn LTS thread");
        let tts = Tts {
            cmd: Cmd::new(main_dict, user_dict, rate),
            ph: Engine::new(rate),
            to_lts: Some(to_lts),
            from_lts,
            lts_thread: Some(lts_thread),
            pending: Vec::new(),
            rate,
        };
        // The LTS thread goes idle immediately, before any input.
        let first = tts.from_lts.recv().expect("LTS thread start");
        assert!(first.is_empty());
        let mut tts = tts;
        // The API's pipeline reset (run when output is opened) sends "last voice" + sync to LTS.
        tts.pending.extend_from_slice(&[0x1f03, 0x1f0a, 0x20]);
        tts.flush();
        tts
    }

    /// TextToSpeechSpeak with TTS_FORCE: the text is followed by a forced clause flush.
    pub fn speak(&mut self, text: &[u8]) {
        let mut bytes = text.to_vec();
        bytes.extend_from_slice(b"\x0b\x0a");
        self.feed(&bytes);
    }

    /// Queue raw API-pipe bytes and run the whole pipeline over them.
    pub fn feed(&mut self, bytes: &[u8]) {
        self.cmd.push_text(bytes);
        for w in self.cmd.run() {
            match w.pipe {
                cmd_engine::LTS_PIPE => self.pending.extend(w.words),
                DRAIN_PIPE => self.flush(),
                cmd_engine::VTM_PIPE => {
                    self.flush();
                    self.ph.m.pipe_write_words(crate::ph::engine::VTM_PIPE, &w.words);
                    self.ph.synthesize();
                }
                cmd_engine::PH_PIPE => {
                    self.flush();
                    self.ph.push_symbols(&w.words);
                    self.ph.run();
                    self.ph.synthesize();
                }
                _ => {}
            }
        }
        self.flush();
    }

    /// Run LTS over everything CMD has queued for it, then PH and the synthesizer.
    fn flush(&mut self) {
        if !self.pending.is_empty() {
            let words = std::mem::take(&mut self.pending);
            self.to_lts.as_ref().unwrap().send(words).expect("LTS thread alive");
            let out = self.from_lts.recv().expect("LTS thread alive");
            self.ph.push_symbols(&out);
        }
        self.ph.run();
        self.ph.synthesize();
    }

    /// Samples produced so far (and clears them).
    pub fn take_pcm(&mut self) -> Vec<i16> {
        std::mem::take(&mut self.ph.pcm)
    }
}

impl Drop for Tts {
    fn drop(&mut self) {
        self.to_lts.take();
        if let Some(h) = self.lts_thread.take() {
            let _ = h.join();
        }
    }
}
