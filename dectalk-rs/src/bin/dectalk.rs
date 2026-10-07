//! Standalone DECtalk: text -> mono WAV.
//!
//! usage: dectalk [-o out.wav] [--8k | --8k-pcm] [-d main.dic] [-u user.dic | --no-user] [-f text.txt | -] [text ...]
//!
//! Output: 11025 Hz 16-bit PCM (default), 8000 Hz mu-law (--8k, the DLL's telephone format) or
//! 8000 Hz 16-bit PCM (--8k-pcm).
//!
//! Both dictionaries are compiled in from dict/. A file on disk overrides them: the main dictionary
//! is found via -d, then $DECTALK_DIC, then dtalk_nws_us.dic next to the executable, in
//! <exe>/../share/dectalk, in the current directory, or (on Unix) in /usr/local/share/dectalk or
//! /usr/share/dectalk. The user dictionary (USER_NWS_US.DIC) is searched first beside the main
//! dictionary and then in the same places.

use std::borrow::Cow;
use std::env;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use dectalk_rs::tts::Tts;

static BUILTIN_MAIN: &[u8] = include_bytes!("../../dict/dtalk_nws_us.dic");
static BUILTIN_USER: &[u8] = include_bytes!("../../dict/USER_NWS_US.DIC");

#[derive(Clone, Copy, PartialEq)]
enum Format {
    Pcm11k,
    Ulaw8k,
    Pcm8k,
}

fn find(name: &str, first: Option<&Path>) -> Option<PathBuf> {
    let mut dirs: Vec<PathBuf> = first.into_iter().map(Path::to_path_buf).collect();
    if let Ok(exe) = env::current_exe() {
        if let Some(d) = exe.parent() {
            dirs.push(d.to_path_buf());
            dirs.push(d.join("../share/dectalk"));
        }
    }
    dirs.push(PathBuf::from("."));
    if cfg!(unix) {
        dirs.push(PathBuf::from("/usr/local/share/dectalk"));
        dirs.push(PathBuf::from("/usr/share/dectalk"));
    }
    dirs.into_iter().map(|d| d.join(name)).find(|p| p.is_file())
}

fn write_wav(path: &Path, pcm: &[i16], fmt: Format) -> io::Result<()> {
    let (rate, tag, bytes) = match fmt {
        Format::Pcm11k => (11025u32, 1u16, 2u16),
        Format::Pcm8k => (8000, 1, 2),
        Format::Ulaw8k => (8000, 7, 1),
    };
    let data_len = pcm.len() as u32 * bytes as u32;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&tag.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * bytes as u32).to_le_bytes());
    out.extend_from_slice(&bytes.to_le_bytes());
    out.extend_from_slice(&(bytes * 8).to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    if fmt == Format::Ulaw8k {
        out.extend(dectalk_rs::ulaw::encode_all(pcm));
    } else {
        for s in pcm {
            out.extend_from_slice(&s.to_le_bytes());
        }
    }
    fs::File::create(path)?.write_all(&out)
}

fn usage() -> ExitCode {
    eprintln!("usage: dectalk [-o out.wav] [--8k | --8k-pcm] [-d main.dic] [-u user.dic | --no-user] [-f text.txt | -] [text ...]");
    ExitCode::from(2)
}

fn main() -> ExitCode {
    let mut out = PathBuf::from("dectalk.wav");
    let mut main_dic: Option<PathBuf> = env::var_os("DECTALK_DIC").map(PathBuf::from);
    let mut user_dic: Option<PathBuf> = None;
    let mut no_user = false;
    let mut fmt = Format::Pcm11k;
    let mut text: Vec<u8> = Vec::new();
    let mut words: Vec<String> = Vec::new();
    let mut args = env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "-o" => out = args.next().map(PathBuf::from).unwrap_or(out),
            "-d" => main_dic = args.next().map(PathBuf::from),
            "-u" => user_dic = args.next().map(PathBuf::from),
            "--no-user" => no_user = true,
            "--8k" => fmt = Format::Ulaw8k,
            "--8k-pcm" => fmt = Format::Pcm8k,
            "-f" => {
                let Some(p) = args.next() else {
                    return usage();
                };
                match fs::read(&p) {
                    Ok(b) => text.extend(b),
                    Err(e) => {
                        eprintln!("cannot read {p}: {e}");
                        return ExitCode::FAILURE;
                    }
                }
            }
            "-" => {
                if io::stdin().read_to_end(&mut text).is_err() {
                    return usage();
                }
            }
            "-h" | "--help" => return usage(),
            _ => words.push(a),
        }
    }
    if !words.is_empty() {
        if !text.is_empty() {
            text.push(b' ');
        }
        // Command-line text arrives as UTF-8; DECtalk reads Latin-1 bytes.
        let joined = words.join(" ");
        text.extend(joined.chars().map(|c| if (c as u32) < 0x100 { c as u8 } else { b'?' }));
    }
    if text.is_empty() {
        return usage();
    }
    let main_path = main_dic.or_else(|| find("dtalk_nws_us.dic", None));
    let main: Cow<[u8]> = match &main_path {
        Some(p) => match fs::read(p) {
            Ok(b) => Cow::Owned(b),
            Err(e) => {
                eprintln!("cannot read {}: {e}", p.display());
                return ExitCode::FAILURE;
            }
        },
        None => Cow::Borrowed(BUILTIN_MAIN),
    };
    let user: Option<Cow<[u8]>> = if no_user {
        None
    } else {
        match user_dic.or_else(|| find("USER_NWS_US.DIC", main_path.as_deref().and_then(Path::parent))) {
            Some(p) => fs::read(p).ok().map(Cow::Owned),
            None => Some(Cow::Borrowed(BUILTIN_USER)),
        }
    };

    let rate = if fmt == Format::Pcm11k { 11025 } else { 8000 };
    let mut tts = Tts::new(&main, user.as_deref(), rate);
    tts.speak(&text);
    let pcm = tts.take_pcm();
    if let Err(e) = write_wav(&out, &pcm, fmt) {
        eprintln!("cannot write {}: {e}", out.display());
        return ExitCode::FAILURE;
    }
    eprintln!("{} samples ({:.2} s) -> {}", pcm.len(), pcm.len() as f64 / rate as f64, out.display());
    ExitCode::SUCCESS
}
