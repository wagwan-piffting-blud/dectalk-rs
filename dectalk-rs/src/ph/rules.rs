//! Clause symbol rules (FUN_1c01c460 and helpers): stress and boundary normalization over the
//! raw clause buffer (ph+0x11ca), then compaction into the phoneme array with feature flags.
//!
//! Clause arrays: sym = ph+0x11ca (u16), arg1 = *(ph+0x1428), arg2 = *(ph+0x142c),
//! arg3 = *(ph+0x1430), out phonemes = *(ph+0x1434), out flags = *(ph+0x1438) (u32).

use super::mem::Mem;
use super::{index_adjust, kernel_of, ph_of};

const FEATURES: u32 = 0x1c06_73fc;
const BOUNDARY_FLAGS: u32 = 0x1c06_9a34;

const S_STRESS2: i32 = 0x66;
const S_STRESS1: i32 = 0x67;
const S_EMPH: i32 = 0x68;
const S_WORD: i32 = 0x6e;

#[inline]
fn at(base: u32, i: i32, scale: i32) -> u32 {
    base.wrapping_add((i * scale) as u32)
}

#[inline]
pub(crate) fn feat(m: &Mem, v: i32) -> u8 {
    m.r8(at(FEATURES, v, 2))
}

#[inline]
fn sym(m: &Mem, ph: u32, i: i32) -> i32 {
    m.s(at(ph + 0x11ca, i, 2))
}

#[inline]
fn set_sym(m: &mut Mem, ph: u32, i: i32, v: i32) {
    m.w16(at(ph + 0x11ca, i, 2), v);
}

#[inline]
fn arr(m: &Mem, ph: u32, slot: u32, i: i32) -> u32 {
    at(m.r32(ph + slot), i, 2)
}

#[inline]
fn count(m: &Mem, ph: u32) -> i32 {
    m.s(ph + 0x1426)
}

/// FUN_1c01d340
#[inline]
fn is_boundary(v: i32) -> bool {
    0x6e < v && v < 0x77
}

/// FUN_1c01d4c0
fn delete_sym(m: &mut Mem, ctx: u32, p: i32) {
    let kernel = kernel_of(m, ctx);
    let ph = ph_of(m, ctx);
    m.w16(ph + 0x1426, count(m, ph) - 1);
    let aux = m.r32(ph + 0x2848);
    m.w32(aux + 0x1c, 1);
    let mut k = p;
    while k < count(m, ph) {
        set_sym(m, ph, k, sym(m, ph, k + 1));
        for slot in [0x1428, 0x142c] {
            let v = m.s(arr(m, ph, slot, k + 1));
            m.w16(arr(m, ph, slot, k), v);
        }
        k += 1;
    }
    index_adjust(m, kernel, p + 1, -1, 1);
}

/// FUN_1c01d360
fn insert_sym(m: &mut Mem, ctx: u32, p: i32, v: i32) {
    let kernel = kernel_of(m, ctx);
    let ph = ph_of(m, ctx);
    let n = count(m, ph);
    if n >= 300 {
        return;
    }
    let mut j = n;
    while j > p {
        set_sym(m, ph, j, sym(m, ph, j - 1));
        for slot in [0x1428, 0x142c] {
            let x = m.s(arr(m, ph, slot, j - 1));
            m.w16(arr(m, ph, slot, j), x);
        }
        j -= 1;
    }
    set_sym(m, ph, p, v);
    m.w16(arr(m, ph, 0x1428, p), 0);
    m.w16(arr(m, ph, 0x142c, p), 0);
    m.w16(ph + 0x1426, n + 1);
    if v != S_STRESS1 {
        index_adjust(m, kernel, p + 1, 1, 0);
    }
}

fn put_stress(m: &mut Mem, ph: u32, j: i32, v: i32, u: i32) {
    set_sym(m, ph, j, v);
    m.w16(arr(m, ph, 0x1428, j), u);
    m.w16(arr(m, ph, 0x142c, j), u);
}

/// FUN_1c01cf70: move the stress mark at `p` onto the following syllable nucleus.
fn move_stress(m: &mut Mem, ctx: u32, p: i32) {
    let ph = ph_of(m, ctx);
    let s1 = sym(m, ph, p);
    let u = m.s(arr(m, ph, 0x1428, p));

    let merge = |m: &mut Mem, find: i32, become_: i32| -> bool {
        let mut j = p + 1;
        while j < count(m, ph) {
            let s3 = sym(m, ph, j);
            if s3 == find {
                put_stress(m, ph, j, become_, u);
                delete_sym(m, ctx, p);
                return true;
            }
            if is_boundary(s3) {
                break;
            }
            j += 1;
        }
        false
    };
    if s1 == S_EMPH {
        if merge(m, S_STRESS1, S_EMPH) || merge(m, S_STRESS2, S_EMPH) {
            return;
        }
    } else if s1 == S_STRESS1 && merge(m, S_STRESS2, S_STRESS1) {
        return;
    }

    let mut j = p + 1;
    if j >= count(m, ph) {
        return;
    }
    loop {
        if is_boundary(sym(m, ph, j)) {
            delete_sym(m, ctx, j - 1);
            return;
        }
        let s3 = sym(m, ph, j);
        if 0x65 < s3 && s3 < 0x69 {
            if sym(m, ph, j) < s1 {
                put_stress(m, ph, j, s1, u);
            }
            delete_sym(m, ctx, j - 1);
            return;
        }
        if (feat(m, s3) & 1) != 0 {
            break;
        }
        set_sym(m, ph, j - 1, s3);
        for slot in [0x1428, 0x142c] {
            let x = m.s(arr(m, ph, slot, j));
            m.w16(arr(m, ph, slot, j - 1), x);
        }
        j += 1;
        if count(m, ph) <= j {
            return;
        }
    }
    put_stress(m, ph, j - 1, s1, u);
}

/// FUN_1c01d460: of two adjacent boundaries keep the stronger (word boundaries are sticky).
fn merge_boundaries(m: &mut Mem, ctx: u32, a: i32, b: i32) {
    let ph = ph_of(m, ctx);
    let (sa, sb) = (sym(m, ph, a), sym(m, ph, b));
    if sa < sb {
        if sa != S_WORD {
            delete_sym(m, ctx, a);
        }
    } else if sb != S_WORD {
        delete_sym(m, ctx, b);
    }
}

/// FUN_1c01ce50: guarantee a primary stress in the phrase ending at *i.
fn ensure_phrase_stress(m: &mut Mem, ctx: u32, i: &mut i32, start: i32) {
    let ph = ph_of(m, ctx);
    let s1 = *i as i16 as i32;
    let mut s4 = s1 - 1;
    let mut s3 = s4;
    while start <= s3 {
        if sym(m, ph, s3) == S_STRESS2 {
            set_sym(m, ph, s3, S_STRESS1);
            return;
        }
        s3 -= 1;
    }
    loop {
        s3 = 0;
        if !(start <= s4) {
            break;
        }
        s3 = s4;
        if !(sym(m, ph, s4) < 0x6f) {
            break;
        }
        s4 -= 1;
    }
    if s3 < s1 {
        while (feat(m, sym(m, ph, s3)) & 1) == 0 {
            s3 += 1;
            if s1 <= s3 {
                return;
            }
        }
        insert_sym(m, ctx, s3, S_STRESS1);
        *i += 1;
    }
}

/// FUN_1c01d420
fn promote_last_stress(m: &mut Mem, ph: u32, p: i32) {
    let mut s = p - 1;
    if 0 < s {
        while sym(m, ph, s) != S_STRESS1 {
            s -= 1;
            if s < 1 {
                return;
            }
        }
        set_sym(m, ph, s, S_EMPH);
    }
}

/// FUN_1c01d5d0
fn set_flag(m: &mut Mem, ph: u32, bits: i32, p: i32) {
    let p = p as i16 as i32;
    if -1 < p && p < 300 && 0 < bits && bits < 0x4000_0001 {
        let a = at(m.r32(ph + 0x1438), p, 4);
        m.w32(a, m.r32(a) | bits as u32);
    }
}

/// FUN_1c01d550: append a phoneme to the output array.
fn emit(m: &mut Mem, ph: u32, v: i32, p: i32, a: i32, b: i32) {
    let c = m.s(ph + 0x143c);
    if c <= p {
        m.w16(arr(m, ph, 0x1434, c), v);
        m.w16(arr(m, ph, 0x1428, c), a);
        if m.s(ph + 0x1440) != 3 {
            m.w16(arr(m, ph, 0x142c, c), b);
        }
        if c < 300 {
            m.w16(ph + 0x143c, c + 1);
        }
    }
}

/// FUN_1c01d200: route pitch arguments attached to stress symbols into the pitch-command arrays.
fn pitch_args(m: &mut Mem, ph: u32, a: &mut i32, b: &mut i32, v: i32, cnt: &mut i32) {
    let mode = m.s(ph + 0x1440);
    if v == 0x67 || v == 0x68 || v == 0x69 || (v == 0x6a && mode != 5 && mode != 4) {
        if *b != 0 || mode == 3 {
            if *b < 0 {
                *b = -*b;
            }
            if 199 < *b {
                *b = 199;
            }
            *b += match v {
                0x69 => 200,
                0x6a => 400,
                _ => 1000,
            };
            *b = *b as i16 as i32;
            m.w16(arr(m, ph, 0x142c, *cnt), *b);
            m.w16(arr(m, ph, 0x1430, *cnt), *a);
            *a = 0;
            *b = 0;
            m.w16(ph + 0x1440, 3);
        }
        *cnt = (*cnt + 1) as i16 as i32;
    } else if *b != 0 {
        if mode == 3 {
            *a = 0;
            *b = 0;
            return;
        }
        if mode != 5 && *b < 0x26 {
            m.w16(ph + 0x1440, 4);
            return;
        }
        if mode != 4 {
            m.w16(ph + 0x1440, 5);
            return;
        }
        *a = 0;
        *b = 0;
    }
}

/// FUN_1c01c2e0: onset-cluster class of two consonants (0 = not a legal onset).
fn onset_cluster(a: i32, b: i32) -> i32 {
    match a - 0x25 {
        0 => match b {
            0x1a => 2,
            0x1b => 1,
            _ => 0,
        },
        2 | 0xb => (b == 0x1a || b == 0x18) as i32,
        4 => matches!(b, 0x18 | 0x1b | 0x2d | 0x2f | 0x31 | 0x1f | 0x20 | 0x25) as i32,
        6 => matches!(b, 0x18 | 0x1b | 0x2d | 0x2f | 0x1a | 0x1f | 0x20) as i32,
        8 => if b == 0x1b || b == 0x1a { 2 } else { 0 },
        9 => (b == 0x1b || b == 0x1a) as i32,
        10 => match b {
            0x1a => 2,
            0x18 => 1,
            _ => 0,
        },
        0xc => if matches!(b, 0x1a | 0x1b | 0x18) { 2 } else { 0 },
        0xd => matches!(b, 0x1a | 0x1b | 0x18) as i32,
        _ => 0,
    }
}

/// FUN_1c01cc70: consonant before a stressed nucleus -> syllable-initial flag if a legal onset.
fn flag_onset(m: &mut Mem, ph: u32, p: i32) {
    let mut j = p + 1;
    if j >= count(m, ph) {
        return;
    }
    let s1;
    loop {
        let v = sym(m, ph, j);
        if v == S_STRESS1 || v == S_STRESS2 || v == S_EMPH {
            s1 = v;
            break;
        }
        if (feat(m, v) & 1) != 0 || (0x6b < v && v < 0x77) {
            return;
        }
        j += 1;
        if count(m, ph) <= j {
            return;
        }
    }
    let d = j - p;
    if d < 4 {
        if d != 1 {
            let u = onset_cluster(sym(m, ph, j - 2), sym(m, ph, j - 1));
            if u == 0 {
                return;
            }
            if d == 3 && (u != 2 || sym(m, ph, j - 3) != 0x29) {
                return;
            }
        }
        let last = m.s(ph + 0x143c) - 1;
        match s1 {
            S_STRESS1 => set_flag(m, ph, 1, last),
            S_STRESS2 => set_flag(m, ph, 2, last),
            _ => set_flag(m, ph, 3, last),
        }
    }
}

/// FUN_1c01cf00: flag the phoneme with the class of the boundary that follows its syllable.
fn flag_next_boundary(m: &mut Mem, ph: u32, p: i32) {
    let mut j = p + 1;
    if j >= count(m, ph) {
        return;
    }
    loop {
        let v = sym(m, ph, j);
        if !(v < 0x6c || 0x76 < v) {
            break;
        }
        if (feat(m, v) & 1) != 0 {
            return;
        }
        j += 1;
        if count(m, ph) <= j {
            return;
        }
    }
    let bits = m.s(at(BOUNDARY_FLAGS, sym(m, ph, j), 2));
    let last = m.s(ph + 0x143c) - 1;
    set_flag(m, ph, bits, last);
}

/// FUN_1c01cd90: syllable position of a vowel within its phrase.
fn flag_vowel_position(m: &mut Mem, ph: u32, p: i32) {
    let mut pos = 0;
    let mut k = m.s(ph + 0x143c) - 2;
    while 0 < k {
        if 0x5f < (m.r32(at(m.r32(ph + 0x1438), k, 4)) & 0x1e0) {
            break;
        }
        if (feat(m, m.s(arr(m, ph, 0x1434, k))) & 1) != 0 {
            pos = 0x18;
        }
        k -= 1;
    }
    let mut j = p + 1;
    if j >= count(m, ph) {
        return;
    }
    loop {
        let v = sym(m, ph, j);
        if !(v < 0x6f || 0x76 < v) {
            break;
        }
        if (feat(m, v) & 1) != 0 {
            if pos == 0x18 {
                pos = 0x10;
            } else if pos == 0 {
                pos = 8;
            }
        }
        j += 1;
        if count(m, ph) <= j {
            return;
        }
    }
    if pos != 0 {
        let last = m.s(ph + 0x143c) - 1;
        set_flag(m, ph, pos, last);
    }
}

/// FUN_1c01c460: returns 0 if the clause was aborted.
pub fn clause_rules(m: &mut Mem, ctx: u32) -> i32 {
    let kernel = kernel_of(m, ctx);
    let ph = ph_of(m, ctx);
    let aux = m.r32(ph + 0x2848);
    m.w16(ph + 0x1c82, 1);
    m.w32(aux + 0x1c, 0);
    let mut word_seen = false;
    let mut i: i32 = 0;
    while i < count(m, ph) {
        m.w32(at(m.r32(ph + 0x1438), i, 4), 0);
        i += 1;
    }
    m.w16(ph + 0x1440, 1);
    m.w16(ph + 0x1442, 0);
    m.w16(ph + 0x143c, 0);

    let mut i: i32 = 0;
    let mut stresses: i32 = 0;
    let mut phrase_start: i32 = 0;
    if 0 < count(m, ph) {
        loop {
            if m.r32(aux + 0x1c) != 0 {
                i -= 1;
                m.w32(aux + 0x1c, 0);
            }
            let v = sym(m, ph, i);
            if v == S_WORD {
                word_seen = true;
            }
            if v == S_STRESS1 && word_seen {
                set_sym(m, ph, i, S_STRESS2);
                word_seen = false;
            }
            if sym(m, ph, i) == 0x78 {
                delete_sym(m, ctx, i);
            }
            if m.i(kernel + 0x174) != 0 {
                return 0;
            }
            let v = sym(m, ph, i);
            if 0x68 < v && v < 0x6c && m.s(ph + 0x1440) == 1 {
                m.w16(ph + 0x1440, 2);
            }

            if sym(m, ph, i) == 0x70 && i + 1 < count(m, ph) {
                let mut j = i + 1;
                loop {
                    if is_boundary(sym(m, ph, j)) {
                        let v2 = sym(m, ph, j);
                        if 0x72 < v2 || (v2 == 0x70 && sym(m, ph, j + 1) != 0x18) {
                            set_sym(m, ph, i, 0x6f);
                            if sym(m, ph, j) == 0x70 {
                                set_sym(m, ph, j, 0x71);
                            }
                            if sym(m, ph, i + 1) == 0x25 && sym(m, ph, i + 2) == 0xf {
                                set_sym(m, ph, i + 2, 0x16);
                            }
                            if sym(m, ph, j - 2) == 0x2f && sym(m, ph, j - 1) == 0xd {
                                set_sym(m, ph, i + 2, 0xe);
                            }
                            if sym(m, ph, i + 1) == S_STRESS2 {
                                set_sym(m, ph, i + 1, S_STRESS1);
                            } else {
                                insert_sym(m, ctx, i + 1, S_STRESS1);
                                move_stress(m, ctx, i + 1);
                            }
                        }
                        break;
                    }
                    j += 1;
                    if !(j < count(m, ph)) {
                        break;
                    }
                }
            }

            let v = sym(m, ph, i);
            if 0x65 < v && v < 0x69 {
                if v != S_STRESS2 {
                    stresses += 1;
                }
                let mut j = i + 1;
                let mut v2 = sym(m, ph, j);
                let mut dropped = false;
                if 0x38 < v2 {
                    loop {
                        if count(m, ph) <= j {
                            break;
                        }
                        if 0x6f < v2 && v2 < 0x77 && v2 != S_WORD {
                            stresses -= 1;
                            delete_sym(m, ctx, i);
                            dropped = true;
                            break;
                        }
                        j += 1;
                        v2 = sym(m, ph, j);
                        if !(0x38 < v2) {
                            break;
                        }
                    }
                }
                if !dropped && (feat(m, sym(m, ph, j)) & 1) == 0 {
                    move_stress(m, ctx, i);
                }
            }

            let v = sym(m, ph, i);
            if 0x6b < v && v < 0x77 {
                let j = i + 1;
                if j < count(m, ph) {
                    let v2 = sym(m, ph, j);
                    if 0x6b < v2 && v2 < 0x77 {
                        merge_boundaries(m, ctx, i, j);
                    }
                }
            }
            let rate = m.s(kernel + 0x222);
            if rate < 0x79 {
                let v = sym(m, ph, i);
                if v == 0x71 || v == 0x70 {
                    set_sym(m, ph, i, 0x73);
                }
            } else if rate < 0x8d && sym(m, ph, i) == 0x70 {
                set_sym(m, ph, i, 0x71);
            }
            let v = sym(m, ph, i);
            if 0x72 < v && v < 0x77 && 0 < i && stresses as i16 == 0 {
                ensure_phrase_stress(m, ctx, &mut i, phrase_start as i16 as i32);
            }
            let v = sym(m, ph, i);
            if (0x72..0x77).contains(&v) {
                stresses = 0;
                phrase_start = i;
                if v == 0x76 {
                    promote_last_stress(m, ph, i);
                }
            }
            if sym(m, ph, i) == 0x75 {
                m.w16(ph + 0x1442, 1);
            }
            i += 1;
            if !(i < count(m, ph)) {
                break;
            }
        }
    }

    let mut pitch_cnt: i32 = 0;
    m.w16(ph + 0x143c, 0);
    m.w8(ph + 0x143e, 0);
    let mut after_boundary = false;
    let mut vowel_seen: i32 = 0;
    let mut i: i32 = 0;
    if 0 < count(m, ph) {
        loop {
            let out_before = m.s(ph + 0x143c);
            if m.i(kernel + 0x174) != 0 {
                return 0;
            }
            let mut a = m.s(arr(m, ph, 0x1428, i));
            let v = sym(m, ph, i);
            m.w16(arr(m, ph, 0x1428, i), 0);
            let mut b = m.s(arr(m, ph, 0x142c, i));
            m.w16(arr(m, ph, 0x142c, i), 0);
            pitch_args(m, ph, &mut a, &mut b, v, &mut pitch_cnt);

            let f = m.r32(at(ph + 0xcb4, i, 4));
            let cur = m.s(ph + 0x143c);
            if (f & 0x400) == 0 {
                if (f & 1) != 0 {
                    set_flag(m, ph, 0x100000, cur);
                } else if (f & 0x20000) != 0 || (f & 0x80) != 0 {
                    m.w16(ph + 0x1c84, m.s(ph + 0x1c84) + 1);
                    set_flag(m, ph, 0x200000, cur);
                }
            } else {
                set_flag(m, ph, 0x80000, cur);
            }
            if (m.r32(at(ph + 0xcb4, i, 4)) & 0x800000) != 0 {
                let cur = m.s(ph + 0x143c);
                set_flag(m, ph, 0x400000, cur);
            }

            if v < 0x39 {
                emit(m, ph, v, i, a, b);
                if (feat(m, v) & 1) == 0 {
                    flag_onset(m, ph, i);
                    if after_boundary {
                        let last = m.s(ph + 0x143c) - 1;
                        set_flag(m, ph, 4, last);
                    }
                    if vowel_seen as i16 == 1 {
                        flag_next_boundary(m, ph, i);
                    }
                } else {
                    vowel_seen = 1;
                    after_boundary = false;
                    flag_vowel_position(m, ph, i);
                    flag_next_boundary(m, ph, i);
                }
            } else {
                let cur = m.s(ph + 0x143c);
                match v {
                    100 => set_flag(m, ph, 0x2000, cur),
                    0x66 => set_flag(m, ph, 2, cur),
                    0x67 => set_flag(m, ph, 1, cur),
                    0x68 => set_flag(m, ph, 3, cur),
                    0x69 => set_flag(m, ph, 0x200, cur),
                    0x6a => set_flag(m, ph, 0x400, cur),
                    0x6f..=0x72 => {
                        if v == 0x6f {
                            m.w16(ph + 0x1c82, m.s(ph + 0x1c82) + 1);
                        }
                        if sym(m, ph, i + 1) != S_WORD {
                            after_boundary = true;
                        }
                    }
                    0x73..=0x76 => {
                        if v == 0x73 {
                            m.w16(ph + 0x1196, m.s(ph + 0x1196) + 1);
                            m.w16(ph + 0x2fc, 1);
                        } else {
                            m.w16(ph + 0x2fc, match v {
                                0x74 => 0,
                                0x75 => 3,
                                _ => 2,
                            });
                            m.w16(ph + 0x1196, 0);
                        }
                        emit(m, ph, 0, i, a, b);
                        after_boundary = true;
                    }
                    0x77 => m.w8(ph + 0x143e, 1),
                    _ => {}
                }
            }
            if m.s(ph + 0x143c) == out_before {
                index_adjust(m, kernel, i + 1, -1, 0);
            }
            i += 1;
            if !(i < count(m, ph)) {
                break;
            }
        }
    }
    1
}
