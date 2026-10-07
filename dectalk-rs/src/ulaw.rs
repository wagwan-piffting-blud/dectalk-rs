//! G.711 mu-law encoding for the 8 kHz telephone output format, as the original encoder does it:
//! table-driven exponent and a 16-bit negate, so -32768 stays negative and encodes as 0x7f.

const BIAS: i32 = 0x84;
const CLIP: i32 = 32635;

/// exp_lut[i] = floor(log2(i)) for i >= 1, 0 for i = 0.
fn exponent(index: i32) -> i32 {
    if index <= 1 {
        0
    } else {
        31 - index.leading_zeros() as i32
    }
}

/// Encode one 16-bit linear sample.
pub fn encode(sample: i16) -> u8 {
    let mut s = sample;
    let sign = if s < 0 {
        s = s.wrapping_neg();
        0x80
    } else {
        0
    };
    let s = (s as i32).min(CLIP) + BIAS;
    let exp = exponent((s >> 7) & 0xff);
    let mantissa = (s >> (exp + 3)) & 0x0f;
    !(sign | (exp << 4) as u8 | mantissa as u8)
}

pub fn encode_all(pcm: &[i16]) -> Vec<u8> {
    pcm.iter().map(|&s| encode(s)).collect()
}
