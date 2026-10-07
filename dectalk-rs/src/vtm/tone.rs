//! VTM tone generator (FUN_1c0247f0): two summed sine oscillators with a sine-squared
//! attack/release ramp. All arithmetic is f64, matching the original's x87 code under MSVC's
//! 53-bit precision control; `ftol` truncates toward zero.

use std::sync::OnceLock;

/// The DLL's 1024-entry sine table: sin(2*pi*k/1024) printed with six decimals.
fn sine() -> &'static [f64; 1024] {
    static T: OnceLock<[f64; 1024]> = OnceLock::new();
    T.get_or_init(|| {
        let mut t = [0.0; 1024];
        for (k, v) in t.iter_mut().enumerate() {
            let s = (2.0 * std::f64::consts::PI * k as f64 / 1024.0).sin();
            *v = format!("{s:.6}").parse().unwrap();
        }
        t
    })
}

fn ftol(v: f64) -> i32 {
    v as i32
}

/// FUN_1c024b30: current table value, then advance the phase (1024 units per cycle).
fn osc(inc: f64, phase: &mut f64) -> f64 {
    let v = sine()[ftol(*phase) as usize];
    let next = inc + *phase;
    *phase = if next < 1024.0 { next } else { next - 1024.0 };
    v
}

/// Kind-2 VTM message: `ms` duration, `f1`/`a1` and `f2`/`a2` frequency/amplitude pairs.
/// Returns the samples, or None where the original fails (tones shorter than the two ramps).
pub fn tone(rate: u32, ms: u16, f1: u16, a1: u16, f2: u16, a2: u16) -> Option<Vec<i16>> {
    let rate_f = rate as f64;
    let inv_rate = 1.0 / rate_f;
    let (ms, f1, a1, f2, a2) = (ms as f64, f1 as f64, a1 as f64, f2 as f64, a2 as f64);
    let ramp = ftol(rate_f * 0.002);
    let steady = -2 * ramp - ftol(ms * rate_f * -0.001);
    let (ramp, steady) = if steady < 0 { (steady >> 1, 0) } else { (ramp, steady) };
    if ramp < 0 {
        // malloc(ramp * 8) fails in the original.
        return None;
    }
    let ramp = ramp as usize;
    let step = 256.0 / ramp as f64;
    let mut pos = 0.0;
    let mut env = Vec::with_capacity(ramp);
    for _ in 0..ramp {
        let v = sine()[ftol(pos) as usize];
        env.push(v * v);
        pos += step;
    }
    let inc1 = f1 * inv_rate * 1024.0;
    let inc2 = f2 * inv_rate * 1024.0;
    let (mut ph1, mut ph2) = (0.0, 0.0);
    let mut sample = |gain: Option<f64>| {
        let s1 = osc(inc1, &mut ph1) * a1;
        let s2 = osc(inc2, &mut ph2) * a2;
        let s = s2 + s1;
        ftol(match gain {
            Some(g) => s * g,
            None => s,
        }) as i16
    };
    let mut out = Vec::with_capacity(2 * ramp + steady as usize);
    for &g in &env {
        out.push(sample(Some(g)));
    }
    for _ in 0..steady {
        out.push(sample(None));
    }
    for &g in env.iter().rev() {
        out.push(sample(Some(g)));
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    #[test]
    fn sine_table_matches_dll() {
        let mut m = crate::ph::mem::Mem::new();
        for k in 0..1024u32 {
            let lo = m.r32(0x1c06_a9c0 + k * 8) as u64;
            let hi = m.r32(0x1c06_a9c4 + k * 8) as u64;
            assert_eq!(super::sine()[k as usize].to_bits(), lo | hi << 32, "entry {k}");
        }
        let _ = &mut m;
    }
}
