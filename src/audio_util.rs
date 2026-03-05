//! Audio utility for mu-law <-> PCM16 conversion and resampling.
//! Essential for Twilio (8kHz mu-law) to Vertex (16kHz PCM16) bridging.

/// Linear mu-law to PCM16 lookup table.
const MULAW_TO_PCM: [i16; 256] = [
    -32124, -31100, -30076, -29052, -28028, -27004, -25980, -24956, -23932, -22908, -21884, -20860,
    -19836, -18812, -17788, -16764, -15996, -15484, -14972, -14460, -13948, -13436, -12924, -12412,
    -11900, -11388, -10876, -10364, -9852, -9340, -8828, -8316, -7932, -7676, -7420, -7164, -6908,
    -6652, -6396, -6140, -5884, -5628, -5372, -5116, -4860, -4604, -4348, -4092, -3900, -3772, -3644,
    -3516, -3388, -3260, -3132, -3004, -2876, -2748, -2620, -2492, -2364, -2236, -2108, -1980, -1884,
    -1820, -1756, -1692, -1628, -1564, -1500, -1436, -1372, -1308, -1244, -1180, -1116, -1052, -988,
    -924, -876, -844, -812, -780, -748, -716, -684, -652, -620, -588, -556, -524, -492, -460, -428,
    -396, -372, -356, -340, -324, -308, -292, -276, -260, -244, -228, -212, -196, -180, -164, -148,
    -132, -120, -112, -104, -96, -88, -80, -72, -64, -56, -48, -40, -32, -24, -16, -8, 0, 32124,
    31100, 30076, 29052, 28028, 27004, 25980, 24956, 23932, 22908, 21884, 20860, 19836, 18812,
    17788, 16764, 15996, 15484, 14972, 14460, 13948, 13436, 12924, 12412, 11900, 11388, 10876,
    10364, 9852, 9340, 8828, 8316, 7932, 7676, 7420, 7164, 6908, 6652, 6396, 6140, 5884, 5628, 5372,
    5116, 4860, 4604, 4348, 4092, 3900, 3772, 3644, 3516, 3388, 3260, 3132, 3004, 2876, 2748, 2620,
    2492, 2364, 2236, 2108, 1980, 1884, 1820, 1756, 1692, 1628, 1564, 1500, 1436, 1372, 1308, 1244,
    1180, 1116, 1052, 988, 924, 876, 844, 812, 780, 748, 716, 684, 652, 620, 588, 556, 524, 492, 460,
    428, 396, 372, 356, 340, 324, 308, 292, 276, 260, 244, 228, 212, 196, 180, 164, 148, 132, 120,
    112, 104, 96, 88, 80, 72, 64, 56, 48, 40, 32, 24, 16, 8, 0,
];

/// Convert mu-law bytes to PCM16 samples.
pub fn mulaw_to_pcm16(input: &[u8]) -> Vec<i16> {
    input.iter().map(|&b| MULAW_TO_PCM[b as usize]).collect()
}

/// Convert PCM16 samples to mu-law bytes.
pub fn pcm16_to_mulaw(input: &[i16]) -> Vec<u8> {
    input.iter().map(|&s| encode_mulaw(s)).collect()
}

/// Encode a single PCM16 sample as mu-law.
fn encode_mulaw(pcm: i16) -> u8 {
    let mut pcm = pcm;
    let sign = (pcm >> 8) & 0x80;
    if sign != 0 {
        pcm = -pcm;
    }
    if pcm > 32635 {
        pcm = 32635;
    }
    pcm += 0x84;
    let mut exponent = 7;
    let mut mask = 0x4000;
    while (pcm & mask) == 0 && exponent > 0 {
        mask >>= 1;
        exponent -= 1;
    }
    let mantissa = (pcm >> (exponent + 3)) & 0x0f;
    let ulaw = (sign | (exponent << 4) | mantissa) as u8;
    !ulaw
}

/// Upsample PCM16 from 8kHz to 16kHz using linear interpolation.
pub fn upsample_8_to_16(input: &[i16]) -> Vec<i16> {
    if input.is_empty() {
        return Vec::new();
    }
    let mut output = Vec::with_capacity(input.len() * 2);
    for i in 0..input.len() - 1 {
        let current = input[i];
        let next = input[i + 1];
        output.push(current);
        output.push((current / 2) + (next / 2));
    }
    // Handle last sample
    output.push(input[input.len() - 1]);
    output.push(input[input.len() - 1]);
    output
}

/// Downsample PCM16 from 16kHz to 8kHz (decimation).
pub fn downsample_16_to_8(input: &[i16]) -> Vec<i16> {
    input.iter().step_by(2).copied().collect()
}
