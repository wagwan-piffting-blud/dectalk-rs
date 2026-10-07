//! Vocal tract model: fixed-point formant synthesizer.
//!
//! Transliterated from dectalk.dll 4.60 (FUN_1c0294e0 and helpers). State is kept as the
//! original 0x230-byte struct so it can be diffed against oracle dumps byte for byte;
//! byte offsets below are offsets into that struct.

mod rom;
pub mod tone;

use rom::*;

pub const STATE_SIZE: usize = 0x230;

const NOISE_MUL: i16 = 20077;
const NOISE_ADD: i16 = 12345;
const NOISE_LPF: i32 = 1499;

// Frame / speaker parameter block lives at 0xd2 (20 words per frame, 24 per speaker).
pub const PARAM_BASE: usize = 0xd2;
const OUT_BASE: usize = 0x08;

#[inline]
fn rom(base: usize, idx: i32) -> i32 {
    ROM[(base as i32 + idx) as usize] as i32
}

#[derive(Clone)]
pub struct Vtm {
    pub s: [u8; STATE_SIZE],
}

impl Vtm {
    pub fn new(rate: u32) -> Self {
        let mut v = Vtm { s: [0; STATE_SIZE] };
        v.s[0x118..0x120].copy_from_slice(&(rate as f64).to_le_bytes());
        match rate {
            11025 => {
                v.sd(0x120, 0);
                v.sd(0x00, 0);
                v.sw(0x04, 0x468f);
                v.sw(0x06, 0x741a);
                v.sd(0x114, 71);
            }
            8000 => {
                v.sd(0x120, 1);
                v.sd(0x00, 1);
                v.sw(0x04, 0x6666);
                v.sw(0x06, 0x5000);
                v.sd(0x114, 51);
            }
            _ => v.sd(0x00, 2),
        }
        v
    }

    pub fn from_bytes(b: &[u8]) -> Self {
        let mut v = Vtm { s: [0; STATE_SIZE] };
        v.s.copy_from_slice(&b[..STATE_SIZE]);
        v
    }

    #[inline]
    fn w(&self, off: usize) -> i32 {
        i16::from_le_bytes([self.s[off], self.s[off + 1]]) as i32
    }

    #[inline]
    fn sw(&mut self, off: usize, v: i32) {
        self.s[off..off + 2].copy_from_slice(&(v as i16).to_le_bytes());
    }

    #[inline]
    fn d(&self, off: usize) -> i32 {
        i32::from_le_bytes(self.s[off..off + 4].try_into().unwrap())
    }

    #[inline]
    fn sd(&mut self, off: usize, v: i32) {
        self.s[off..off + 4].copy_from_slice(&v.to_le_bytes());
    }

    pub fn samples_per_frame(&self) -> usize {
        self.d(0x114) as usize
    }

    pub fn set_params(&mut self, words: &[u16]) {
        for (i, &p) in words.iter().enumerate() {
            self.sw(PARAM_BASE + i * 2, p as i16 as i32);
        }
    }

    pub fn output(&self) -> Vec<i16> {
        (0..self.samples_per_frame())
            .map(|i| self.w(OUT_BASE + i * 2) as i16)
            .collect()
    }

    fn scale_fb(&self, f: i32, bw: i32) -> (i32, i32) {
        let k = self.w(0x06);
        match self.d(0x00) {
            0 => ((f as i16 as i32 * k) >> 15, (bw as i16 as i32 * k) >> 15),
            1 => (
                ((f as i16 as i32 * k) >> 14) & 0xfffe,
                ((bw as i16 as i32 * k) >> 14) & 0xfffe,
            ),
            _ => (f, bw),
        }
    }

    fn pole(&self, f: i32, bw: i32) -> (i32, i32) {
        let r = rom(BW_DECAY, (bw as i16 as i32) >> 3);
        let b = ((rom(COS2, (f as i16 as i32) >> 3) * r) >> 12) as i16 as i32;
        let r2 = ((r * r) >> 12) as i16 as i32;
        (b, r2)
    }

    /// FUN_1c029400: resonator coefficients, silenced at or above 4500 Hz.
    fn res_cut(&mut self, b_off: Option<usize>, c_off: Option<usize>, f: i32, bw: i32, gain: i32) -> (i32, i32, i32) {
        let (f, bw) = self.scale_fb(f, bw);
        if (f as i16) < 0x1194 {
            let (b, r2) = self.pole(f, bw);
            let c = (-r2) as i16 as i32;
            let sum = ((0x1000 - b) + r2) as i16 as i32;
            self.sw(0x20e, sum);
            if let Some(o) = b_off {
                self.sw(o, b);
            }
            if let Some(o) = c_off {
                self.sw(o, c);
            }
            let a = ((sum * (gain as i16 as i32)) >> 11) & !1;
            (a, b, c)
        } else {
            if let Some(o) = b_off {
                self.sw(o, 0);
            }
            if let Some(o) = c_off {
                self.sw(o, 0);
            }
            (0, 0, 0)
        }
    }

    /// FUN_1c029240: resonator coefficients, zero poles at or above 4500 Hz but gain still computed.
    fn res_zero(&mut self, b_off: usize, c_off: usize, f: i32, bw: i32, gain: i32) -> i32 {
        let (f, bw) = self.scale_fb(f, bw);
        let (b, c) = if (f as i16) < 0x1194 {
            let (b, r2) = self.pole(f, bw);
            (b, (-r2) as i16 as i32)
        } else {
            (0, 0)
        };
        self.sw(b_off, b);
        self.sw(c_off, c);
        let sum = ((0x1000 - b) - c) as i16 as i32;
        self.sw(0x20e, sum);
        ((sum * (gain as i16 as i32)) >> 11) & !1
    }

    /// FUN_1c029320: resonator coefficients, clamped to Nyquist at or above 4500 Hz.
    fn res_clamp(&mut self, rate: u32, b_off: usize, c_off: usize, f: i32, bw: i32, gain: i32) -> i32 {
        let (mut f, mut bw) = self.scale_fb(f, bw);
        if (f as i16) > 0x1193 {
            f = (rate >> 1) as u16 as i16 as i32;
            bw = (rate >> 2) as u16 as i16 as i32;
        }
        let (b, r2) = self.pole(f, bw);
        self.sw(b_off, b);
        self.sw(c_off, -r2);
        let sum = ((0x1000 - b) + r2) as i16 as i32;
        self.sw(0x20e, sum);
        ((sum * (gain as i16 as i32)) >> 11) & !1
    }

    /// FUN_1c02a670: clear filter delay lines.
    pub fn reset(&mut self) {
        for off in (0x12a..=0x160).step_by(2) {
            self.sw(off, 0);
        }
        self.sw(0x16c, 0);
        self.sw(0x1e6, 0);
    }

    /// FUN_1c02a270: derive speaker constants from the 24-word block at PARAM_BASE.
    pub fn apply_speaker(&mut self) {
        self.sd(0x214, 1);
        for off in (0x12a..=0x160).step_by(2) {
            self.sw(off, 0);
        }
        for off in [0x16c, 0x1e6, 0x1fa, 0x1e8, 0x17e, 0x172, 0x170, 0x17a, 0x178, 0x176, 0x174] {
            self.sw(off, 0);
        }
        let mode = self.d(0x00);
        self.sw(0x1e2, if mode == 1 { 0xf8afu16 as i16 as i32 } else if mode == 0 || mode == 2 { 0xf49fu16 as i16 as i32 } else { self.w(0x1e2) });
        self.res_cut(Some(0x1d2), Some(0x1d4), 0x122, 0x46, 0);
        let (g, bw, f) = match mode {
            0 => (0x960, 0x267, 0x3b4),
            1 => (0x960, 0x1c5, 0x2ba),
            _ => (0x960, 0x22e, 0x35c),
        };
        let (a, _, _) = self.res_cut(Some(0x1de), Some(0x1e0), f, bw, g);
        self.sw(0x1dc, a);
        let (f, bw) = (self.w(0xd2), self.w(0xd4));
        self.res_zero(0x1c6, 0x1c8, f, bw, 0);
        let (f, bw) = (self.w(0xd6), self.w(0xd8));
        self.res_zero(0x1cc, 0x1ce, f, bw, 0);
        let f = self.w(0xda);
        self.res_cut(Some(0x1aa), Some(0x1ac), f, 400, 0);
        let f = self.w(0xdc);
        self.res_cut(Some(0x1ae), Some(0x1b0), f, 500, 0);

        if self.w(0x124) < 0 {
            self.sw(0x124, -self.w(0xde));
        } else {
            self.sw(0x124, self.w(0xde));
        }
        match mode {
            0 => self.sw(0x124, ((self.w(0x04) * self.w(0x124)) >> 14) & 0xfffe),
            1 => self.sw(0x124, (self.w(0x04) * self.w(0x124)) >> 15),
            _ => {}
        }

        let db = |v: &Vtm, off: usize| rom(DB_GAIN, v.w(off));
        self.sw(0x1ca, db(self, 0xe0));
        self.sw(0x1c4, db(self, 0xe2));
        self.sw(0x1f2, db(self, 0xe4));
        self.sw(0x1f4, db(self, 0xe6));
        self.sw(0x1f6, db(self, 0xe8));
        self.sw(0x200, self.w(0xea));
        self.sw(0x202, self.w(0xec));
        self.sw(0x128, db(self, 0xee));
        self.sw(0x126, self.w(0xf0));
        self.sw(0x1f0, db(self, 0xf2));
        self.sw(0x1d0, db(self, 0xf4));
        let t = (self.w(0xf6) - 2).max(0);
        self.sw(0x1ea, rom(DB_GAIN, t));
        self.sw(0x1ee, db(self, 0xf8));
    }

    /// FUN_1c0294e0: synthesize one frame from the 20-word block at PARAM_BASE.
    pub fn synth_frame(&mut self, rate: u32) {
        if self.w(0x218) > 1 {
            self.sw(0x218, 0);
        }
        if self.d(0x214) == 1 || self.w(0x218) == 1 {
            self.sw(0x218, self.w(0x218) + 1);
            self.sd(0x214, 0);
            for off in [0xe6, 0xd2, 0xd6, 0xd8, 0xda, 0xdc, 0xde, 0xe0, 0x1f8] {
                self.sw(off, 0);
            }
        }

        let mode = self.d(0x00);
        let f9 = self.w(0xe4);
        let t0 = match mode {
            0 => ((self.w(0x04) * f9) >> 14) & !1,
            1 => (self.w(0x04) * f9) >> 15,
            _ => f9,
        } as i16 as i32;

        let k = self.w(0x126);
        let mut f1s = ((self.w(0xd4) * k) >> 12) + ((0x1000 - k) >> 4);
        let f2s = ((self.w(0xe8) * k) >> 12) + ((0x1000 - k) >> 3);
        let f3s = (self.w(0xea) * k) >> 12;
        let f13 = self.w(0xec);
        let tilt = match mode {
            0 => (self.w(0x06) * f13) >> 15,
            1 => ((self.w(0x06) * f13) >> 14) & !1,
            _ => f13,
        } as i16 as i32;

        let bw2 = self.w(0xf0);
        let bw3 = self.w(0xf2);
        let bw1 = self.w(0xee);
        let av_idx = self.w(0xe6);
        let voic = (self.w(0xe2) - 12) as i16 as i32;

        let g_asp = (rom(DB_GAIN + 10, self.w(0xd2)) * self.w(0x1ee)) >> 12;
        let g_p3db = rom(DB_GAIN + 10, self.w(0xd8));
        let g_par = self.w(0x1f0);
        let g_p4 = (rom(DB_GAIN + 7, self.w(0xda)) * g_par) >> 15;
        let g_p5 = (rom(DB_GAIN + 6, self.w(0xdc)) * g_par) >> 15;
        let g_p6 = (rom(DB_GAIN + 5, self.w(0xde)) * g_par) >> 15;
        let g_byp = (rom(DB_GAIN + 5, self.w(0xe0)) * g_par) >> 12;

        let g2 = (rom(DB_GAIN + 13, self.w(0xd6)) * g_par) >> 15;
        let (a_p2, b_p2, c_p2) = self.res_cut(None, None, f2s as u16 as i32, 0xd2, g2);
        let g3 = (g_p3db * g_par) >> 15;
        let (a_p3, b_p3, c_p3) = self.res_cut(None, None, f3s as u16 as i32, 0x118, g3);

        let spf = self.d(0x114) as u32;
        for n in 0..spf as usize {
            let seed = (NOISE_MUL.wrapping_mul(self.w(0x210) as i16)).wrapping_add(NOISE_ADD);
            self.sw(0x210, seed as i32);
            let n1 = (((self.w(0x1a8) * 0x5ffe) >> 15) + ((seed >> 2) as i32)) as i16 as i32;
            self.sw(0x1a8, n1);
            let old160 = self.w(0x160);
            let cur15e = self.w(0x15e);
            self.sw(0x160, cur15e);
            self.sw(0x15e, n1);
            let mut noise = (n1 + ((old160 * NOISE_LPF + cur15e * self.w(0x1e2)) >> 12)) as i16;
            if self.w(0x204) < self.w(0x20c) {
                noise >>= 1;
            }
            let noise = noise as i32;

            let mut glot = 0;
            for _ in 0..4 {
                if self.w(0x206) - self.w(0x208) < self.w(0x204) {
                    self.sw(0x1fc, self.w(0x1fc) - self.w(0x1fe));
                    self.sw(0x1e8, self.w(0x1e8) + (self.w(0x1fc) >> 4));
                    self.sw(0x1fa, self.w(0x1f8));
                } else {
                    self.sw(0x1e8, 0);
                }
                let src = (self.w(0x1e8) * self.w(0x1ea)) >> 12;

                if self.w(0x204) == self.w(0x206) {
                    self.sw(0x204, 0);
                    self.sw(0x1f8, rom(DB_GAIN + 4, av_idx));
                    self.sw(0x206, t0);
                    self.sw(0x206, ((self.w(0x124) * t0) >> 12) + t0);
                    self.sw(0x124, -self.w(0x124));
                    self.sw(0x1ec, self.w(0x128));
                    if (f1s as i16) < 0xfa {
                        f1s = 0xfa;
                    }
                    match mode {
                        0 | 2 => self.sw(0x1e4, voic.wrapping_mul(0x446)),
                        1 => self.sw(0x1e4, voic.wrapping_mul(0x431)),
                        _ => {}
                    }
                    if self.w(0x1e4) < 0 {
                        self.sw(0x1e6, 0x7fff);
                    } else {
                        self.sw(0x1e6, 0x7fff - self.w(0x1e4));
                    }
                    self.sw(0x20c, 0);
                    if self.w(0x1f8) > 0 {
                        self.sw(0x20c, self.w(0x206) >> 1);
                    }
                    let open = ((self.w(0x200) * self.w(0x206)) >> 15) + self.w(0x202) + voic * 4;
                    self.sw(0x208, open);
                    if self.w(0x208) < 0x28 {
                        self.sw(0x208, 0x28);
                    } else if self.w(0x208) > 0x107 {
                        self.sw(0x208, 0x107);
                    }
                    let lim = (self.w(0x206) * 3) >> 2;
                    if lim <= self.w(0x208) {
                        self.sw(0x208, lim);
                    }
                    let step = rom(GLOTTAL_STEP, self.w(0x208));
                    self.sw(0x1fe, step);
                    let step1 = (step + 1) as i16 as i32;
                    self.sw(0x20e, step1);
                    let v = if self.w(0x208) <= 0x5f {
                        let q = (step1 * 10923) >> 15;
                        self.sw(0x20e, q);
                        (q as i16).wrapping_mul(self.w(0x208) as i16) as i32
                    } else {
                        let p = (self.w(0x208) as i16).wrapping_mul(step1 as i16) as i32;
                        self.sw(0x20e, p);
                        (p * 10923) >> 15
                    };
                    self.sw(0x1fc, v);

                    let g = self.w(0x1f2);
                    let a = self.res_clamp(rate, 0x1c0, 0x1c2, f3s as u16 as i32, bw3, g);
                    self.sw(0x1be, a);
                    let g = self.w(0x1f4);
                    let a = self.res_clamp(rate, 0x1ba, 0x1bc, f2s as u16 as i32, bw2, g);
                    self.sw(0x1b8, a);
                    let g = self.w(0x1f6);
                    let a = self.res_clamp(rate, 0x1b4, 0x1b6, f1s as u16 as i32, bw1, g);
                    self.sw(0x1b2, a);
                    if self.w(0x1b2) > 0x3fff {
                        self.sw(0x1b2, 0x3fff);
                    }
                    self.sw(0x1b2, self.w(0x1b2) << 1);

                    self.sw(0x20e, (tilt >> 3) - 31);
                    if self.w(0x20e) > 0x22 {
                        self.sw(0x20e, 0x22);
                    }
                    let ti = self.w(0x20e);
                    self.sw(0x1d6, rom(GSRC_B, ti));
                    self.sw(0x1d8, rom(GSRC_C, ti));
                    self.sw(0x1da, rom(GSRC_A, ti));
                }

                let old15c = self.w(0x15c);
                self.sw(0x15c, self.w(0x15a));
                let y = (src as i16 as i32 * self.w(0x1dc) + self.w(0x1e0) * old15c + self.w(0x15a) * self.w(0x1de)) >> 12;
                self.sw(0x15a, y);
                glot = self.w(0x15a);
                self.sw(0x204, self.w(0x204) + 1);
            }

            let v16c = ((self.w(0x1e4) * self.w(0x16c)) >> 15) as i16;
            let v16c = v16c.wrapping_add(((self.w(0x1e6) * glot) >> 15) as i16) as i32;
            self.sw(0x16c, v16c);

            let s156 = self.w(0x156);
            let old158 = self.w(0x158);
            let asp = (((self.w(0x1ec) * noise) >> 15) as i16).wrapping_add(v16c as i16) as i32;
            let v156 = (((asp * self.w(0x1fa)) >> 12) as i16).wrapping_add(((g_asp as i16 as i32 * noise) >> 15) as i16) as i32;
            self.sw(0x158, s156);
            self.sw(0x156, v156);

            let old154 = self.w(0x154);
            let cur152 = self.w(0x152);
            self.sw(0x154, cur152);
            let tilt_out = ((self.w(0x1d8) * s156 + self.w(0x1da) * old158 + self.w(0x1d6) * v156) >> 12) as i16 as i32;
            let v152 = (self.w(0x1d2) * cur152 + self.w(0x1d4) * old154 + self.w(0x1d0) * tilt_out) >> 12;
            self.sw(0x152, v152);

            if rate < 0x251d {
                self.sw(0x14e, (self.w(0x1ca) * self.w(0x152)) >> 13);
            } else {
                let old150 = self.w(0x150);
                let cur14e = self.w(0x14e);
                self.sw(0x150, cur14e);
                self.sw(0x14e, (self.w(0x1ca) * self.w(0x152) + self.w(0x1ce) * old150 + self.w(0x1cc) * cur14e) >> 12);
            }

            let old14c = self.w(0x14c);
            let cur14a = self.w(0x14a);
            self.sw(0x14c, cur14a);
            let old148 = self.w(0x148);
            self.sw(0x14a, (self.w(0x1c6) * cur14a + self.w(0x1c8) * old14c + self.w(0x1c4) * self.w(0x14e)) >> 12);
            let cur146 = self.w(0x146);
            self.sw(0x148, cur146);
            self.sw(0x146, (self.w(0x1be) * self.w(0x14a) + self.w(0x1c2) * old148 + self.w(0x1c0) * cur146) >> 12);

            let old144 = self.w(0x144);
            let cur142 = self.w(0x142);
            self.sw(0x144, cur142);
            self.sw(0x142, (self.w(0x1b8) * self.w(0x146) + self.w(0x1bc) * old144 + self.w(0x1ba) * cur142) >> 12);

            let old140 = self.w(0x140);
            let cur13e = self.w(0x13e);
            self.sw(0x140, cur13e);
            let old13c = self.w(0x13c);
            self.sw(0x13e, (self.w(0x1b4) * cur13e + self.w(0x1b6) * old140 + self.w(0x1b2) * self.w(0x142)) >> 12);

            let cur13a = self.w(0x13a);
            self.sw(0x13c, cur13a);
            let v13a = ((g_p6 as i16 as i32 * noise + cur13a * -0x1646 + old13c * -0x7cb) >> 12) as i16 as i32;
            self.sw(0x13a, v13a);

            if rate < 0x2581 {
                self.sw(0x136, 0);
            } else {
                let old138 = self.w(0x138);
                let cur136 = self.w(0x136);
                self.sw(0x138, cur136);
                self.sw(0x136, (g_p5 as i16 as i32 * noise + self.w(0x1b0) * old138 + self.w(0x1ae) * cur136) >> 12);
            }

            let old134 = self.w(0x134);
            let cur132 = self.w(0x132);
            self.sw(0x134, cur132);
            let old130 = self.w(0x130);
            self.sw(0x132, (g_p4 as i16 as i32 * noise + self.w(0x1ac) * old134 + self.w(0x1aa) * cur132) >> 12);

            let cur12e = self.w(0x12e);
            self.sw(0x130, cur12e);
            let old12c = self.w(0x12c);
            self.sw(0x12e, (a_p3 as i16 as i32 * noise + old130 * c_p3 + b_p3 * cur12e) >> 12);

            let cur12a = self.w(0x12a);
            self.sw(0x12c, cur12a);
            let v12a = ((a_p2 as i16 as i32 * noise + old12c * c_p2 + b_p2 * cur12a) >> 12) as i16 as i32;
            self.sw(0x12a, v12a);

            let byp = ((g_byp as i16 as i32 * noise) >> 15) as i16 as i32;
            let mut out = ((((byp - v12a) - self.w(0x132)) - (v13a - self.w(0x13e))) + self.w(0x12e) + self.w(0x136)) as i16;
            if out >= 0x4000 {
                out = 0x3fff;
            } else if out < -0x4000 {
                out = -0x4000;
            }
            self.sw(OUT_BASE + n * 2, (out as i32) * 2);

            if self.d(0x21c) != 0 {
                self.track_overload();
            }
        }
    }

    fn track_overload(&mut self) {
        if self.w(0xf4) != self.d(0x18c) {
            self.sd(0x190, self.d(0x18c));
            self.sd(0x18c, self.w(0xf4));
        }
        const PAIRS: [(usize, usize); 12] = [
            (0x15a, 0x17e), (0x156, 0x172), (0x152, 0x170), (0x14e, 0x17c),
            (0x14a, 0x17a), (0x146, 0x178), (0x142, 0x176), (0x13e, 0x174),
            (0x12a, 0x180), (0x12e, 0x182), (0x132, 0x184), (0x136, 0x186),
        ];
        for (src, peak) in PAIRS {
            let v = (self.w(src) as i16).wrapping_abs() as i32;
            if self.w(peak) < v {
                self.sw(peak, v);
            }
        }
        if PAIRS[..8].iter().any(|&(_, p)| self.w(p) > 28000) {
            self.sd(0x21c, 0);
            self.sd(0x188, 1);
            for (dst, src) in [(0x194, 0x140), (0x196, 0x144), (0x198, 0x148), (0x19a, 0x14c),
                               (0x19c, 0x150), (0x19e, 0x154), (0x1a0, 0x158), (0x1a2, 0x15c)] {
                self.sw(dst, self.w(src));
            }
        }
    }
}
