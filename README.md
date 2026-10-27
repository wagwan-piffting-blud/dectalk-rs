# dectalk-rs

DECtalk 4.60 (the NWS_US build used by NOAA Weather Radio) as native Rust. **Sample-for-sample identical to the original Windows `dectalk.dll`**, with no DLL, no Wine and no emulator.

```sh
dectalk -o warning.wav "The National Weather Service has issued a severe thunderstorm warning."
```

## Status

The whole pipeline is ported: command parser (CMD, every `[:command]`), letter-to-sound and dictionary lookup (LTS), phonetics and prosody (PH), and the vocal tract model (VTM) with the tone generator. Raw text in, PCM out, sample-exact against the DLL on every corpus it has been checked on, including `[:tone]`, `[:rate]`, `[:dv]`, `[:mode]`, voice changes and inline phonemes. 8 kHz mu-law output is byte-exact as well.

There is **no tolerance anywhere**. A render either matches the DLL to the sample, or it is a bug.

Not yet ported:

- PH clause mode 8
- index marks and callbacks (there is no API host to deliver them to)
- `[:play]`, `[:volume]`, `[:pause]`, `[:resume]` drive an audio device in the original and do nothing here

## `dectalk` - the deliverable

One static executable with both dictionaries compiled in. Nothing beside it, no install step.

```sh
dectalk -o out.wav "Hello there."
dectalk -o out.wav -f bulletin.txt
echo "[:np] Hello." | dectalk -o out.wav -
dectalk --8k -o phone.wav "Telephone quality, 8 kHz mu-law."
```

| Option | Meaning |
|---|---|
| `-o FILE` | Output WAV (default `dectalk.wav`) |
| (default) | 11025 Hz, 16-bit PCM mono |
| `--8k` | 8000 Hz, 8-bit mu-law (the original's telephone format) |
| `--8k-pcm` | 8000 Hz, 16-bit PCM mono |
| `-f FILE` / `-` | Read text from a file / stdin (Latin-1 bytes, as the original) |
| `-d FILE` | Main dictionary, overriding the built-in one |
| `-u FILE` / `--no-user` | User dictionary, overriding the built-in one / none |

A dictionary on disk takes precedence over the built-in copy: `-d`, then `$DECTALK_DIC`, then `dtalk_nws_us.dic` next to the executable, in `<exe>/../share/dectalk`, in the current directory, or (on Unix) in `/usr/local/share/dectalk` and `/usr/share/dectalk`. `USER_NWS_US.DIC` is searched beside the main dictionary first, then in the same places. The original always loads the user dictionary, so output only matches it when one is loaded; `--no-user` is there for experiments.

## Platforms

```text
dectalk-windows-amd64.exe    x86_64-pc-windows-msvc          Windows 10/11 64-bit
dectalk-windows-x86.exe      i686-pc-windows-msvc            32-bit Windows
dectalk-linux-amd64          x86_64-unknown-linux-musl       x86_64 Linux (most desktop/server distros, Docker, WSL)
dectalk-linux-x86            i686-unknown-linux-musl         32-bit x86 Linux, Pentium 4 or newer
dectalk-linux-arm64          aarch64-unknown-linux-musl      Pi 3/4/5, Zero 2 W on 64-bit Raspberry Pi OS
dectalk-linux-armv7          armv7-unknown-linux-musleabihf  Pi 2/3/4 on 32-bit Raspberry Pi OS
dectalk-linux-armv6          arm-unknown-linux-musleabihf    Pi 1, Zero / Zero W
dectalk-macos-universal      aarch64 + x86_64-apple-darwin   Apple Silicon and Intel Macs, one file
```

Every Linux binary is fully static (musl), so it runs on any distro with no libraries. The Windows binaries link the C runtime statically and need no VC++ redistributable.

## Building

Needs a Rust toolchain with edition 2024 support (1.85 or newer). The crate has **no external dependencies**.

```sh
cd dectalk-rs
cargo build --release --bin dectalk
```

### All targets

`build_targets.ps1` builds all eight targets from a Windows machine with **no cross toolchain**: the Linux targets are linked by Rust's own bundled `rust-lld`, and macOS is handed to a Mac over ssh (below). Missing `rustup` targets are added automatically. Results land in `dist/`.

```powershell
powershell -ExecutionPolicy Bypass -File build_targets.ps1                    # all eight
powershell -ExecutionPolicy Bypass -File build_targets.ps1 -NoMac             # everything but macOS
powershell -ExecutionPolicy Bypass -File build_targets.ps1 -Only linux-arm64  # one target
powershell -ExecutionPolicy Bypass -File build_targets.ps1 -Check             # build, then verify
```

The script checks each binary's header against the architecture it was meant for, so a mis-targeted build cannot ship. `-Check` renders a fixed set of phrases (both 11 kHz and 8 kHz) through every built binary and demands that each WAV's SHA256 equal the one from `windows-amd64`. The Linux binaries run under WSL, the ARM ones through `qemu-user` on the CPU they target (Cortex-A7 for armv7, ARM1176 for armv6), which needs `apt install qemu-user-static` in the default WSL distro. Both slices of the macOS binary run on the Mac over ssh, the Intel one under Rosetta. `-NoBuild -Check` checks what is already in `dist/`.

### macOS

macOS has to be linked on a Mac. `build_macos.sh` builds both slices, `lipo`s them into one universal binary, checks that the Apple Silicon slice and the Intel slice (under Rosetta) render the same phrases byte-identically, then signs it with a Developer ID (hardened runtime) and notarizes it. A bare Mach-O cannot be stapled, so Gatekeeper checks the ticket online on first run.

```sh
sh build_macos.sh                # build, sign, check, notarize
sh build_macos.sh --no-notarize  # sign only
sh build_macos.sh --no-sign      # unsigned local build
```

The signing identity and the App Store Connect API key ID, issuer and `.p8` path are stored at the top of `build_macos.sh`. If the `.p8` is not at the stored path, the script looks for `AuthKey_<key id>.p8` in `~/Desktop`, `~/Downloads`, `~/Documents`, `~/.appstoreconnect/private_keys`, `~/.private_keys` and `~/private_keys`. The login keychain is unlocked before the build when it is locked (over ssh it always is), which asks for the Mac password.

From Windows, `build_targets.ps1` runs it over ssh on `-MacHost` (default `wags@192.168.1.86`) in `-MacPath` (default `/Volumes/LocalUser/Documents/dect`) and picks the result up from `dist/`; the path must be this same checkout as the Mac sees it (an SMB share, for example).

## Using it as a library

```rust
use dectalk_rs::tts::Tts;

let main = std::fs::read("dict/dtalk_nws_us.dic")?;
let user = std::fs::read("dict/USER_NWS_US.DIC")?;
let mut tts = Tts::new(&main, Some(&user), 11025); // or 8000
tts.speak(b"[:np] Hello from Rust.");
let pcm: Vec<i16> = tts.take_pcm();
```

Text is Latin-1 bytes, exactly as the original took it.

## Layout

```text
dectalk-rs/src/bin/dectalk.rs   THE BINARY: text in, WAV out
dectalk-rs/src/tts.rs           the pipeline: CMD -> LTS (own thread) -> PH -> VTM
dectalk-rs/src/cmd/             command parser, text normalization, all [:command] handlers
dectalk-rs/src/lts/             letter-to-sound, dictionary lookup, spelling
dectalk-rs/src/ph/              allophones, duration, intonation, F0, speaker definitions
dectalk-rs/src/vtm/             vocal tract model and tone generator
dectalk-rs/src/tables/          the DLL's data tables as named Rust statics
dectalk-rs/src/ulaw.rs          8 kHz mu-law encoder
dectalk-rs/dict/                the NWS_US main and user dictionaries
```

The other binaries in `src/bin/` (`tts_chain`, `cmd_replay`, `lts_replay`, `ph_replay`, `vtm_replay`, ...) are the verification tools. They are built only with `--features oracle` and diff each stage against captures taken from the original DLL; those captures, the capture harness and the DLL itself are not part of this repository.

## How it was done

Clean-room reverse engineering of the GW Micro DECtalk-OEM 4.60 NWS_US `dectalk.dll` in Ghidra, and nothing else: the publicly circulating DECtalk source was never consulted. Each stage was verified against an instrumented harness around the original DLL that dumps the data flowing between the stages, so a port could be diffed stage by stage before the next one existed, and then end to end.

## GenAI Disclosure Notice: Portions of this codebase have been generated using Generative AI tools (Claude Code).
