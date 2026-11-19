//! DSD -> PCM decimation: Kaiser-windowed-sinc FIR low-pass + polyphase
//! rational resampling.
//!
//! The DSD noise shaper pushes huge quantization noise above ~20-30 kHz, so
//! the anti-alias low-pass must run at (nearly) the full DSD rate before any
//! decimation; otherwise that noise aliases into the audible band.
//!
//! `Polyphase` implements y[n] = sum_k h[k] * x_up[n*M - k] with
//! x_up[m] = x[m/L] if m % L == 0 else 0 (upsampled-by-L domain), i.e. only
//! taps k with k ≡ n*M (mod L) contribute — the classic polyphase form.

use std::collections::VecDeque;

fn gcd(mut a: u64, mut b: u64) -> u64 {
    while b != 0 {
        let t = a % b;
        a = b;
        b = t;
    }
    a
}

/// Kaiser I0 (modified Bessel function of the first kind, order 0).
fn i0(x: f64) -> f64 {
    let y = x / 2.0;
    let mut sum = 1.0;
    let mut term = 1.0;
    for k in 1..200 {
        term *= (y / k as f64) * (y / k as f64);
        sum += term;
        if term < 1e-14 * sum {
            break;
        }
    }
    sum
}

/// Design a linear-phase low-pass FIR (odd length, DC gain = 1) at sample
/// rate `fs` (Hz), with `cutoff` and `stopband` in Hz and ~`atten_db`
/// stopband attenuation (Kaiser window).
pub fn design_lowpass(fs: f64, cutoff: f64, stopband: f64, atten_db: f64) -> Vec<f64> {
    let beta = if atten_db > 50.0 {
        0.1102 * (atten_db - 8.7)
    } else if atten_db > 21.0 {
        0.5842 * (atten_db - 21.0).powf(0.4) + 0.07886 * (atten_db - 21.0)
    } else {
        0.0
    };
    let trans = (stopband - cutoff).max(1.0);
    let mut n =
        ((atten_db - 8.0) / (2.285 * 2.0 * std::f64::consts::PI * trans / fs)).ceil() as usize + 1;
    if n.is_multiple_of(2) {
        n += 1;
    }
    let d = (n - 1) / 2;
    let fc = cutoff / fs; // cycles per sample
    let mut h = vec![0.0f64; n];
    for (i, v) in h.iter_mut().enumerate() {
        let x = i as f64 - d as f64;
        let sinc = if x == 0.0 {
            2.0 * fc
        } else {
            (2.0 * std::f64::consts::PI * fc * x).sin() / (std::f64::consts::PI * x)
        };
        let r = x / d as f64;
        let w = i0(beta * (1.0 - r * r).max(0.0).sqrt()) / i0(beta);
        *v = sinc * w;
    }
    let sum: f64 = h.iter().sum();
    for v in h.iter_mut() {
        *v /= sum;
    }
    h
}

/// Streaming polyphase rational resampler fs_in -> fs_out (both integer Hz).
/// `cutoff`/`stopband` in Hz, must satisfy cutoff < stopband <= fs_out/2.
pub struct Polyphase {
    h: Vec<f64>,
    l: usize,
    m: usize,
    jmax: u64,
    hist: VecDeque<f64>,
    hist_start: u64,
    pushed: u64,
    next_out: u64,
}

impl Polyphase {
    pub fn new(fs_in: u64, fs_out: u64, cutoff: f64, stopband: f64, atten_db: f64) -> Self {
        assert!(fs_in > 0 && fs_out > 0);
        let g = gcd(fs_in, fs_out);
        let l = (fs_out / g) as usize;
        let m = (fs_in / g) as usize;
        let fs_i = fs_in as f64 * l as f64;
        let stop = stopband.min(fs_out as f64 / 2.0);
        let mut h = design_lowpass(fs_i, cutoff, stop, atten_db);
        // Up by L inserts L-1 zeros between samples, so the filter needs DC
        // gain L in the upsampled domain to keep the signal amplitude.
        for v in h.iter_mut() {
            *v *= l as f64;
        }
        let jmax = ((h.len() - 1) / l) as u64;
        Polyphase {
            h,
            l,
            m,
            jmax,
            hist: VecDeque::new(),
            hist_start: 0,
            pushed: 0,
            next_out: 0,
        }
    }

    /// Feed input samples; returns the output samples produced.
    pub fn push(&mut self, x: &[f64]) -> Vec<f64> {
        self.hist.extend(x.iter().copied());
        self.pushed += x.len() as u64;
        let mut out = Vec::new();
        let n = self.h.len();
        loop {
            let t = self.next_out * self.m as u64;
            let base = t / self.l as u64;
            if base >= self.pushed {
                break;
            }
            let phase = (t % self.l as u64) as usize;
            let mut acc = 0.0f64;
            let mut j = 0u64;
            loop {
                let k = phase + j as usize * self.l;
                if k >= n {
                    break;
                }
                let s = if j > base {
                    0.0 // before the start of the stream
                } else {
                    let idx = base - j;
                    if idx >= self.hist_start {
                        self.hist
                            .get((idx - self.hist_start) as usize)
                            .copied()
                            .unwrap_or(0.0)
                    } else {
                        0.0
                    }
                };
                acc += self.h[k] * s;
                j += 1;
            }
            out.push(acc);
            self.next_out += 1;
            let keep = base.saturating_sub(self.jmax);
            if keep > self.hist_start {
                let cut = ((keep - self.hist_start) as usize).min(self.hist.len());
                self.hist.drain(..cut);
                self.hist_start += cut as u64;
            }
        }
        out
    }

    /// Flush: emit outputs that only need zero-padded history.
    pub fn finish(&mut self) -> Vec<f64> {
        let mut out = Vec::new();
        let n = self.h.len();
        loop {
            let t = self.next_out * self.m as u64;
            let base = t / self.l as u64;
            let phase = (t % self.l as u64) as usize;
            // last input index is pushed-1; allow base up to pushed-1+jmax (zeros beyond)
            if base > self.pushed - 1 + self.jmax {
                break;
            }
            let mut acc = 0.0f64;
            let mut j = 0u64;
            loop {
                let k = phase + j as usize * self.l;
                if k >= n {
                    break;
                }
                let s = if j > base || base - j >= self.pushed {
                    0.0
                } else {
                    let idx = base - j;
                    if idx >= self.hist_start {
                        self.hist
                            .get((idx - self.hist_start) as usize)
                            .copied()
                            .unwrap_or(0.0)
                    } else {
                        0.0
                    }
                };
                acc += self.h[k] * s;
                j += 1;
            }
            out.push(acc);
            self.next_out += 1;
        }
        out
    }
}

/// Expand DSD bytes to +/-1.0 samples. `msb_first` = SACD/DFF native order
/// (bit 7 is the oldest); DSF/DoP are LSB-first (bit 0 is the oldest).
pub fn expand_bits(bytes: &[u8], msb_first: bool, out: &mut Vec<f64>) {
    out.reserve(bytes.len() * 8);
    for &b in bytes {
        if msb_first {
            for i in (0..8).rev() {
                out.push(if (b >> i) & 1 == 1 { 1.0 } else { -1.0 });
            }
        } else {
            for i in 0..8 {
                out.push(if (b >> i) & 1 == 1 { 1.0 } else { -1.0 });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Goertzel amplitude of `freq` in `x` sampled at `fs`.
    fn goertzel(x: &[f64], fs: f64, freq: f64) -> f64 {
        let w = 2.0 * std::f64::consts::PI * freq / fs;
        let (mut s1, mut s2) = (0.0, 0.0);
        for &v in x {
            let s0 = v + 2.0 * w.cos() * s1 - s2;
            s2 = s1;
            s1 = s0;
        }
        let power = s1 * s1 + s2 * s2 - 2.0 * w.cos() * s1 * s2;
        2.0 * power.max(0.0).sqrt() / x.len() as f64
    }

    #[test]
    fn fir_dc_gain_is_one() {
        let h = design_lowpass(2_822_400.0, 40_000.0, 44_100.0, 100.0);
        let sum: f64 = h.iter().sum();
        assert!((sum - 1.0).abs() < 1e-9);
        assert!(h.len() % 2 == 1);
    }

    #[test]
    fn sine_passes_and_ultrasonic_is_rejected() {
        let fs_d = 2_822_400u64;
        let fs_mid = fs_d / 16; // 176400
        let rate = 96_000u64;

        let n_in = fs_d as usize * 2; // 2 s
        let run = |freq: f64| -> Vec<f64> {
            // fresh resamplers per run: stage 1 DSD -> 176.4k, stage 2 -> 96k
            let mut r1 = Polyphase::new(
                fs_d,
                fs_mid,
                0.45 * fs_mid as f64,
                fs_mid as f64 / 2.0,
                100.0,
            );
            let mut r2 = Polyphase::new(fs_mid, rate, 0.45 * rate as f64, rate as f64 / 2.0, 100.0);
            let mut mid_all = Vec::new();
            let chunk = 8192 * 4;
            let mut i = 0usize;
            while i < n_in {
                let n = chunk.min(n_in - i);
                let bits: Vec<f64> = (i..i + n)
                    .map(|k| {
                        let ph = 2.0 * std::f64::consts::PI * freq * k as f64 / fs_d as f64;
                        if ph.sin() >= 0.0 { 1.0 } else { -1.0 }
                    })
                    .collect();
                let mid = r1.push(&bits);
                mid_all.extend_from_slice(&mid);
                i += n;
            }
            mid_all.extend_from_slice(&r1.finish());
            let mut out = Vec::new();
            let chunk2 = 65536;
            let mut i = 0;
            while i < mid_all.len() {
                let n = chunk2.min(mid_all.len() - i);
                out.extend_from_slice(&r2.push(&mid_all[i..i + n]));
                i += n;
            }
            out.extend_from_slice(&r2.finish());
            out
        };

        // square-wave fundamental amplitude is 4/pi ~ 1.2732
        let out1k = run(1000.0);
        let a1k = goertzel(&out1k[rate as usize / 10..], rate as f64, 1000.0);
        assert!(
            (a1k - 4.0 / std::f64::consts::PI).abs() / (4.0 / std::f64::consts::PI) < 0.02,
            "1 kHz amplitude {a1k}"
        );
        let out60k = run(60_000.0);
        let a60k = goertzel(
            &out60k[rate as usize / 10..],
            rate as f64,
            60_000.0f64.min(rate as f64 / 2.0),
        );
        // 60 kHz aliases/stopband: must be tiny vs the 1 kHz reference
        assert!(
            a60k < 0.01 * 4.0 / std::f64::consts::PI,
            "60 kHz leak {a60k}"
        );
    }

    #[test]
    fn expand_bits_orders() {
        let mut out = Vec::new();
        expand_bits(&[0b1000_0001], true, &mut out); // MSB first
        assert_eq!(out[0], 1.0);
        assert_eq!(out[7], 1.0);
        assert_eq!(out[1], -1.0);
        expand_bits(&[0b1000_0001], false, &mut out); // LSB first
        assert_eq!(out[8], 1.0); // bit0 first
        assert_eq!(out[15], 1.0); // bit7 last
    }
}
