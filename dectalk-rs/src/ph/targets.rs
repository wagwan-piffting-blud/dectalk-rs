//! Per-phoneme parameter targets and transitions (FUN_1c019e60 and helpers).
//!
//! Tracks are 0x24-byte records at ph+0x24..=ph+0x240 (see `frame_tracks_interp`). Per-voice
//! tables: *(ph+0x11b4) locus data, *(ph+0x11b8) breakpoint lists, *(ph+0x11bc) targets,
//! *(ph+0x11c0) context targets. `aux` scratch words: +0 target, +4 locus delta, +6 transition
//! frames, +8 locus time, +0xa next phoneme, +0xc blend factor, +0x10 current track, +0x14 track
//! type (byte), +0x16 flag, +0x18 current phoneme.

use super::duration::to_frames;
use super::mem::Mem;
use super::{ph_of, wrap};

const FEATURES: u32 = 0x1c06_73fc;
const FEATURES2: u32 = 0x1c06_7388;
const PLACE_BEFORE: u32 = 0x1c06_72a0;
const PLACE_AFTER: u32 = 0x1c06_7314;
const DUR_MINIMUM: u32 = 0x1c06_70d0;
const DUR_BURST: u32 = 0x1c06_71b8;
const CONTEXT_ROW: u32 = 0x1c06_8220;
const LOCUS_INDEX: u32 = 0x1c06_8a9a;
const TRACK_TYPE: u32 = 0x1c06_9a78;
const RAMP_GAIN: u32 = 0x1c06_9a88;
const TRACK_DEFAULT: u32 = 0x1c06_9aec;

#[inline]
fn at(base: u32, i: i32, scale: i32) -> u32 {
    base.wrapping_add((i * scale) as u32)
}

#[inline]
fn tw(m: &Mem, base: u32, i: i32) -> i32 {
    m.s(at(base, i, 2))
}

#[inline]
fn feat(m: &Mem, v: i32) -> i32 {
    m.r16(at(FEATURES, v, 2)) as u16 as i32
}

#[inline]
fn feat2(m: &Mem, v: i32) -> i32 {
    m.r16(at(FEATURES2, v, 2)) as u16 as i32
}

#[inline]
fn phone(m: &Mem, ph: u32, i: i32) -> i32 {
    m.s(at(ph + 0x302, i, 2))
}

#[inline]
fn phone_or0(m: &Mem, ph: u32, i: i32) -> i32 {
    if i < 0 || m.s(ph + 0x11ae) <= i { 0 } else { phone(m, ph, i) }
}

#[inline]
fn flags(m: &Mem, ph: u32, i: i32) -> u32 {
    m.r32(at(ph + 0x570, i, 4))
}

struct Ctx {
    ctx: u32,
    ph: u32,
    aux: u32,
}

impl Ctx {
    fn track(&self, m: &Mem) -> u32 {
        m.r32(self.aux + 0x10)
    }
    fn track_index(&self, m: &Mem) -> i32 {
        ((self.track(m).wrapping_sub(self.ph) as i32) - 0x24) / 0x24
    }
    fn ty(&self, m: &Mem) -> i8 {
        m.r8(self.aux + 0x14) as i8
    }
    fn tv(&self, m: &Mem) -> i32 {
        m.s(self.aux)
    }
    fn set_tv(&self, m: &mut Mem, v: i32) {
        m.w16(self.aux, v);
    }
    fn tdur(&self, m: &Mem) -> i32 {
        m.s(self.aux + 6)
    }
    fn set_tdur(&self, m: &mut Mem, v: i32) {
        m.w16(self.aux + 6, v);
    }
    fn cur(&self, m: &Mem) -> i32 {
        m.s(self.aux + 0x18)
    }
    fn next(&self, m: &Mem) -> i32 {
        m.s(self.aux + 0xa)
    }
    fn dur(&self, m: &Mem) -> i32 {
        m.s(self.ph + 0x11ac)
    }
    fn pos(&self, m: &Mem) -> i32 {
        m.s(self.ph + 0x201c)
    }
}

/// FUN_1c01ba90: place-of-articulation F2 adjustments (only for tracks 1 and 2).
fn place_adjust(m: &Mem, c: &Ctx, i: i32, k: i32) -> i32 {
    let ph = c.ph;
    let mut r: i32 = 0;
    let cur = phone_or0(m, ph, i);
    let next = phone_or0(m, ph, i + 1);
    let prev = phone_or0(m, ph, i - 1);
    let t = c.track(m);
    if t == ph + 0x6c
        && (feat(m, cur) & 4) != 0
        && cur != 0xf
        && (matches!(prev, 0x18 | 0x1a | 0x1d) || matches!(next, 0x18 | 0x1a | 0x1d))
    {
        r = -0x96;
    }
    if t != ph + 0x48 {
        return r;
    }
    let front = (0 < cur && cur < 6) || cur == 0x12;
    if next == 0x1e {
        if front {
            r = -0x96;
        }
        if (cur == 7 || cur == 0xc) && k == 1 {
            r = -0xfa;
        }
        if (cur == 7 || cur == 0xc) && 1 < k {
            r = -0x15e;
        }
    }
    if matches!(prev, 0x18 | 0x1b | 0x1e) && front {
        r = -0x96;
    }
    let mut add_next = false;
    if cur == 0xe {
        if (feat2(m, prev) & 8) != 0 {
            r = 200;
        }
        add_next = true;
    } else if cur == 0x10 && 0 < k {
        add_next = true;
    }
    if add_next && (feat2(m, next) & 8) != 0 {
        r = wrap(r + 200);
    }
    let fl = flags(m, ph, i);
    let half = r >> 1;
    let v;
    if (fl & 3) == 0 {
        r = wrap(r + half);
        if cur == 0x10 && 0 < k {
            return 400;
        }
        v = r;
    } else if (fl & 0x1e0) < 0xa0 {
        v = r;
    } else {
        v = half;
    }
    if 400 < v {
        400
    } else if -0x191 < v {
        v
    } else {
        -400
    }
}

/// FUN_1c01a330: raw target for the current track at phoneme `i` (negative = breakpoint list).
fn raw_target(m: &mut Mem, c: &Ctx, i: i32) -> i32 {
    let ph = c.ph;
    let t = c.track(m);
    let trk = c.track_index(m) as i16 as i32;
    let mut base = trk * 0x39;
    if ph + 0x90 <= t {
        base -= 0x39;
    }
    let prev = phone_or0(m, ph, i - 1);
    let cur = phone_or0(m, ph, i);
    let next = phone_or0(m, ph, i + 1);
    let ty = m.r8(at(TRACK_TYPE, trk, 1)) as i8;
    m.w8(c.aux + 0x14, ty as u8);

    if ty < 3 {
        if ty == 1 {
            m.w16(c.aux + 0x16, 0);
            return if (feat(m, cur) & 0x80) != 0 { 400 } else { 0x122 };
        }
        if ty == 0 {
            if trk == 7 {
                let fl = flags(m, ph, i);
                let mut v = tw(m, m.r32(ph + 0x11bc), base + cur);
                if (fl & 0x800) != 0 {
                    v -= 7;
                }
                if (feat(m, cur) & 0x40) != 0 && (feat(m, prev) & 2) == 0 {
                    v = 0;
                }
                if cur == 0x1c && (feat(m, prev) & 2) != 0 && (fl & 1) == 0 {
                    v = 0x36;
                }
                if (fl & 3) != 0 {
                    return wrap(v);
                }
                return if -1 < wrap(v - 4) { wrap(v - 4) } else { 0 };
            }
            if cur != 0x1c {
                return 0;
            }
            return if tw(m, PLACE_BEFORE, next) == 1 { 0x35 } else { 0x3c };
        }
        if ty != 2 {
            return cur;
        }
        if t != ph + 0x240 {
            let mut v = tw(m, CONTEXT_ROW, cur);
            if 0 < v {
                let mut col = tw(m, PLACE_BEFORE, next);
                if next == 0 {
                    col = tw(m, PLACE_AFTER, prev);
                }
                col -= 1;
                if col == 4 {
                    col = 2;
                }
                v = tw(m, m.r32(ph + 0x11c0), wrap(v + col * 6 - 9 + trk));
                if (m.r32(at(ph + 0x574, i, 4)) & 0x800) != 0 && 3 < v {
                    v -= 4;
                }
            }
            return v;
        }
        if cur != 0 && cur == 0x1c {
            return 0x14;
        }
        if (flags(m, ph, i) & 0x800) != 0 {
            return 0x14;
        }
        let f = feat(m, cur);
        if (f & 0x20) != 0 {
            if (f & 2) == 0 {
                return 7;
            }
            if (f & 0x40) == 0 && c.cur(m) != 0x37 {
                return 7;
            }
            return 0x28;
        }
        if (f & 0x80) != 0 {
            return 6;
        }
        if tw(m, PLACE_BEFORE, cur) != 1 && tw(m, PLACE_AFTER, cur) != 1 {
            return 0;
        }
        return if m.s(ph + 0x11b0) != 0 { 3 } else { 6 };
    }

    let tab = m.r32(ph + 0x11bc);
    let lists = m.r32(ph + 0x11b8);
    let mut v = tw(m, tab, base + cur);
    if v < -1 {
        return v;
    }
    if v == -1 {
        v = tw(m, tab, next + base);
        if v == -1 {
            let nn = phone_or0(m, ph, i + 2);
            v = tw(m, tab, nn + base);
            if v == -1 {
                v = tw(m, tab, prev + base);
                if v < -1 {
                    let mut k = v;
                    while tw(m, lists, -k) != -1 {
                        k -= 1;
                    }
                    v = tw(m, lists, -k - 1);
                }
                if v == -1 {
                    v = tw(m, TRACK_DEFAULT, trk);
                }
            }
        }
    }
    if v < -1 {
        v = tw(m, lists, -v);
    }
    let fc = feat(m, cur);
    if trk == 0 && (fc & 0x20) != 0 && (fc & 0x2000) == 0 && (feat(m, prev) & 1) != 0 {
        v = wrap(v + 0x28);
    }
    if (cur == 0x20 || cur == 0x24) && trk == 5 && tw(m, PLACE_BEFORE, next) != 1 {
        v = wrap(v + 0x3c);
    }
    if matches!(cur, 0x20 | 0x24 | 0x21)
        && trk == 6
        && ((feat2(m, next) & 0x40) != 0 || (feat2(m, prev) & 0x80) != 0)
    {
        return 0x640;
    }
    v
}

/// FUN_1c01ba30: target at the start of phoneme `i` (first breakpoint for list targets).
fn start_target(m: &mut Mem, c: &Ctx, i: i32) -> i32 {
    let mut v = raw_target(m, c, i);
    if v < -1 {
        v = tw(m, m.r32(c.ph + 0x11b8), -v);
        if c.ty(m) == 3 {
            v = wrap(v + place_adjust(m, c, i, 0));
        }
    }
    v
}

/// FUN_1c01bcb0: target at the end of phoneme `i` (last breakpoint for list targets).
fn end_target(m: &mut Mem, c: &Ctx, i: i32) -> i32 {
    let mut v = raw_target(m, c, i);
    if v < -1 {
        let lists = m.r32(c.ph + 0x11b8);
        let mut k = -v;
        while tw(m, lists, k) != -1 {
            k += 1;
        }
        v = tw(m, lists, k - 1);
        if c.ty(m) == 3 {
            v = wrap(v + place_adjust(m, c, i, 1));
        }
    }
    v
}

/// FUN_1c01bf90: breakpoint time (frames) scaled by duration.
fn breakpoint_time(m: &mut Mem, ph: u32, a: i32, dmin: i32, k: i32) -> i32 {
    let s3 = wrap(a * 10 + 5);
    m.w16(ph + 0x11c6, k);
    let s1 = wrap((dmin * 0x47) >> 1);
    m.w16(ph + 0x11c4, s1);
    let s2 = wrap((s1 * k) >> 14);
    let mut s4 = s3;
    if s1 < s3 {
        s4 = wrap(dmin * 0x47 - s3);
    }
    m.w16(ph + 0x11c4, s1 - s4);
    let s5 = ((k + 0x4000) as u32 >> 1) as i32 as i16 as i32;
    m.w16(ph + 0x11c6, s5);
    let mut s4b = wrap((wrap(s1 - s4) * s5) >> 14);
    if s2 < s4b {
        s4b = s2;
    }
    let mut r = wrap(s2 - s4b);
    if s1 < s3 {
        r = wrap(s2 * 2 - r);
    }
    if r < 0x47 {
        r = 0x47;
    }
    r >> 6
}

/// FUN_1c01bd20: expand a breakpoint list into the track's (time, slope) stream at *ptr.
fn expand_breakpoints(m: &mut Mem, c: &Ctx, idx: i32, dmin: i32, k: i32, ptr: &mut u32) {
    let ph = c.ph;
    let aux = c.aux;
    let t = c.track(m);
    m.w32(t + 0x1c, *ptr);
    let lists = m.r32(ph + 0x11b8);
    let first = m.r16(at(lists, idx, 2)) as u16 as i32;
    let mut p6 = first;
    let mut lc: i32 = 0;
    if c.ty(m) == 3 {
        m.w16(aux + 0xc, 0x666);
        let s9 = wrap(m.s(t + 0x18) - first);
        m.w16(ph + 0x11c4, s9);
        let s10 = m.s(aux + 0xc);
        m.w16(ph + 0x11c6, s10);
        let adj = place_adjust(m, c, c.pos(m), 0);
        p6 = p6 + adj + ((s10 * s9) >> 14);
    }
    m.w16(t, p6);
    let mut last_time: i32 = 0;
    let mut started = false;
    let mut p1: i32;
    let mut kk = idx;
    loop {
        if started {
            let lists = m.r32(ph + 0x11b8);
            p1 = m.r16(at(lists, kk, 2)) as u16 as i32;
            if c.ty(m) == 3 {
                let s9 = m.s(t + 0x16);
                if 0 < s9 {
                    m.w16(ph + 0x11c4, s9 - p1);
                    let g = m.s(aux + 0xc);
                    m.w16(ph + 0x11c6, g);
                    p1 += (g * m.s(ph + 0x11c4)) >> 14;
                }
                lc += 1;
                p1 += place_adjust(m, c, c.pos(m), lc);
            }
        } else {
            started = true;
            p1 = p6;
        }
        let k1 = kk + 1;
        let lists = m.r32(ph + 0x11b8);
        let s9 = tw(m, lists, k1);
        let time = if s9 == -1 { c.dur(m) as u16 as i32 } else { breakpoint_time(m, ph, s9, dmin, k) };
        m.w16(*ptr, time);
        let old = *ptr;
        *ptr = ptr.wrapping_add(2);
        let d = wrap(time - last_time);
        if d == 0 {
            m.w16(old + 2, 0);
        } else {
            let s7 = wrap(wrap(p1 - p6) * 8);
            m.w16(ph + 0x11c6, s7);
            if d < 0x32 {
                let g = tw(m, RAMP_GAIN, d);
                m.w16(ph + 0x11c4, g);
                m.w16(*ptr, (m.s(ph + 0x11c6) * g) >> 14);
            } else {
                m.w16(*ptr, s7 / d);
            }
            p6 = p1;
            last_time = time;
        }
        kk += 2;
        *ptr = ptr.wrapping_add(2);
        if tw(m, m.r32(ph + 0x11b8), k1) == -1 {
            break;
        }
    }
    m.w16(t + 0x1a, p1);
    let bp = m.r32(t + 0x1c);
    m.w16(t + 2, m.s(bp));
    m.w32(t + 0x1c, bp + 2);
    let bp = m.r32(t + 0x1c);
    m.w16(t + 4, m.s(bp));
    m.w32(t + 0x1c, bp + 2);
}

/// FUN_1c01b9c0: locus overshoot for short syllables.
fn locus_overshoot(m: &mut Mem, ph: u32, a: i32, b: i32, dur: i32) {
    let aux = m.r32(ph + 0x2848);
    if 0x10 < dur {
        m.w16(aux + 4, 0);
        m.w16(aux + 8, 0);
        return;
    }
    m.w16(ph + 0x11c6, 0x1cc);
    let s = wrap((a - b) * (0x10 - dur));
    m.w16(ph + 0x11c4, s);
    m.w16(aux + 8, 0xd - (dur >> 2));
    m.w16(aux + 4, (s * 0x1cc) >> 14);
}

/// FUN_1c01b690: consonant-vowel locus interpolation; returns 1 if applied.
fn locus(m: &mut Mem, c: &Ctx, pa: i32, pb: i32, initial: bool, pc: i32) -> i32 {
    let ph = c.ph;
    let aux = c.aux;
    let a = phone_or0(m, ph, pa);
    let b = phone_or0(m, ph, pb);
    let cc = phone_or0(m, ph, pc);
    let (s4, s6) = if initial {
        (tw(m, PLACE_AFTER, a), tw(m, PLACE_BEFORE, b))
    } else {
        (tw(m, PLACE_BEFORE, a), tw(m, PLACE_AFTER, b))
    };
    let t = c.track(m);
    if !(t <= ph + 0x6c && s4 == 4 && s6 != 4) {
        return 0;
    }
    let (bit, v4) = if initial {
        (feat2(m, b) & 0x40, start_target(m, c, pb))
    } else {
        (feat2(m, b) & 0x80, end_target(m, c, pb))
    };
    let row = if s6 == 5 { 3 } else { s6 };
    let u7 = row * 0x39 + a;
    let li = tw(m, LOCUS_INDEX, u7);
    if li == 0 {
        return 0;
    }
    let tb = m.r32(ph + 0x11b4);
    let idx = wrap(wrap(c.track_index(m)) * 3 + li);
    let s5 = tw(m, tb, idx);
    let mut s10 = tw(m, tb, idx + 1);
    let d = to_frames(tw(m, tb, idx + 2));
    m.w16(aux + 6, d);
    if s6 == 5 && ph + 0x24 < t && (feat2(m, a) & 6) == 0 {
        s10 = wrap((s10 >> 1) + 0x32);
    }
    if bit != 0 && t == ph + 0x48 {
        s10 = wrap(s10 + (0x19 - (s10 >> 2)));
        m.w16(aux + 6, (d as i16 as i32 >> 1) + 2);
    }
    m.w16(ph + 0x11c4, s10);
    m.w16(ph + 0x11c8, 100);
    let diff = wrap(v4 - s5);
    m.w16(ph + 0x11c6, diff);
    m.w16(aux, wrap((diff * s10) / 100 + s5));
    if (feat(m, b) & 4) != 0 && (feat(m, cc) & 4) != 0 && c.track(m) == ph + 0x48 {
        let s6b = if initial { end_target(m, c, pc) } else { start_target(m, c, pc) };
        let dur = m.s(at(ph + 0xa48, pa, 2));
        locus_overshoot(m, ph, s6b, v4, dur);
        let over = m.s(aux + 4);
        m.w16(ph + 0x11c4, s10);
        m.w16(ph + 0x11c8, 100);
        let s6c = wrap(v4 + over - s5);
        m.w16(ph + 0x11c6, s6c);
        m.w16(aux, wrap((s6c * s10) / 100 - over + s5));
    }
    1
}

/// FUN_1c01a7d0: onset transition target/duration for the current track.
fn onset_transition(m: &mut Mem, c: &Ctx, scale: i32, prev: i32, pfeat: i32, cfeat: i32, pflags: i32) {
    let ph = c.ph;
    let t = c.track(m);
    let ty = c.ty(m);
    if ty == 3 {
        if (cfeat & 0x10) != 0 {
            if (cfeat & 0x200) == 0 {
                c.set_tdur(m, 7);
                if (pfeat & 0x200) == 0 {
                    if c.cur(m) == 0x1c {
                        c.set_tv(m, (m.s(t + 0x18) + c.tv(m)) >> 1);
                    }
                } else {
                    let v = (m.s(t + 0x18) + c.tv(m)) >> 1;
                    c.set_tv(m, v);
                    if prev == 0x1b {
                        if t == ph + 0x24 {
                            c.set_tv(m, v + 0x50);
                        }
                    } else if prev == 0x1a && t != ph + 0x24 {
                        c.set_tdur(m, 0xb);
                    }
                }
            } else {
                if (pfeat & 0x200) == 0 {
                    c.set_tv(m, (m.s(t) + c.tv(m)) >> 1);
                }
                c.set_tdur(m, 5);
            }
        }
        if c.cur(m) == 0 {
            if c.pos(m) < 2 {
                c.set_tv(m, m.s(t + 0x16));
            } else {
                c.set_tv(m, m.s(t + 0x18));
            }
            c.set_tdur(m, c.dur(m));
        } else {
            let i = c.pos(m);
            locus(m, c, i - 1, i, true, i - 2);
            let i = c.pos(m);
            locus(m, c, i, i - 1, false, i + 1);
            if (pfeat & 0x40) != 0 && (pfeat & 2) == 0 && t == ph + 0x24 {
                c.set_tv(m, c.tv(m) + 100);
            }
            if (cfeat & 0x20) != 0 {
                c.set_tdur(m, 5);
                if t == ph + 0x24 {
                    c.set_tdur(m, 3);
                }
                if (cfeat & 0x40) != 0 {
                    c.set_tdur(m, c.dur(m));
                }
            }
            if (cfeat & 0x80) != 0 {
                c.set_tdur(m, c.dur(m));
                if t == ph + 0x24 {
                    c.set_tdur(m, 0);
                } else {
                    let cur = c.cur(m);
                    if (cur == 0x20 || cur == 0x24) && tw(m, PLACE_AFTER, prev) == 1 {
                        if t == ph + 0x48 {
                            c.set_tv(m, c.tv(m) - 100);
                            if (feat2(m, prev) & 0x80) != 0 {
                                c.set_tv(m, c.tv(m) - 100);
                            }
                        }
                        if t == ph + 0x6c {
                            c.set_tv(m, c.tv(m) - 100);
                        }
                    } else if t == ph + 0x48 && cur == 0x1f && (feat2(m, prev) & 0x80) != 0 {
                        c.set_tv(m, c.tv(m) - 0x96);
                    }
                }
            }
        }
        if (cfeat & 0x20) == 0 && tw(m, PLACE_AFTER, prev) != 4 && 0 < c.tdur(m) {
            m.w16(ph + 0x11c4, c.tdur(m));
            m.w16(ph + 0x11c6, scale);
            c.set_tdur(m, ((c.tdur(m) * scale) >> 14) + 1);
        }
    } else if ty == 1 {
        c.set_tdur(m, 0);
        if (pfeat & 0x80) != 0 && (cfeat & 0x80) == 0 {
            c.set_tv(m, 400);
            c.set_tdur(m, 0xd);
        }
    } else if ty != 4 {
        if ty == 2 || ty == 0 {
            let t0 = m.s(t);
            let s7 = wrap(t0 - 10);
            if c.tv(m) < s7 || (pfeat & 0x40) != 0 || prev == 0x37 {
                c.set_tv(m, s7);
                if (cfeat & 0x20) == 0 {
                    c.set_tdur(m, 3);
                }
                if t == ph + 0x120 {
                    if prev == 0 && (cfeat & 2) != 0 {
                        c.set_tv(m, c.tv(m) - 8);
                        c.set_tdur(m, 7);
                    }
                    if (pfeat & 0x20) != 0 {
                        c.set_tv(m, t0 - 4);
                    }
                    if (pfeat & 0x40) != 0 {
                        c.set_tv(m, m.s(t) - 5);
                    }
                }
            }
            if (pfeat & 0x80) != 0 && (cfeat & 2) != 0 {
                c.set_tdur(m, 0);
            }
            if (cfeat & 0x80) != 0 && (pfeat & 2) != 0 && t == ph + 0x120 {
                c.set_tdur(m, 0);
            }
            if c.tv(m) < wrap(m.s(t + 0x18) - 10) {
                c.set_tv(m, m.s(t + 0x18) - 0xd);
                if c.cur(m) == 0 {
                    c.set_tdur(m, 0xb);
                }
                if t == ph + 0x120 {
                    c.set_tdur(m, 0);
                }
            }
            if t == ph + 0x18c && (c.cur(m) == 0x36 || c.cur(m) == 0x37) {
                c.set_tdur(m, c.dur(m) - 2);
                c.set_tv(m, m.s(t) - 0x1e);
            }
            if t == ph + 0x144 {
                let cur = c.cur(m);
                if matches!(cur, 0 | 0x25 | 0x27 | 0x29 | 0x2b) && (pfeat & 2) != 0 && (pfeat & 0x20) == 0 {
                    if cur == 0 {
                        c.set_tv(m, 0x34);
                        c.set_tdur(m, 0xd);
                    } else {
                        c.set_tv(m, 0x30);
                        c.set_tdur(m, 7);
                    }
                }
            }
            if t == ph + 0x240 {
                c.set_tdur(m, 4);
                if prev == 0 {
                    c.set_tv(m, m.s(t));
                }
                if c.cur(m) == 0 {
                    c.set_tv(m, m.s(ph + 0x2d6));
                }
                if (pfeat & 0x2000) != 0 || (cfeat & 0x2000) != 0 {
                    c.set_tdur(m, 0);
                }
            }
        }
    } else {
        c.set_tdur(m, 6);
        if (cfeat & 2) == 0 {
            c.set_tdur(m, 3);
        } else if t == ph + 0xb4 && (pfeat & 2) == 0 {
            c.set_tdur(m, 8);
            c.set_tv(m, (m.s(ph + 0x24) >> 3) + m.s(t));
        }
        let step = wrap((ph as i32 - t as i32 + 0xfc) / 0x24);
        if prev == 0 {
            c.set_tv(m, step * 0x32 + m.s(t));
            c.set_tdur(m, 8);
        } else if c.cur(m) == 0 {
            c.set_tv(m, step * 0x32 + m.s(t + 0x18));
            let pp = m.s(at(ph + 0x2fe, c.pos(m), 2));
            if (feat(m, pp) & 2) == 0 && (pflags & 0x800) != 0 && t == ph + 0xb4 {
                c.set_tv(m, 0x104);
            }
            c.set_tdur(m, 8);
        }
        if m.s(c.aux + 0x16) != 0 {
            c.set_tv(m, c.tv(m) - 0x14);
        }
        if (pfeat & 0x80) != 0 {
            let t0 = m.s(t);
            c.set_tv(m, t0);
            if t == ph + 0xd8 && (prev == 0x20 || prev == 0x24) && tw(m, PLACE_BEFORE, c.cur(m)) != 1 {
                c.set_tdur(m, 9);
                c.set_tv(m, t0 + 0x3c);
            }
            if t == ph + 0xb4 {
                c.set_tv(m, c.tv(m) + 0x46);
                c.set_tdur(m, 0x10);
            }
        }
        if (cfeat & 0x80) != 0 {
            c.set_tdur(m, 0);
        }
    }
    if c.dur(m) < c.tdur(m) {
        c.set_tdur(m, c.dur(m));
    }
    if 0x14 < c.tdur(m) {
        c.set_tdur(m, 0x14);
    }
    if c.tv(m) < 0 {
        c.set_tv(m, 0);
    }
}

/// FUN_1c01adc0: offset transition target/duration for the current track.
fn offset_transition(m: &mut Mem, c: &Ctx, scale: i32, cfeat: i32, nfeat: i32, nflags: i32) {
    let ph = c.ph;
    let t = c.track(m);
    let ty = c.ty(m);
    let mut to_end = false;
    'body: {
        if ty == 3 {
            if (cfeat & 0x10) != 0 {
                c.set_tdur(m, 7);
                if (cfeat & 0x200) == 0 {
                    if (nfeat & 0x200) == 0 {
                        if c.next(m) == 0x1c {
                            c.set_tv(m, (m.s(t + 0x1a) + c.tv(m)) >> 1);
                        }
                    } else {
                        c.set_tv(m, (m.s(t + 0x16) + c.tv(m)) >> 1);
                        if t == ph + 0x6c {
                            c.set_tdur(m, 10);
                        }
                        if c.next(m) == 0x1b && t == ph + 0x24 {
                            c.set_tv(m, c.tv(m) + 0x50);
                        }
                    }
                } else {
                    c.set_tdur(m, 6);
                    if (nfeat & 0x200) == 0 {
                        c.set_tdur(m, 3);
                        c.set_tv(m, (m.s(t + 0x1a) + c.tv(m)) >> 1);
                    }
                }
            }
            if c.next(m) == 0 {
                c.set_tdur(m, 0);
            } else {
                let i = c.pos(m);
                locus(m, c, i + 1, i, false, i + 2);
                let i = c.pos(m);
                locus(m, c, i, i + 1, true, i - 1);
                if (cfeat & 0x20) != 0 {
                    c.set_tdur(m, 5);
                    if t == ph + 0x24 {
                        c.set_tdur(m, 3);
                    }
                    if (cfeat & 0x40) != 0 {
                        c.set_tdur(m, c.dur(m));
                        if t == ph + 0x24 && (cfeat & 2) == 0 {
                            c.set_tv(m, c.tv(m) + 100);
                        }
                    }
                }
                if (cfeat & 0x80) != 0 {
                    c.set_tdur(m, c.dur(m));
                    if t == ph + 0x24 {
                        c.set_tdur(m, 0);
                    } else {
                        let cur = c.cur(m);
                        let nx = c.next(m);
                        if (cur == 0x20 || cur == 0x24) && tw(m, PLACE_BEFORE, nx) == 1 {
                            if t == ph + 0x48 {
                                c.set_tv(m, c.tv(m) - 100);
                                if (feat2(m, nx) & 0x40) != 0 {
                                    c.set_tv(m, c.tv(m) - 100);
                                }
                            }
                            if t == ph + 0x6c {
                                c.set_tv(m, c.tv(m) - 100);
                            }
                        } else if t == ph + 0x48 && cur == 0x1f && (feat2(m, nx) & 0x40) != 0 {
                            c.set_tv(m, c.tv(m) - 0x96);
                        }
                    }
                }
            }
            if (cfeat & 0x20) == 0 && tw(m, PLACE_BEFORE, c.next(m)) != 4 && 0 < c.tdur(m) {
                m.w16(ph + 0x11c4, c.tdur(m));
                m.w16(ph + 0x11c6, scale);
                c.set_tdur(m, ((c.tdur(m) * scale) >> 14) + 1);
            }
            break 'body;
        }
        if ty == 1 {
            c.set_tdur(m, 0);
            if (nfeat & 0x80) == 0 || (cfeat & 0x80) != 0 {
                break 'body;
            }
            c.set_tv(m, 400);
            c.set_tdur(m, 0xd);
            if c.next(m) != 0x24 {
                break 'body;
            }
            to_end = true;
            break 'body;
        }
        if ty == 4 {
            c.set_tdur(m, 6);
            if (cfeat & 2) == 0 {
                c.set_tdur(m, 3);
            } else if t == ph + 0xb4 && (nfeat & 2) == 0 {
                c.set_tdur(m, 8);
                c.set_tv(m, (m.s(ph + 0x24) >> 3) + m.s(t + 0x1a));
                if m.s(ph + 0x11b0) == 0 {
                    c.set_tdur(m, 0x10);
                }
            }
            let step = wrap((ph as i32 - t as i32 + 0xfc) / 0x24);
            if c.next(m) == 0 {
                c.set_tv(m, step * 0x32 + m.s(t + 0x1a));
                c.set_tdur(m, 8);
            } else if c.cur(m) == 0 {
                c.set_tv(m, step * 0x32 + m.s(t + 0x16));
                c.set_tdur(m, 8);
            }
            if (nfeat & 0x80) != 0 {
                let e = m.s(t + 0x1a);
                c.set_tv(m, e);
                let nx = c.next(m);
                if t == ph + 0xd8 && (nx == 0x20 || nx == 0x24) && tw(m, PLACE_AFTER, c.cur(m)) != 1 {
                    c.set_tdur(m, 9);
                    c.set_tv(m, e + 0x3c);
                }
                if t == ph + 0xb4 {
                    c.set_tv(m, c.tv(m) + 100);
                    c.set_tdur(m, 0x10);
                }
            }
            if (cfeat & 0x80) != 0 {
                c.set_tdur(m, 0);
            }
            break 'body;
        }
        if ty != 2 && ty != 0 {
            break 'body;
        }
        let s6 = wrap(m.s(t + 0x16) - 10);
        if c.tv(m) < s6 {
            c.set_tv(m, s6);
            if c.cur(m) == 0 {
                c.set_tdur(m, 0xb);
            }
        }
        if t == ph + 0x120 && c.tv(m) < m.s(t + 0x16) {
            let cur = c.cur(m);
            if !matches!(cur, 0x26 | 0x28 | 0x37 | 0x2c | 0x2a) {
                c.set_tdur(m, 0);
                if (cfeat & 0x40) != 0 || cur == 0x36 {
                    if (cfeat & 2) == 0 {
                        c.set_tv(m, 0);
                    } else {
                        c.set_tdur(m, 7);
                        c.set_tv(m, m.s(t + 0x1a) - 3);
                    }
                    break 'body;
                }
            }
        }
        if (nfeat & 0x80) != 0 && (cfeat & 2) != 0 {
            c.set_tdur(m, 0);
        }
        if (cfeat & 0x80) != 0 {
            if (nfeat & 2) == 0 || (nfeat & 0x20) != 0 || (nflags & 0x800) != 0 {
                c.set_tdur(m, 6);
            } else {
                c.set_tdur(m, nflags & 0x800);
            }
        }
        let cur = c.cur(m);
        let mut s5 = wrap(m.s(t + 0x1a) - 10);
        if 0x2c < cur {
            c.set_tdur(m, 2);
            if cur < 0x36 {
                s5 = m.s(t + 0x1a);
            }
        }
        if c.tv(m) < s5 {
            c.set_tdur(m, 3);
            c.set_tv(m, s5 - 3);
        }
        if t == ph + 0x120 && (c.tv(m) < s5 || (0 < s5 && (nflags & 0x800) != 0)) {
            c.set_tv(m, s5 + 3);
            if c.next(m) == 0 || (nflags & 0x800) != 0 {
                c.set_tdur(m, 0xc);
            }
        }
        if 0x2c < c.next(m) && ((cfeat & 0x80) == 0 || t != ph + 0x120) {
            c.set_tdur(m, 0);
        }
        if t == ph + 0x144 {
            if matches!(cur, 0x25 | 0x27 | 0x29 | 0x2b) && (nfeat & 2) != 0 && (nfeat & 0x20) == 0 {
                c.set_tv(m, 0x34);
                c.set_tdur(m, 6);
            }
            if (cfeat & 1) != 0 && c.next(m) == 0 {
                c.set_tv(m, 0x34);
                c.set_tdur(m, 0x14);
            }
        }
        if t != ph + 0x240 {
            break 'body;
        }
        c.set_tdur(m, 4);
        if c.next(m) == 0 {
            c.set_tv(m, m.s(t + 0x1a));
        }
        if cur == 0 {
            c.set_tv(m, m.s(c.track(m) + 0x16));
        }
        if (nfeat & 0x2000) != 0 || (cfeat & 0x2000) != 0 {
            c.set_tdur(m, 0);
        }
        if (cfeat & 2) == 0 || (cfeat & 0x80) != 0 || c.next(m) != 0 {
            break 'body;
        }
        c.set_tv(m, 0xf);
        to_end = true;
    }
    if to_end {
        c.set_tdur(m, 0x14);
    }
    if 0x14 < c.tdur(m) {
        c.set_tdur(m, 0x14);
    }
    if c.dur(m) < c.tdur(m) {
        c.set_tdur(m, c.dur(m));
    }
    m.w16(t + 0x10, c.dur(m) - c.tdur(m));
    if c.tv(m) < 0 {
        c.set_tv(m, 0);
    }
}

/// FUN_1c01b400: stop-closure silences and aspiration shaping.
fn closures(m: &mut Mem, c: &Ctx, pfeat_lo: i32, cfeat: i32, nfeat_lo: i32, ppflags: i32, cflags: i32) {
    let ph = c.ph;
    let aux = c.aux;
    let cur = c.cur(m);
    if (cfeat & 0x800) != 0 {
        let mut v = to_frames(tw(m, DUR_BURST, cur));
        if (nfeat_lo & 0xc0) == 0 || feat2(m, cur) != feat2(m, c.next(m)) {
            if 1 < v && (((cfeat & 0x40) != 0 && (nfeat_lo & 0x20) != 0) || c.dur(m) < 8) {
                v -= 1;
            }
        } else {
            v = 0;
        }
        let mut v = wrap(c.dur(m) - v);
        if (cur == 0x36 || cur == 0x37) && 0xd < v {
            v = 0xd;
        }
        let mut t = ph + 0x168;
        m.w32(aux + 0x10, t);
        while t <= ph + 0x21c {
            m.w16(t + 0x12, v);
            m.w16(t + 0x14, 0);
            t += 0x24;
            m.w32(aux + 0x10, t);
        }
    }
    if (pfeat_lo & 0x40) != 0 && (pfeat_lo & 2) == 0 && (cfeat & 0x10) != 0 {
        m.w16(ph + 0x158, 0x34);
        if tw(m, PLACE_BEFORE, c.next(m)) != 1 {
            m.w16(ph + 0x158, 0x37);
        }
        m.w16(ph + 0x134, 0);
        let mut v = 6;
        if (cflags & 1) == 0 {
            m.w16(ph + 0x158, m.s(ph + 0x158) - 3);
            v = 4;
        }
        if (cfeat & 0x200) != 0 || cur == 0xf {
            m.w16(ph + 0x158, m.s(ph + 0x158) + 3);
        }
        let pp = m.s(at(ph + 0x2fe, c.pos(m), 2));
        if feat(m, pp) == 0x120 {
            if (ppflags & 0x1e0) == 0 {
                v = 2;
            }
        } else if (cfeat & 1) == 0 {
            v += 3;
        }
        let d = c.dur(m);
        if d <= v {
            v = d - 1;
        }
        if (d >> 1) < v && (cfeat & 1) != 0 && (cflags & 1) != 0 {
            v = d >> 1;
        }
        if (cflags & 0x800) != 0 {
            m.w16(ph + 0x158, m.s(ph + 0x158) - 3);
            v = d;
        }
        for o in [0x132, 0x156, 0xc6, 0xea] {
            m.w16(ph + o, v);
        }
        m.w16(ph + 0xc8, m.s(ph + 0xb4) + 0xfa);
        m.w16(ph + 0xec, m.s(ph + 0xd8) + 0x46);
    }
    if (cfeat & 0x800) != 0 && (cfeat & 2) != 0 && (pfeat_lo & 2) != 0 && (nfeat_lo & 2) == 0 && cur != 0x34 {
        let d = c.dur(m);
        m.w16(ph + 0x134, 10);
        m.w16(ph + 0xc6, d);
        m.w16(ph + 0xea, d);
        m.w16(ph + 0x10e, d);
        m.w16(ph + 0x132, d - 2);
        m.w16(ph + 0xc8, 1000);
        m.w16(ph + 0xec, 1000);
        m.w16(ph + 0x110, 0x5dc);
    }
}

struct Context {
    min_dur: i32,
    prev: i32,
    pfeat: i32,
    cfeat: i32,
    nfeat: i32,
    ppflags: i32,
    pflags: i32,
    cflags: i32,
    nflags: i32,
    ptr: u32,
}

/// FUN_1c01c050: gather phoneme context and reset per-phoneme track holds.
fn gather(m: &mut Mem, c: &Ctx) -> Context {
    let ph = c.ph;
    let aux = c.aux;
    let i = c.pos(m);
    let mut x = Context {
        min_dur: 0,
        prev: 0,
        pfeat: 0,
        cfeat: 0,
        nfeat: 0,
        ppflags: 0,
        pflags: 0,
        cflags: 0,
        nflags: 0,
        ptr: 0,
    };
    if i == 0 {
        if m.s(aux + 0xe) == 0 {
            m.w16(aux + 0xe, 1);
            let mut t = ph + 0x24;
            m.w32(aux + 0x10, t);
            while t <= ph + 0x240 {
                let v = start_target(m, c, 0);
                let tr = m.r32(aux + 0x10);
                m.w16(tr + 0x1a, v);
                t = tr + 0x24;
                m.w32(aux + 0x10, t);
            }
        }
    } else {
        if 1 < i {
            x.ppflags = m.r16(at(ph + 0x568, i, 4)) as u16 as i32;
        }
        x.prev = m.s(aux + 0x18);
        x.pflags = m.r16(at(ph + 0x56c, i, 4)) as u16 as i32;
    }
    let i = c.pos(m);
    m.w16(aux + 0x18, phone(m, ph, i));
    x.cflags = m.r16(at(ph + 0x570, i, 4)) as u16 as i32;
    let n = m.s(ph + 0x11ae);
    if i < n - 1 {
        m.w16(aux + 0xa, m.s(at(ph + 0x304, i, 2)));
        x.nflags = m.r16(at(ph + 0x574, i, 4)) as u16 as i32;
    } else {
        m.w16(aux + 0xa, 0);
    }
    x.ptr = ph + 0x1fc2;
    x.pfeat = feat(m, x.prev);
    x.cfeat = feat(m, c.cur(m));
    x.nfeat = feat(m, c.next(m));
    let md = wrap(to_frames(tw(m, DUR_MINIMUM, c.cur(m))));
    x.min_dur = md;
    if (x.cfeat & 0x20) == 0 && c.cur(m) != 0 {
        let d = c.dur(m);
        let k = if d < md * 2 {
            m.w16(ph + 0x11c4, 0x4000);
            m.w16(ph + 0x11c6, d);
            m.w16(ph + 0x11c8, md);
            wrap((d << 14) / md)
        } else {
            0x7fff
        };
        m.w16(ph + 0x2830, k);
        m.w16(ph + 0x2832, (k >> 1) + 0x2000);
        m.w16(ph + 0x2834, (k >> 1) + 0x19c0);
    }
    for o in [0x132, 0x156, 0x36, 0xc6, 0xea, 0x10e, 0x17a, 0x19e, 0x1c2, 0x1e6, 0x20a, 0x22e, 0x252] {
        m.w16(ph + o, 0);
    }
    x
}

/// FUN_1c019e60
pub fn phone_targets(m: &mut Mem, ctx: u32) {
    let ph = ph_of(m, ctx);
    let aux = m.r32(ph + 0x2848);
    let c = Ctx { ctx, ph, aux };
    let _ = c.ctx;
    let mut x = gather(m, &c);
    if c.cur(m) == 0 {
        m.w16(ph + 0x2b6, 0);
    }
    if (x.cflags & 0x100) != 0 {
        m.w16(ph + 0x2b6, 1);
    }
    m.w32(aux + 0x10, ph + 0x24);
    loop {
        let t = c.track(m);
        let trk = ((t - ph) as i32 - 0x24) / 0x24;
        m.w8(aux + 0x14, m.r8(at(TRACK_TYPE, trk, 1)));
        m.w16(t + 0x18, m.s(t + 0x1a));
        let v = start_target(m, &c, c.pos(m) + 1);
        m.w16(c.track(m) + 0x16, v);
        let v = raw_target(m, &c, c.pos(m));
        m.w16(c.track(m), v);
        m.w16(t + 6, 0);
        let v = m.s(t);
        if v < -1 {
            let k = m.s(ph + 0x2830);
            expand_breakpoints(m, &c, -v, x.min_dur, k, &mut x.ptr);
        } else {
            m.w16(t + 4, 0);
            m.w16(t + 2, m.s(ph + 0x11ac));
            if c.ty(m) == 3 {
                m.w16(aux + 0xc, 0x666);
                if (x.cflags & 3) == 0 {
                    m.w16(aux + 0xc, 0x999);
                    if t == ph + 0x48 {
                        m.w16(aux + 0xc, 0x1000);
                    }
                }
                let d = wrap(((m.s(t + 0x18) + m.s(t + 0x16)) >> 1) - m.s(t));
                m.w16(ph + 0x11c4, d);
                let g = m.s(aux + 0xc);
                m.w16(ph + 0x11c6, g);
                m.w16(t, m.s(t) + ((g * d) >> 14));
            }
            m.w16(t + 0x1a, m.s(t));
        }
        if c.ty(m) == 3 {
            m.w16(ph + 0x11c6, 0x666);
            let d = wrap(m.s(t + 0x1a) - m.s(t + 0x16));
            m.w16(ph + 0x11c4, d);
            m.w16(t + 0x16, m.s(t + 0x16) + ((d * 0x666) >> 14));
        }

        m.w16(aux + 6, 5);
        m.w16(aux, (m.s(t + 0x18) + m.s(t)) >> 1);
        let s = m.s(ph + 0x2832);
        onset_transition(m, &c, s, x.prev, x.pfeat, x.cfeat, x.pflags);
        m.w16(t + 8, 0);
        let td = m.s(aux + 6);
        if 0 < td {
            m.w16(t + 8, wrap((m.s(aux) - m.s(t)) * 8));
            if m.s(t + 8) != 0 {
                m.w16(ph + 0x11c4, m.s(t + 8));
                let g = tw(m, RAMP_GAIN, td);
                m.w16(ph + 0x11c6, g);
                m.w16(t + 0xa, (g * m.s(t + 8)) >> 14);
                m.w16(t + 8, m.s(t + 0xa) * td);
            }
        }
        if t == ph + 0x48 {
            m.w16(ph + 0x2ac, 0);
            m.w16(ph + 0x2b2, 0);
            if m.s(ph + 0x11ac) < m.s(aux + 8) {
                m.w16(aux + 8, m.s(ph + 0x11ac));
            }
            let lt = m.s(aux + 8);
            let ld = m.s(aux + 4);
            if 0 < lt && ld != 0 {
                m.w16(ph + 0x11c4, ld << 3);
                let g = tw(m, RAMP_GAIN, lt);
                m.w16(ph + 0x11c6, g);
                let v = wrap((g * m.s(ph + 0x11c4)) >> 14);
                m.w16(ph + 0x2b2, v);
                m.w16(ph + 0x2ac, v * lt);
            }
            m.w16(aux + 8, 0);
            m.w16(aux + 4, 0);
        }

        m.w16(aux + 6, 4);
        m.w16(aux, (m.s(t + 0x1a) + m.s(t + 0x16)) >> 1);
        let s = m.s(ph + 0x2834);
        offset_transition(m, &c, s, x.cfeat, x.nfeat, x.nflags);
        m.w16(t + 0xc, 0);
        m.w16(t + 0xe, 0);
        let td = m.s(aux + 6);
        if 0 < td {
            let d = wrap((m.s(aux) - m.s(t + 0x1a)) * 8);
            if d != 0 {
                m.w16(ph + 0x11c4, d);
                let g = tw(m, RAMP_GAIN, td);
                m.w16(ph + 0x11c6, g);
                m.w16(t + 0xe, (g * d) >> 14);
            }
        }
        if t == ph + 0x48 {
            let d = m.s(ph + 0x11ac);
            m.w16(ph + 0x2ae, 0);
            m.w16(ph + 0x2b4, 0);
            m.w16(ph + 0x2b0, d);
            if d < m.s(aux + 8) {
                m.w16(aux + 8, d);
            }
            let lt = m.s(aux + 8);
            let ld = m.s(aux + 4);
            if 0 < lt && ld != 0 {
                m.w16(ph + 0x2b0, m.s(ph + 0x11ac) - lt);
                let s8 = wrap(ld << 3);
                m.w16(ph + 0x11c4, s8);
                let g = tw(m, RAMP_GAIN, lt);
                m.w16(ph + 0x11c6, g);
                m.w16(ph + 0x2b4, (g * s8) >> 14);
            }
            m.w16(aux + 8, 0);
            m.w16(aux + 4, 0);
        }
        let next_t = t + 0x24;
        m.w32(aux + 0x10, next_t);
        if next_t > ph + 0x240 {
            break;
        }
    }
    closures(m, &c, x.pfeat & 0xff, x.cfeat, x.nfeat & 0xff, x.ppflags, x.cflags);
}
