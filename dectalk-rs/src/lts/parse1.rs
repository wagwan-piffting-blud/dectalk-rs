use super::*;

// Clause-word node (0x24 bytes): +0 next, +4 prev, +0xc flags, +0x10 phrase class, +0x14 form
// class, +0x18 alternate form class. lts+0x408 holds up to 10 phrase-head nodes, lts+0x430 their
// count (i16), +0x432 first classed head, +0x434 split index.

#[inline]
fn nx(m: &Mem, n: u32) -> u32 {
    m.r32(n)
}

#[inline]
fn pv(m: &Mem, n: u32) -> u32 {
    m.r32(n.wrapping_add(4))
}

#[inline]
fn tg(m: &Mem, n: u32) -> u32 {
    m.r32(n.wrapping_add(0x14))
}

#[inline]
fn low4(m: &Mem, n: u32) -> bool {
    m.r8(n.wrapping_add(0xc)) & 0xf != 0
}

#[inline]
fn head_bit(m: &Mem, n: u32) -> bool {
    m.r8(n.wrapping_add(0xc)) & 0x10 != 0
}

#[inline]
fn or_flags(m: &mut Mem, n: u32, bits: u32) {
    let a = n.wrapping_add(0xc);
    let v = m.r32(a) | bits;
    m.w32(a, v);
}

/// Address of phrase-head slot `i` (lts+0x408 + i*4).
#[inline]
fn slot(lts: u32, i: i32) -> u32 {
    lts.wrapping_add(0x408).wrapping_add((i as u32).wrapping_mul(4))
}

#[inline]
fn split(m: &Mem, lts: u32) -> i32 {
    m.s(lts + 0x434)
}

#[inline]
fn count(m: &Mem, lts: u32) -> i32 {
    m.s(lts + 0x430)
}

/// Pairing strength between a preceding form class `p1` and the following form class `p2` (0/1/2).
pub fn f_1c00a1a0(_m: &mut Mem, p1: u32, p2: u32) -> u32 {
    match p1 {
        0x25 => match p2 {
            0x1a => 2,
            0x1b => 1,
            _ => 0,
        },
        0x27 | 0x30 => (p2 == 0x18 || p2 == 0x1a) as u32,
        0x29 => matches!(p2, 0x18 | 0x1b | 0x2d | 0x2f | 0x31 | 0x1f | 0x20 | 0x25) as u32,
        0x2b => matches!(p2, 0x18 | 0x1b | 0x1a | 0x2d | 0x2f | 0x1f | 0x20) as u32,
        0x2d => {
            if p2 == 0x1b || p2 == 0x1a {
                2
            } else {
                0
            }
        }
        0x2e => (p2 == 0x1b || p2 == 0x1a) as u32,
        0x2f => match p2 {
            0x1a => 2,
            0x18 => 1,
            _ => 0,
        },
        0x31 => (p2 == 0x18 || p2 == 0x1b || p2 == 0x1a) as u32 * 2,
        0x32 => (p2 == 0x18 || p2 == 0x1b || p2 == 0x1a) as u32,
        _ => 0,
    }
}

/// Homograph / form-class disambiguation pass over the clause word list at lts+0x8c8.
pub fn f_1c00a2f0(m: &mut Mem, p1: u32) -> u32 {
    let lts = p1;
    let head = lts + 0x8c8;

    let mut class = 0x79u32;
    let mut head_flags = 0u32;
    let mut n = nx(m, head);
    while n != head {
        let fl = m.r32(n + 0xc);
        if fl & 0x10 != 0 {
            class = m.r32(n + 0x10);
            head_flags = fl;
        }
        if head_flags & 0x80 == 0 {
            let alt = m.r32(n + 0x18);
            if alt != 0 && class == 0x7a {
                m.w32(n + 0x14, alt);
                m.w32(n + 0x18, 0);
            }
        }
        n = nx(m, n);
    }

    m.w32(lts + 0x8dc, 0);
    m.w32(lts + 0x8e0, 0);
    m.w32(lts + 0x8d4, 0xf);

    let mut cur = 0x79u32;
    let mut prev_class = 0x79u32;
    let mut node = nx(m, head);
    while node != head {
        let tag = tg(m, node);
        if head_bit(m, node) {
            prev_class = cur;
            cur = m.r32(node + 0x10);
        }
        match a2f0_step(m, lts, node, tag, cur, prev_class) {
            Some(n) => node = n,
            None => return 0,
        }
    }
    0
}

/// One node of FUN_1c00a2f0's second pass; returns the next node to visit, or None to return.
fn a2f0_step(m: &mut Mem, lts: u32, node: u32, tag: u32, cur: u32, prev_class: u32) -> Option<u32> {
    if cur == 0x7a {
        if tag == 0x11 || tag == 0x12 {
            let a = nx(m, node);
            if tg(m, a) == 0x1b && !head_bit(m, a) && low4(m, nx(m, a)) {
                f_1c00b410(m, lts, a);
                m.w32(node + 0x14, 0x22);
                return Some(nx(m, node));
            }
        }
    } else if (tag == 0x1b || tag == 0x1a)
        && f_1c00b390(m, pv(m, node)) != 0
        && low4(m, nx(m, node))
    {
        m.w32(node + 0x14, 0xf);
        if tag == 0x1b {
            m.w32(node + 0x14, 0x22);
        }
        return Some(nx(m, node));
    }

    if tag == 0x11 {
        let a = nx(m, node);
        if tg(m, a) != 0x1a || head_bit(m, a) {
            return Some(nx(m, node));
        }
        f_1c00b410(m, lts, a);
        let a = nx(m, node);
        if tg(m, a) == 0x1a {
            if head_bit(m, a) {
                return Some(nx(m, node));
            }
            f_1c00b410(m, lts, a);
        }
        m.w32(node + 0x14, 0xf);
        return Some(nx(m, node));
    }

    if tag == 0x20 {
        let e = nx(m, node);
        if tg(m, e) == 0xe && tg(m, nx(m, e)) == 0x22 && low4(m, nx(m, nx(m, e))) {
            if f_1c00b3b0(m, lts, e, 0x19, 0, 0x79) == 0 {
                return None;
            }
            return Some(e);
        }
        let t = tg(m, e);
        if (t == 0x31 || t == 0x32) && !head_bit(m, e) {
            m.w32(node + 0x14, 0x21);
        }
        return Some(nx(m, node));
    }

    let mut tag = tag;
    if tag == 0x32 || tag == 0x31 {
        // The replacement class also steers the checks below (0x31 continues as 0x29).
        tag = if tag == 0x32 { 0x37 } else { 0x29 };
        let mut a = nx(m, node);
        let mut t = tg(m, a);
        if t == 7 {
            a = nx(m, a);
            t = tg(m, a);
            if t == 0x2a && low4(m, nx(m, a)) {
                m.w32(node + 0x14, tag);
                return Some(nx(m, node));
            }
        }
        if t == 0x12 {
            a = nx(m, a);
            let t2 = tg(m, a);
            if t2 == 0x30 && low4(m, nx(m, a)) {
                m.w32(node + 0x14, tag);
                return Some(nx(m, node));
            }
            if t2 == 0x2a {
                let b = nx(m, a);
                if tg(m, b) == 0x11 {
                    let c = nx(m, b);
                    if tg(m, c) == 0x1f && low4(m, nx(m, c)) {
                        m.w32(node + 0x14, tag);
                        return Some(nx(m, node));
                    }
                }
            }
            if t2 == 0x29 {
                let b = nx(m, a);
                if tg(m, b) == 0x2f && low4(m, nx(m, b)) {
                    m.w32(node + 0x14, tag);
                    return Some(nx(m, node));
                }
            }
        }
    }

    if tag == 0x30 {
        let a = nx(m, node);
        if tg(m, a) == 0xe {
            let b = nx(m, a);
            let t = tg(m, b);
            if (t == 0x1b || t == 0x22) && low4(m, nx(m, b)) {
                m.w32(node + 0x14, 0x37);
            }
        }
        return Some(nx(m, node));
    }

    if tag == 0x29 {
        let p = pv(m, node);
        if tg(m, p) == 0x31
            && tg(m, pv(m, p)) == 0x12
            && prev_class == 0x7a
            && f_1c00a8f0(m, nx(m, node)) != 0
            && cur != 0x7a
        {
            m.w32(p + 0x14, 0x32);
            m.w32(node + 0x14, 0x2a);
            // Revisits the same node with its new class.
            return Some(node);
        }
        let p = pv(m, node);
        let a = nx(m, node);
        if tg(m, p) == 0x29 && tg(m, a) == 0xe && low4(m, nx(m, a)) {
            return a2f0_absorb(m, lts, node, p);
        }
        let a = nx(m, node);
        if tg(m, a) == 1 {
            let b = nx(m, a);
            if tg(m, b) == 0x11 {
                let c = nx(m, b);
                if tg(m, c) == 0x29 && low4(m, nx(m, c)) {
                    return a2f0_absorb(m, lts, node, a);
                }
            }
        }
        return Some(nx(m, node));
    }

    if tag == 0x2f {
        let a = nx(m, node);
        let t = tg(m, a);
        if t == 0xe {
            let b = nx(m, a);
            if low4(m, b) {
                m.w32(node + 0x14, 0x36);
            } else {
                let t2 = tg(m, b);
                if t2 == 0x22 && low4(m, nx(m, b)) {
                    m.w32(node + 0x14, 0x36);
                } else if t2 == 3 {
                    let c = nx(m, b);
                    if tg(m, c) == 0x1a {
                        let d = nx(m, c);
                        if tg(m, d) == 1 && low4(m, nx(m, d)) {
                            m.w32(node + 0x14, 0x36);
                        }
                    }
                }
            }
        } else if t == 1 {
            let c = nx(m, a);
            let t2 = tg(m, c);
            if t2 == 0x11 && low4(m, nx(m, c)) {
                return a2f0_absorb(m, lts, node, a);
            }
            if t2 == 3 {
                let d = nx(m, c);
                if tg(m, d) == 0x2f {
                    let e = nx(m, d);
                    if tg(m, e) == 0xf && low4(m, nx(m, e)) {
                        m.w32(node + 0x14, 0x2b);
                    }
                }
            }
        }
    }
    Some(nx(m, node))
}

/// FUN_1c00a2f0 label 0x1c00a68b: drop `victim` from the list and reclass `node` as 0x2b.
fn a2f0_absorb(m: &mut Mem, lts: u32, node: u32, victim: u32) -> Option<u32> {
    if head_bit(m, victim) {
        return None;
    }
    f_1c00b410(m, lts, victim);
    m.w32(node + 0x14, 0x2b);
    Some(nx(m, node))
}

/// Groups words [p1, p2) into phrases, walking backwards from the first phrase head and marking
/// new phrase heads (flag 0x10) with the phrase class they absorb.
pub fn f_1c00a6e0(m: &mut Mem, p1: u32, p2: u32) -> u32 {
    let first = p1;
    let end = p2;
    if first == end {
        return 0;
    }
    let mut last = 0u32;
    let mut n = first;
    loop {
        if head_bit(m, n) {
            last = n;
            break;
        }
        n = nx(m, n);
        if n == end {
            break;
        }
    }
    if n == first {
        return 0;
    }

    let absorb = |m: &mut Mem, x: u32, val: &mut u32| {
        let v = m.r32(x + 0x10);
        if v != 0x79 {
            *val = v;
            m.w32(x + 0x10, 0x79);
        }
    };

    let mut cur = n;
    loop {
        let mut val = 0x79u32;
        let mut s = pv(m, cur);
        let mut at_first = false;
        while f_1c00a8d0(m, s) != 0 {
            cur = s;
            absorb(m, s, &mut val);
            if s == first {
                at_first = true;
                break;
            }
            s = pv(m, s);
        }
        if at_first || cur == first {
            if last != 0 {
                let fl = m.r32(last + 0xc) & 0xffff_ffef;
                m.w32(last + 0xc, fl);
                val = m.r32(last + 0x10);
                m.w32(last + 0x10, 0x79);
            }
            m.w32(cur + 0x10, val);
            or_flags(m, cur, 0x10);
            return 0;
        }

        cur = pv(m, cur);
        let fl = m.r32(cur + 0xc) | 0x40;
        m.w32(cur + 0xc, fl);
        absorb(m, cur, &mut val);
        if fl & 0xf == 0 && cur != first {
            let b = pv(m, cur);
            if f_1c00a8d0(m, b) != 0 {
                let fl = m.r32(b + 0xc) | 0x40;
                m.w32(b + 0xc, fl);
                cur = b;
                absorb(m, b, &mut val);
                if fl & 0xf == 0 && b != first {
                    let c = pv(m, b);
                    if f_1c00a8d0(m, c) != 0 {
                        let (tc, tb) = (tg(m, c), tg(m, b));
                        let r = f_1c00a1a0(m, tc, tb);
                        if r != 0 {
                            let fl = m.r32(c + 0xc) | 0x40;
                            m.w32(c + 0xc, fl);
                            cur = c;
                            absorb(m, c, &mut val);
                            if r == 2 && fl & 0xf == 0 && c != first {
                                let d = pv(m, c);
                                let t = tg(m, d);
                                if t == 0x29 || t == 0x2b {
                                    or_flags(m, d, 0x40);
                                    cur = d;
                                    absorb(m, d, &mut val);
                                }
                            }
                        }
                    }
                }
            }
        }
        or_flags(m, cur, 0x10);
        m.w32(cur + 0x10, val);
        last = cur;
        if cur == first {
            return 0;
        }
    }
}

/// Form-class attribute bit 0 (table at 0x1c032e18, stride 2): word attaches to the following word.
pub fn f_1c00a8d0(m: &mut Mem, p1: u32) -> u32 {
    let t = tg(m, p1);
    (m.r8(0x1c032e18u32.wrapping_add(t.wrapping_mul(2))) & 1) as u32
}

/// Form-class attribute bit 1 (table at 0x1c032e18, stride 2).
pub fn f_1c00a8f0(m: &mut Mem, p1: u32) -> u32 {
    let t = tg(m, p1);
    ((m.r8(0x1c032e18u32.wrapping_add(t.wrapping_mul(2))) >> 1) & 1) as u32
}

/// Chooses the clause split point among the phrase heads of [p2, p3) and assigns class p4 to the
/// head at the split; p5 selects a fixed-pattern rule (FUN_1c00e100..e600).
pub fn f_1c00a910(m: &mut Mem, p1: u32, p2: u32, p3: u32, p4: u32, p5: u32) -> u32 {
    let lts = m.r32(p1 + 0x10);
    if f_1c00ada0(m, lts, p2, p3) == 0 {
        return 0;
    }
    if f_1c00ae50(m, p1, p2, p3, p5) == 0 {
        return 0;
    }

    if p5 != 0 && p5 != 3 {
        let mut i = 0;
        while i < count(m, lts) {
            let n = m.r32(slot(lts, i));
            m.w32(n + 0x10, 0x7a);
            i += 1;
        }
        match p5 {
            1 => return f_1c00e100(m, lts, p2, p3, p4),
            2 => return f_1c00e150(m, lts, p2, p3, p4),
            4 => return f_1c00e280(m, lts, p2, p3, p4),
            5 => return f_1c00e340(m, lts, p2, p3, p4),
            6 => return f_1c00e380(m, lts, p2, p3, p4),
            7 => return f_1c00e600(m, lts, p2, p3, p4),
            _ => {}
        }
    }

    let dec = |m: &mut Mem| {
        let v = split(m, lts) - 1;
        m.w16(lts + 0x434, v);
    };
    let inc = |m: &mut Mem| {
        let v = split(m, lts) + 1;
        m.w16(lts + 0x434, v);
    };
    let flag20 = |m: &Mem, i: i32| m.r8(m.r32(slot(lts, i)) + 0xc) & 0x20 != 0;

    let s = split(m, lts);
    if s < 0 {
        if !flag20(m, 0) {
            f_1c00b020(m, lts);
            return a910_tail(m, lts, p4);
        }
        m.w16(lts + 0x434, 0);
        let lim = count(m, lts) - 1;
        if lim > 0 {
            while flag20(m, split(m, lts)) {
                inc(m);
                if split(m, lts) >= lim {
                    break;
                }
            }
        }
        a910_forward(m, lts, lim);
    } else {
        let n = m.r32(slot(lts, s));
        let v = m.r32(n + 0x10);
        if (v == 0x7d || v == 0x7e) && s != 0 {
            m.w32(n + 0x10, 0x7a);
            dec(m);
            let s2 = split(m, lts);
            if v == 0x7e && s2 != 0 {
                let n2 = m.r32(slot(lts, s2));
                m.w32(n2 + 0x10, 0x7a);
                dec(m);
            }
        }
        while split(m, lts) != 0 {
            let s = split(m, lts);
            if f_1c00ad60(m, lts, s as u32) == 0 {
                break;
            }
            dec(m);
        }
        let lim = count(m, lts) - 1;
        while split(m, lts) < lim && flag20(m, split(m, lts)) {
            inc(m);
        }
        a910_forward(m, lts, lim);
    }
    a910_tail(m, lts, p4)
}

/// FUN_1c00a910: advance the split past heads whose phrase ends in form class 0x22.
fn a910_forward(m: &mut Mem, lts: u32, lim: i32) {
    if split(m, lts) >= lim {
        return;
    }
    loop {
        let s = split(m, lts);
        if f_1c00ad60(m, lts, s as u32) == 0 {
            break;
        }
        m.w16(lts + 0x434, s + 1);
        if split(m, lts) >= count(m, lts) - 1 {
            break;
        }
    }
}

/// FUN_1c00a910 tail: tag the split head with `class`, alternate 0x79 heads after it, then B150.
fn a910_tail(m: &mut Mem, lts: u32, class: u32) -> u32 {
    let n = m.r32(slot(lts, split(m, lts)));
    m.w32(n + 0x10, class);
    let mut skip = true;
    let mut i = split(m, lts) + 1;
    while i < count(m, lts) {
        let n = m.r32(slot(lts, i));
        if skip {
            skip = false;
        } else {
            if m.r32(n + 0x10) == 0x79 {
                or_flags(m, n, 0x80);
            }
            skip = true;
        }
        if m.r32(n + 0x10) == 0x79 {
            m.w32(n + 0x10, 0x7a);
        }
        i += 1;
    }
    f_1c00b150(m, lts)
}

/// True if the phrase starting at head slot p2 ends (after attaching words) in form class 0x22.
pub fn f_1c00ad60(m: &mut Mem, p1: u32, p2: u32) -> u32 {
    let mut n = m.r32(p1.wrapping_add(p2.wrapping_mul(4)).wrapping_add(0x408));
    while f_1c00a8d0(m, n) != 0 {
        n = nx(m, n);
    }
    (tg(m, n) == 0x22) as u32
}

/// Collects phrase heads of [p2, p3) into lts+0x408 (max 10); records first classed head and the
/// first head with class >= 0x7c. Returns 0 on overflow.
pub fn f_1c00ada0(m: &mut Mem, p1: u32, p2: u32, p3: u32) -> u32 {
    let lts = p1;
    m.w16(lts + 0x432, -1);
    m.w16(lts + 0x434, -1);
    m.w16(lts + 0x430, 0);
    let mut n = p2;
    while n != p3 {
        if head_bit(m, n) {
            let c = count(m, lts);
            if c >= 0xa {
                return 0;
            }
            let class = m.r32(n + 0x10);
            if class != 0x79 {
                if m.s(lts + 0x432) < 0 {
                    m.w16(lts + 0x432, c);
                }
                if m.s(lts + 0x434) < 0 && class as i32 >= 0x7c {
                    m.w16(lts + 0x434, c);
                }
            }
            m.w32(slot(lts, c), n);
            m.w16(lts + 0x430, c + 1);
        }
        n = nx(m, n);
    }
    if m.s(lts + 0x432) < 0 {
        let c = m.s(lts + 0x430);
        m.w16(lts + 0x432, c);
    }
    1
}

/// Marks phrase heads that must not be split (flag 0x20) using the phrase-pattern table at
/// 0x1c032364 (entries: {kind, flags|len, form classes[len]}); p4 = pattern kind. Always 1.
pub fn f_1c00ae50(m: &mut Mem, p1: u32, p2: u32, p3: u32, p4: u32) -> u32 {
    let lts = m.r32(p1 + 0x10);
    let end = p3;
    let mut cur = p2;
    let mut idx: i32 = 0;
    if count(m, lts) - 1 <= 0 {
        return 1;
    }
    let mut ptr = lts + 0x408;
    'restart: loop {
        let mut e = 0x1c032364u32;
        loop {
            let len = (m.r8(e + 1) & 0xf) as u32;
            if len == 0 {
                if idx >= count(m, lts) - 1 || cur == end || f_1c00a8f0(m, cur) == 0 {
                    return 1;
                }
                let a = nx(m, cur);
                if a == end || f_1c00a8d0(m, a) == 0 {
                    return 1;
                }
                let b = nx(m, a);
                if b == end || !head_bit(m, b) || tg(m, a) != tg(m, b) {
                    return 1;
                }
                or_flags(m, m.r32(slot(lts, idx)), 0x20);
                return 1;
            }
            let skip = e + len + 2;
            let kind = m.r8(e);
            if kind != 0xff && sx8(kind as u32) != p4 {
                e = skip;
                continue;
            }
            let mut w = cur;
            let mut i = 0u32;
            while (i as i32) < len as i32 {
                if w == end || tg(m, w) != sx8(m.r8(e + 2 + i) as u32) {
                    break;
                }
                w = nx(m, w);
                i += 1;
            }
            if i != len {
                e = skip;
                continue;
            }
            if w != end && m.r8(w + 0xc) & 0x40 == 0 {
                e = skip;
                continue;
            }
            if m.r8(e + 1) & 0x20 != 0 && (w == end || f_1c00a8d0(m, w) == 0) {
                e = skip;
                continue;
            }
            if m.r8(e + 1) & 0x40 != 0 && (w == end || f_1c00a8f0(m, w) == 0) {
                e = skip;
                continue;
            }

            let n = m.r32(ptr);
            ptr += 4;
            or_flags(m, n, 0x20);
            idx += 1;
            if m.r8(e + 1) & 0x80 != 0 && idx < count(m, lts) - 1 {
                let n = m.r32(ptr);
                or_flags(m, n, 0x20);
                idx += 1;
                ptr += 4;
            }
            if m.r8(e + 1) & 0x10 == 0 {
                return 1;
            }
            cur = w;
            if idx < count(m, lts) - 1 {
                continue 'restart;
            }
            return 1;
        }
    }
}

/// Default split when no head carries a forced class, keyed on the first classed head (lts+0x432).
pub fn f_1c00b020(m: &mut Mem, p1: u32) -> u32 {
    let lts = p1;
    let first = m.s(lts + 0x432);
    match first {
        0 | 1 => m.w16(lts + 0x434, 0),
        2 => return f_1c00b0c0(m, lts),
        3 => {
            m.w16(lts + 0x434, 0);
            let n = pv(m, m.r32(lts + 0x410));
            if f_1c00a8d0(m, n) != 0 {
                m.w16(lts + 0x434, 1);
            }
        }
        _ => {
            let n = m.r32(lts.wrapping_add((first as u32).wrapping_mul(4)).wrapping_add(0x404));
            if tg(m, pv(m, n)) == 1 && tg(m, n) == 0x11 {
                m.w16(lts + 0x434, first - 3);
            } else {
                m.w16(lts + 0x434, first - 2);
            }
        }
    }
    0
}

/// Split index for a two-head clause: 1 if the first phrase looks like a short subject, else 0.
pub fn f_1c00b0c0(m: &mut Mem, p1: u32) -> u32 {
    let lts = p1;
    let n = m.r32(lts + 0x408);
    let t = tg(m, n);
    if t == 5 && head_bit(m, nx(m, n)) {
        m.w16(lts + 0x434, 1);
        return 0;
    }
    if f_1c00a8f0(m, n) != 0
        && f_1c00a8d0(m, nx(m, n)) != 0
        && f_1c00a8d0(m, nx(m, nx(m, n))) != 0
        && matches!(t, 5 | 4 | 2 | 9)
    {
        m.w16(lts + 0x434, 1);
        return 0;
    }
    m.w16(lts + 0x434, 0);
    0
}

/// Alternates 0x7a/0x7b classes over unclassed heads before the split, then applies two
/// clause-level fixups on the word list at lts+0x8c8.
pub fn f_1c00b150(m: &mut Mem, p1: u32) -> u32 {
    let lts = p1;
    let mut prev = 0x7cu32;
    while split(m, lts) != 0 {
        let s = split(m, lts);
        let n = m.r32(slot(lts, s - 1));
        if m.r32(n + 0x10) == 0x79 {
            break;
        }
        m.w16(lts + 0x434, s - 1);
        prev = m.r32(m.r32(slot(lts, s - 1)) + 0x10);
    }
    while split(m, lts) != 0 {
        let s = split(m, lts) - 1;
        m.w16(lts + 0x434, s);
        if prev == 0x7a {
            let n = m.r32(slot(lts, s));
            if m.r32(n + 0x10) == 0x79 {
                or_flags(m, n, 0x80);
            }
            prev = 0x7b;
        } else {
            prev = 0x7a;
        }
        let n = m.r32(slot(lts, split(m, lts)));
        if m.r32(n + 0x10) == 0x79 {
            m.w32(n + 0x10, 0x7a);
        }
    }

    let head = lts + 0x8c8;
    let f = nx(m, head);
    if tg(m, f) == 0x11 {
        let a = nx(m, f);
        if a != head && tg(m, a) == 0x20 {
            let b = nx(m, a);
            if b != head && head_bit(m, b) {
                m.w32(f + 0x14, 9);
                m.w32(f + 0x18, 0);
                m.w32(f + 0x10, 0x7c);
            }
        }
    }
    if count(m, lts) == 3 {
        let mut l = m.r32(lts + 0x8cc);
        if tg(m, l) == 0x2a {
            l = pv(m, l);
        }
        if l != head && tg(m, l) == 6 && m.r32(l + 0x18) == 0x11 {
            let n = m.r32(lts + 0x410);
            let fl = m.r32(n + 0xc) & 0xffff_ff7f;
            m.w32(n + 0xc, fl);
        }
    }
    0
}

/// True if the p4 (low 16 bits) nodes from p1 (stopping at p2) have the form classes in the
/// signed byte string at p3.
pub fn f_1c00e0c0(m: &mut Mem, p1: u32, p2: u32, p3: u32, p4: u32) -> u32 {
    let len = (p4 & 0xffff) as i32;
    let mut n = p1;
    let mut i: i32 = 0;
    while i < len {
        if n == p2 || tg(m, n) != sx8(m.r8(p3.wrapping_add(i as u32)) as u32) {
            break;
        }
        n = nx(m, n);
        i += 1;
    }
    (i == len) as u32
}

/// Places a pattern on the scratch stack and runs FUN_1c00e0c0 against it.
fn match_pat(m: &mut Mem, start: u32, end: u32, pat: &[u8]) -> bool {
    let sp = m.sp();
    let buf = m.alloca(pat.len() as u32);
    for (k, &b) in pat.iter().enumerate() {
        m.w8(buf + k as u32, b);
    }
    let r = f_1c00e0c0(m, start, end, buf, pat.len() as u32);
    m.set_sp(sp);
    r != 0
}

/// Common tail of FUN_1c00e100..e600: clamp the split to >= 1 and class head[split-1] as `class`.
fn set_split_class(m: &mut Mem, lts: u32, class: u32) -> u32 {
    if split(m, lts) < 1 {
        m.w16(lts + 0x434, 1);
    }
    let s = split(m, lts);
    let n = m.r32(lts.wrapping_add((s as u32).wrapping_mul(4)).wrapping_add(0x404));
    m.w32(n + 0x10, class);
    0
}

/// Pattern rule 1: split after head 2 of a two-head clause, else after head 1.
pub fn f_1c00e100(m: &mut Mem, p1: u32, _p2: u32, _p3: u32, p4: u32) -> u32 {
    let s = if count(m, p1) == 2 { 2 } else { 1 };
    m.w16(p1 + 0x434, s);
    set_split_class(m, p1, p4)
}

/// Pattern rule 2: split 2 only for the 3-word form sequence {26,05,20}, else 1.
pub fn f_1c00e150(m: &mut Mem, p1: u32, p2: u32, p3: u32, p4: u32) -> u32 {
    let s = if match_pat(m, p2, p3, &[0x26, 0x05, 0x20, 0x30, 0x11, 0x20])
        || match_pat(m, p2, p3, &[0x26, 0x05, 0x20, 0x30, 0x0f])
        || match_pat(m, p2, p3, &[0x26, 0x05, 0x20, 0x30, 0x11])
    {
        1
    } else if match_pat(m, p2, p3, &[0x26, 0x05, 0x20]) {
        2
    } else {
        1
    };
    m.w16(p1 + 0x434, s);
    set_split_class(m, p1, p4)
}

/// Pattern rule 4: split before the last one or two heads depending on the clause-final words.
pub fn f_1c00e280(m: &mut Mem, p1: u32, _p2: u32, p3: u32, p4: u32) -> u32 {
    let start = pv(m, pv(m, p3));
    let hit = match_pat(m, start, p3, &[0x01, 0x0b]) || {
        let start = pv(m, pv(m, p3));
        match_pat(m, start, p3, &[0x01, 0x11])
    };
    let s = count(m, p1) - if hit { 2 } else { 1 };
    m.w16(p1 + 0x434, s);
    set_split_class(m, p1, p4)
}

/// Pattern rule 5: split before the last head.
pub fn f_1c00e340(m: &mut Mem, p1: u32, _p2: u32, _p3: u32, p4: u32) -> u32 {
    let s = count(m, p1) - 1;
    m.w16(p1 + 0x434, s);
    set_split_class(m, p1, p4)
}

/// Pattern rule 6: split before the last two heads if the clause tail matches any known pattern,
/// else before the last head.
pub fn f_1c00e380(m: &mut Mem, p1: u32, _p2: u32, p3: u32, p4: u32) -> u32 {
    let w3 = pv(m, pv(m, pv(m, p3)));
    let hit = match_pat(m, w3, p3, &[0x26, 0x02, 0x36])
        || match_pat(m, w3, p3, &[0x18, 0x02, 0x36])
        || {
            let w4 = pv(m, w3);
            match_pat(m, w4, p3, &[0x26, 0x02, 0x2f, 0x29])
        }
        || match_pat(m, w3, p3, &[0x36, 0x05, 0x31])
        || match_pat(m, w3, p3, &[0x36, 0x04, 0x31])
        || match_pat(m, w3, p3, &[0x36, 0x0d, 0x31])
        || match_pat(m, w3, p3, &[0x36, 0x02, 0x31])
        || {
            let w2 = nx(m, w3);
            match_pat(m, w2, p3, &[0x0a, 0x25])
        }
        || {
            let w4 = pv(m, w3);
            match_pat(m, w4, p3, &[0x18, 0x02, 0x2f, 0x29])
        }
        || match_pat(m, w3, p3, &[0x01, 0x05, 0x31]);
    let s = count(m, p1) - if hit { 2 } else { 1 };
    m.w16(p1 + 0x434, s);
    set_split_class(m, p1, p4)
}

/// Pattern rule 7: no split if the last word is 1a/1b/20/30 (or a two-head clause ends 04 2a);
/// otherwise before the last two heads after 01 0b / 01 11, else before the last head.
pub fn f_1c00e600(m: &mut Mem, p1: u32, _p2: u32, p3: u32, p4: u32) -> u32 {
    let w1 = pv(m, p3);
    let whole = match_pat(m, w1, p3, &[0x1a])
        || match_pat(m, w1, p3, &[0x1b])
        || match_pat(m, w1, p3, &[0x20])
        || match_pat(m, w1, p3, &[0x30])
        || (count(m, p1) == 2 && {
            let w2 = pv(m, w1);
            match_pat(m, w2, p3, &[0x04, 0x2a])
        });
    let s = if whole {
        count(m, p1)
    } else {
        let w2 = pv(m, w1);
        let hit = match_pat(m, w2, p3, &[0x01, 0x0b]) || {
            let w2 = pv(m, w1);
            match_pat(m, w2, p3, &[0x01, 0x11])
        };
        count(m, p1) - if hit { 2 } else { 1 }
    };
    m.w16(p1 + 0x434, s);
    set_split_class(m, p1, p4)
}
