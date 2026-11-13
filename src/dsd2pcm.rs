//! `dsd2pcm`: convert DSD (DSF / DFF / DoP-WAV file, or a SACD-R ISO) to linear
//! PCM and write WAV, FLAC or WavPack.
//!
//! DSP chain: the 1-bit DSD stream (expanded to +/-1.0 samples in the correct
//! bit order: MSB-first for SACD/DFF native, LSB-first for DSF/DoP) goes through
//! a two-stage polyphase Kaiser FIR low-pass (see `resample`), because the DSD
//! noise shaper dumps huge ultrasonic noise above ~20-30 kHz that must be
//! filtered out before decimation. Stage 1 decimates to fs_d/2^k >= target
//! rate; stage 2 does the rational resample to the exact target rate.
//! A gain (default +6 dB, since DSD full scale sits ~6 dB below PCM 0 dBFS)
//! is applied and the result is clamped to full scale.

use crate::dop::{DOP_GROUP_BYTES, SAMPLES_PER_BLOCK_PAIR};
use crate::dsf::{AudioPacking, DsfInfo};
use crate::error::{Result, SacdError};
use crate::iso_read::{self, IsoTrack};
use crate::resample::{Polyphase, expand_bits};
use crate::sacd::SECTOR_PAIR_BYTES;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

/// DSD64 rate handled by all our parsers.
pub const DSD64: u32 = 2_822_400;

#[derive(Debug, Clone, Copy)]
pub struct PcmParams {
    /// Target PCM sample rate (44100/48000/88200/96000/176400/192000/...).
    pub rate: u32,
    /// Output bit depth: 16 or 24.
    pub bits: u8,
    /// Low-pass cutoff in Hz (default: min(50000, 0.45*rate)).
    pub lowpass: Option<f64>,
    /// Gain in dB applied after filtering (default +6 dB).
    pub gain_db: f64,
}

impl Default for PcmParams {
    fn default() -> Self {
        PcmParams {
            rate: 96_000,
            bits: 24,
            lowpass: None,
            gain_db: 6.0,
        }
    }
}

pub fn supported_rate(rate: u32) -> bool {
    matches!(
        rate,
        44_100 | 48_000 | 88_200 | 96_000 | 176_400 | 192_000 | 352_800 | 384_000
    )
}

impl PcmFormat {
    /// Max bit depth and sample rate for this output format.
    pub fn limits(self) -> (u8, u32) {
        match self {
            // WAV: integer PCM up to 32 bit / 384 kHz
            PcmFormat::Wav => (32, 384_000),
            // FLAC: up to 24 bit / 192 kHz (practical FLAC ceiling)
            PcmFormat::Flac => (24, 192_000),
            // WavPack: integer PCM up to 32 bit / 384 kHz
            PcmFormat::Wv => (32, 384_000),
        }
    }
}

/// Check that `params` are within the limits of `fmt`.
pub fn validate_params(fmt: PcmFormat, params: &PcmParams) -> Result<()> {
    let (max_bits, max_rate) = fmt.limits();
    if !supported_rate(params.rate) || params.rate > max_rate {
        return Err(SacdError::InvalidParams {
            msg: format!(
                "rate {} Hz not supported for {} (max {max_rate} Hz)",
                params.rate,
                fmt.ext()
            ),
        });
    }
    if !matches!(params.bits, 16 | 24 | 32) || params.bits > max_bits {
        return Err(SacdError::InvalidParams {
            msg: format!(
                "bit depth {} not supported for {} (max {max_bits} bits)",
                params.bits,
                fmt.ext()
            ),
        });
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PcmFormat {
    Wav,
    Flac,
    Wv,
}

impl PcmFormat {
    pub fn from_ext(ext: &str) -> Option<PcmFormat> {
        match ext.to_ascii_lowercase().as_str() {
            "wav" => Some(PcmFormat::Wav),
            "flac" => Some(PcmFormat::Flac),
            "wv" => Some(PcmFormat::Wv),
            _ => None,
        }
    }

    pub fn ext(self) -> &'static str {
        match self {
            PcmFormat::Wav => "wav",
            PcmFormat::Flac => "flac",
            PcmFormat::Wv => "wv",
        }
    }
}

/// Streaming DSD reader: yields 8192-byte byte-interleaved (L0 R0 L1 R1 ...)
/// blocks in the file's native bit order (bit order is handled later, at bit
/// expansion time).
struct DsdBlockReader {
    file: File,
    remaining_raw: u64,
    dop: bool,
    channels: usize,
    eof: bool,
}

impl DsdBlockReader {
    fn open(info: &DsfInfo) -> std::io::Result<Self> {
        let mut file = File::open(&info.path)?;
        file.seek(SeekFrom::Start(info.data_offset))?;
        let channels = info.channels as usize;
        if info.packing == AudioPacking::Dop && channels != 2 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "DoP (WAV-DSD) input must be stereo",
            ));
        }
        let remaining_raw = if info.packing == AudioPacking::Dop {
            info.data_size / SECTOR_PAIR_BYTES as u64 * DOP_GROUP_BYTES as u64
        } else {
            info.data_size
        };
        Ok(Self {
            file,
            remaining_raw,
            dop: info.packing == AudioPacking::Dop,
            channels,
            eof: false,
        })
    }

    fn read_padded(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let mut read = 0;
        while read < buf.len() {
            match self.file.read(&mut buf[read..])? {
                0 => break,
                n => read += n,
            }
        }
        for b in &mut buf[read..] {
            *b = 0;
        }
        self.remaining_raw = self.remaining_raw.saturating_sub(read as u64);
        Ok(read)
    }

    /// Next interleaved block: 4096*channels bytes, sample-interleaved
    /// (ch0[0], ch1[0], ..., chC-1[0], ch0[1], ...).
    fn next_block(&mut self) -> std::io::Result<Option<Vec<u8>>> {
        if self.eof || self.remaining_raw == 0 {
            return Ok(None);
        }
        let c = self.channels;
        let mut block = vec![0u8; 4096 * c];
        if self.dop {
            // stereo only (enforced in open())
            let mut chunk = [0u8; DOP_GROUP_BYTES];
            let n = self.read_padded(&mut chunk)?;
            if n == 0 {
                self.eof = true;
                return Ok(None);
            }
            let samples = (n / 6).min(SAMPLES_PER_BLOCK_PAIR);
            for s in 0..samples {
                let b = &chunk[s * 6..s * 6 + 6];
                block[4 * s] = b[0];
                block[4 * s + 1] = b[3];
                block[4 * s + 2] = b[1];
                block[4 * s + 3] = b[4];
            }
        } else {
            // DSF/DFF layout: C channel blocks of 4096 bytes, block by block
            let mut chan_blocks = vec![vec![0u8; 4096]; c];
            let mut any = 0usize;
            for cb in chan_blocks.iter_mut() {
                let want = 4096.min(self.remaining_raw as usize);
                any += self.read_padded(&mut cb[..want])?;
            }
            if any == 0 {
                self.eof = true;
                return Ok(None);
            }
            for i in 0..4096 {
                for ch in 0..c {
                    block[i * c + ch] = chan_blocks[ch][i];
                }
            }
        }
        Ok(Some(block))
    }
}

/// Convert a stream of interleaved DSD blocks (4096*channels bytes each,
/// sample-interleaved across channels) to interleaved i32 PCM (full-scale
/// i32; shift right for 24/16-bit output), calling `sink` with chunks.
/// Returns the number of output frames.
pub fn convert_blocks(
    mut next_block: impl FnMut() -> std::io::Result<Option<Vec<u8>>>,
    msb_first: bool,
    dsd_rate: u32,
    channels: usize,
    params: &PcmParams,
    mut sink: impl FnMut(&[i32]),
) -> std::io::Result<u64> {
    let fs_d = dsd_rate as u64;
    let rate = params.rate as u64;
    // stage 1: smallest power-of-two decimation keeping fs_mid >= rate
    let mut k = 0u32;
    while (fs_d >> (k + 1)) >= rate && (fs_d >> (k + 1)).is_multiple_of(2) {
        k += 1;
    }
    let fs_mid = fs_d >> k;
    let mut r1: Vec<Polyphase> = (0..channels)
        .map(|_| {
            Polyphase::new(
                fs_d,
                fs_mid,
                0.45 * fs_mid as f64,
                fs_mid as f64 / 2.0,
                100.0,
            )
        })
        .collect();
    let fc = params.lowpass.unwrap_or(50_000.0).min(0.45 * rate as f64);
    let use_stage2 = fs_mid != rate;
    let mut r2: Vec<Option<Polyphase>> = (0..channels)
        .map(|_| use_stage2.then(|| Polyphase::new(fs_mid, rate, fc, rate as f64 / 2.0, 100.0)))
        .collect();

    let gain = 10f64.powf(params.gain_db / 20.0);
    let to_i32 = |y: f64| -> i32 {
        let v = (y * gain).clamp(-1.0, 1.0);
        (v * 2_147_483_647.0).round() as i64 as i32
    };

    let mut frames_out = 0u64;
    // run per-channel mid-rate vectors through stage 2 and emit interleaved
    let process = |mids: &[Vec<f64>],
                   r2: &mut [Option<Polyphase>],
                   sink: &mut dyn FnMut(&[i32]),
                   frames_out: &mut u64| {
        let mut outs: Vec<Vec<f64>> = Vec::with_capacity(channels);
        for (r, mid) in r2.iter_mut().zip(mids.iter()) {
            outs.push(match r {
                Some(r) => r.push(mid),
                None => mid.clone(),
            });
        }
        let n = outs.iter().map(|v| v.len()).min().unwrap_or(0);
        let mut buf = Vec::with_capacity(n * channels);
        for i in 0..n {
            for out in outs.iter() {
                buf.push(to_i32(out[i]));
            }
        }
        *frames_out += n as u64;
        sink(&buf);
    };

    while let Some(block) = next_block()? {
        let mut mids = Vec::with_capacity(channels);
        for (c, r) in r1.iter_mut().enumerate() {
            let bytes: Vec<u8> = block.iter().skip(c).step_by(channels).copied().collect();
            let mut bits = Vec::with_capacity(bytes.len() * 8);
            expand_bits(&bytes, msb_first, &mut bits);
            mids.push(r.push(&bits));
        }
        process(&mids, &mut r2, &mut sink, &mut frames_out);
    }
    // flush stage 1, then stage 2
    let mids: Vec<Vec<f64>> = r1.iter_mut().map(|r| r.finish()).collect();
    process(&mids, &mut r2, &mut sink, &mut frames_out);
    let outs: Vec<Vec<f64>> = r2
        .iter_mut()
        .map(|r| match r {
            Some(r) => r.finish(),
            None => Vec::new(),
        })
        .collect();
    let n = outs.iter().map(|v| v.len()).min().unwrap_or(0);
    let mut buf = Vec::with_capacity(n * channels);
    for i in 0..n {
        for out in outs.iter() {
            buf.push(to_i32(out[i]));
        }
    }
    frames_out += n as u64;
    sink(&buf);
    Ok(frames_out)
}

// ---------- output sinks ----------

trait PcmSink {
    fn push(&mut self, samples: &[i32]) -> std::io::Result<()>;
    fn finish(self: Box<Self>) -> std::io::Result<()>;
}

struct WavSink {
    file: File,
    bits: u8,
    data_bytes: u64,
}

impl WavSink {
    fn create(path: &Path, params: &PcmParams, channels: u32) -> std::io::Result<Self> {
        let mut file = File::create(path)?;
        let ch = channels as u16;
        let bits = params.bits as u16;
        let block_align = ch * bits / 8;
        let byte_rate = params.rate * block_align as u32;
        let mut hdr = Vec::with_capacity(44);
        hdr.extend_from_slice(b"RIFF");
        hdr.extend_from_slice(&0u32.to_le_bytes()); // patched at finish
        hdr.extend_from_slice(b"WAVE");
        hdr.extend_from_slice(b"fmt ");
        hdr.extend_from_slice(&16u32.to_le_bytes());
        hdr.extend_from_slice(&1u16.to_le_bytes()); // PCM
        hdr.extend_from_slice(&ch.to_le_bytes());
        hdr.extend_from_slice(&params.rate.to_le_bytes());
        hdr.extend_from_slice(&byte_rate.to_le_bytes());
        hdr.extend_from_slice(&block_align.to_le_bytes());
        hdr.extend_from_slice(&bits.to_le_bytes());
        hdr.extend_from_slice(b"data");
        hdr.extend_from_slice(&0u32.to_le_bytes()); // patched at finish
        file.write_all(&hdr)?;
        Ok(WavSink {
            file,
            bits: params.bits,
            data_bytes: 0,
        })
    }
}

impl PcmSink for WavSink {
    fn push(&mut self, samples: &[i32]) -> std::io::Result<()> {
        let shift = 32 - self.bits;
        let nbytes = self.bits as usize / 8;
        let mut buf = Vec::with_capacity(samples.len() * nbytes);
        for &s in samples {
            let v = (s >> shift) as u32; // two's complement, low nbytes are the sample
            buf.extend_from_slice(&v.to_le_bytes()[..nbytes]);
        }
        self.data_bytes += buf.len() as u64;
        self.file.write_all(&buf)
    }

    fn finish(mut self: Box<Self>) -> std::io::Result<()> {
        self.file.seek(SeekFrom::Start(4))?;
        self.file
            .write_all(&((36 + self.data_bytes) as u32).to_le_bytes())?;
        self.file.seek(SeekFrom::Start(40))?;
        self.file
            .write_all(&(self.data_bytes as u32).to_le_bytes())?;
        self.file.flush()
    }
}

struct FlacSink {
    path: PathBuf,
    bits: u8,
    enc: rusty_flac::Encoder,
}

impl FlacSink {
    fn create(path: &Path, params: &PcmParams, channels: u32) -> std::io::Result<Self> {
        if channels > 8 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("FLAC encoder supports at most 8 channels (got {channels})"),
            ));
        }
        let enc = rusty_flac::Encoder::new(params.rate, channels, params.bits as u32)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, format!("{e}")))?;
        Ok(FlacSink {
            path: path.to_path_buf(),
            bits: params.bits,
            enc,
        })
    }
}

impl PcmSink for FlacSink {
    fn push(&mut self, samples: &[i32]) -> std::io::Result<()> {
        // shift full-scale i32 down to the target bit depth
        let shift = 32 - self.bits;
        let v: Vec<i32> = samples.iter().map(|s| s >> shift).collect();
        self.enc
            .push_interleaved(&v)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, format!("{e}")))?;
        Ok(())
    }

    fn finish(self: Box<Self>) -> std::io::Result<()> {
        let bytes = self.enc.finish();
        std::fs::write(&self.path, bytes)
    }
}

struct WvSink {
    path: PathBuf,
    params: PcmParams,
    channels: u32,
    buf: Vec<i32>,
}

impl WvSink {
    fn create(path: &Path, params: PcmParams, channels: u32) -> std::io::Result<Self> {
        if channels > 2 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!(
                    "WavPack output supports at most 2 channels (wavicle encoder limitation, got {channels}); use wav or flac for multichannel"
                ),
            ));
        }
        Ok(WvSink {
            path: path.to_path_buf(),
            params,
            channels,
            buf: Vec::new(),
        })
    }
}

impl PcmSink for WvSink {
    fn push(&mut self, samples: &[i32]) -> std::io::Result<()> {
        let shift = 32 - self.params.bits;
        self.buf.extend(samples.iter().map(|s| s >> shift));
        Ok(())
    }

    fn finish(self: Box<Self>) -> std::io::Result<()> {
        let params = wavicle::EncodeParams {
            channels: self.channels,
            sample_rate: self.params.rate,
            bits_per_sample: self.params.bits as u32,
        };
        let bytes = wavicle::encode_int(params, &self.buf)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, format!("{e}")))?;
        std::fs::write(&self.path, bytes)
    }
}

fn make_sink(
    path: &Path,
    format: PcmFormat,
    params: &PcmParams,
    channels: u32,
) -> std::io::Result<Box<dyn PcmSink>> {
    match format {
        PcmFormat::Wav => Ok(Box::new(WavSink::create(path, params, channels)?)),
        PcmFormat::Flac => Ok(Box::new(FlacSink::create(path, params, channels)?)),
        PcmFormat::Wv => Ok(Box::new(WvSink::create(path, *params, channels)?)),
    }
}

// ---------- top-level entry points ----------

fn sanitize_name(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == ' ' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Convert a single DSD file (DSF/DFF/DoP-WAV) to `out` (format from the
/// output extension, or `format`). Returns output frames.
pub fn convert_file(
    input: &Path,
    out: &Path,
    format: Option<PcmFormat>,
    params: &PcmParams,
) -> Result<u64> {
    let ext = input
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let info = match ext.as_str() {
        "dff" => crate::dff::parse(input)?,
        "wav" => crate::dop::parse(input)?,
        _ => crate::dsf::parse(input)?,
    };
    let fmt = match out
        .extension()
        .and_then(|e| e.to_str())
        .and_then(PcmFormat::from_ext)
    {
        Some(f) => f,
        None => format.ok_or_else(|| SacdError::NotDop {
            path: out.to_path_buf(),
            msg: "unknown output extension (use .wav/.flac/.wv or -f)".to_string(),
        })?,
    };
    convert_dsf_info(&info, out, fmt, params)
}

/// Convert a DsfInfo (any packing) to a PCM file. Returns output frames.
pub fn convert_dsf_info(
    info: &DsfInfo,
    out: &Path,
    fmt: PcmFormat,
    params: &PcmParams,
) -> Result<u64> {
    let io = |e: std::io::Error| SacdError::Io {
        path: out.to_path_buf(),
        source: e,
    };
    let mut reader = DsdBlockReader::open(info).map_err(|e| SacdError::Io {
        path: info.path.clone(),
        source: e,
    })?;
    let mut sink = make_sink(out, fmt, params, info.channels).map_err(io)?;
    let frames = convert_blocks(
        || reader.next_block(),
        !info.bit_reverse_needed,
        info.sample_rate,
        info.channels as usize,
        params,
        |s| sink.push(s).unwrap(),
    )
    .map_err(io)?;
    sink.finish().map_err(io)?;
    Ok(frames)
}

/// Convert one ISO track. `IsoTrack.data` is already in DSF layout
/// ([L 4096][R 4096] per 8192-byte block pair, LSB-first bit order).
fn convert_iso_track(
    track: &IsoTrack,
    out: &Path,
    fmt: PcmFormat,
    params: &PcmParams,
) -> Result<u64> {
    let data = track.data.clone();
    let mut pos = 0usize;
    let mut sink = make_sink(out, fmt, params, 2).map_err(|e| SacdError::Io {
        path: out.to_path_buf(),
        source: e,
    })?;
    let frames = convert_blocks(
        || {
            if pos >= data.len() {
                return Ok(None);
            }
            let mut chunk = [0u8; SECTOR_PAIR_BYTES];
            let n = SECTOR_PAIR_BYTES.min(data.len() - pos);
            chunk[..n].copy_from_slice(&data[pos..pos + n]);
            pos += n;
            // de-interleave the DSF block pair into L0 R0 L1 R1 ...
            let mut pair = [0u8; SECTOR_PAIR_BYTES];
            for i in 0..SECTOR_PAIR_BYTES / 2 {
                pair[2 * i] = chunk[i];
                pair[2 * i + 1] = chunk[SECTOR_PAIR_BYTES / 2 + i];
            }
            Ok(Some(pair.to_vec()))
        },
        false, // DSF bit order = LSB first
        DSD64,
        2, // the 2C (stereo) audio area of a SACD-R
        params,
        |s| sink.push(s).unwrap(),
    )
    .map_err(|e| SacdError::Io {
        path: out.to_path_buf(),
        source: e,
    })?;
    sink.finish().map_err(|e| SacdError::Io {
        path: out.to_path_buf(),
        source: e,
    })?;
    Ok(frames)
}

/// Convert a SACD-R ISO: one PCM file per track into directory `out_dir`.
/// Returns total output frames.
pub fn convert_iso(iso: &Path, out_dir: &Path, fmt: PcmFormat, params: &PcmParams) -> Result<u64> {
    let img = iso_read::read_iso(iso).map_err(|e| SacdError::Io {
        path: iso.to_path_buf(),
        source: e,
    })?;
    std::fs::create_dir_all(out_dir).map_err(|e| SacdError::Io {
        path: out_dir.to_path_buf(),
        source: e,
    })?;
    let ext = fmt.ext();
    let mut total = 0u64;
    for (i, track) in img.tracks.iter().enumerate() {
        let name = track
            .title
            .clone()
            .unwrap_or_else(|| format!("track{:02}", i + 1));
        let p = out_dir.join(format!("{:02} {}.{}", i + 1, sanitize_name(&name), ext));
        total += convert_iso_track(track, &p, fmt, params)?;
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{make_test_dff, make_test_dop, pattern};
    use std::io::Write;

    fn test_params() -> PcmParams {
        PcmParams {
            rate: 96_000,
            bits: 24,
            lowpass: None,
            gain_db: 0.0,
        }
    }

    fn collect(info: &DsfInfo, params: &PcmParams) -> (Vec<i32>, u64) {
        let mut reader = DsdBlockReader::open(info).unwrap();
        let mut v = Vec::new();
        let frames = convert_blocks(
            || reader.next_block(),
            !info.bit_reverse_needed,
            info.sample_rate,
            info.channels as usize,
            params,
            |s| v.extend_from_slice(s),
        )
        .unwrap();
        (v, frames)
    }

    /// Build a DSF whose data chunk is `data` (block-interleaved L/R blocks).
    pub(super) fn dsf_with_data(data: &[u8]) -> (tempfile::NamedTempFile, DsfInfo) {
        let samples = (data.len() / 2) as u64 * 8;
        let mut bytes = crate::testutil::make_test_dsf(samples, false);
        let off = 92;
        bytes[off..off + data.len()].copy_from_slice(data);
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(&bytes).unwrap();
        let info = crate::dsf::parse_bytes(&bytes, f.path()).unwrap();
        (f, info)
    }

    #[test]
    fn dsf_dop_dff_produce_identical_pcm() {
        let data = pattern(2 * 8192, 0xC0FFEE); // 2 block pairs
        let (_f, dsf_info) = dsf_with_data(&data);

        // equivalent DoP: byte-interleave L/R of the DSF block layout
        let mut interleaved = Vec::with_capacity(data.len());
        for block in 0..data.len() / 8192 {
            let base = block * 8192;
            for i in 0..4096 {
                interleaved.push(data[base + i]); // L
                interleaved.push(data[base + 4096 + i]); // R
            }
        }
        let dop_bytes = make_test_dop(&interleaved, None);
        let mut dop_file = tempfile::NamedTempFile::new().unwrap();
        dop_file.write_all(&dop_bytes).unwrap();
        let dop_info = crate::dop::parse_bytes(&dop_bytes, dop_file.path()).unwrap();

        // equivalent DFF: same stream in native (bit-reversed) order
        let native: Vec<u8> = data.iter().map(|b| b.reverse_bits()).collect();
        let dff_bytes = make_test_dff(&native, 2, None);
        let mut dff_file = tempfile::NamedTempFile::new().unwrap();
        dff_file.write_all(&dff_bytes).unwrap();
        let dff_info = crate::dff::parse_bytes(&dff_bytes, dff_file.path()).unwrap();

        let params = test_params();
        let (a, fa) = collect(&dsf_info, &params);
        let (b, fb) = collect(&dop_info, &params);
        let (c, fc) = collect(&dff_info, &params);
        assert_eq!(fa, fb);
        assert_eq!(fa, fc);
        assert_eq!(a, b, "DSF vs DoP PCM mismatch");
        assert_eq!(a, c, "DSF vs DFF PCM mismatch");
        assert!(!a.is_empty());
        // 2 block pairs = 65536 samples/ch -> 65536*96000/2822400 = 2228 frames (+ tail)
        assert!((2228..2228 + 3000).contains(&fa), "frames {fa}");
    }

    pub(super) fn parse_wav(path: &Path) -> (u32, u16, u16, u16, Vec<u8>) {
        let b = std::fs::read(path).unwrap();
        assert_eq!(&b[0..4], b"RIFF");
        assert_eq!(&b[8..12], b"WAVE");
        assert_eq!(&b[12..16], b"fmt ");
        let tag = u16::from_le_bytes([b[20], b[21]]);
        let ch = u16::from_le_bytes([b[22], b[23]]);
        let rate = u32::from_le_bytes([b[24], b[25], b[26], b[27]]);
        let bits = u16::from_le_bytes([b[34], b[35]]);
        assert_eq!(&b[36..40], b"data");
        let data_size = u32::from_le_bytes([b[40], b[41], b[42], b[43]]) as usize;
        let riff_size = u32::from_le_bytes([b[4], b[5], b[6], b[7]]) as usize;
        assert_eq!(riff_size, 36 + data_size);
        assert_eq!(b.len(), 44 + data_size);
        (rate, ch, bits, tag, b[44..].to_vec())
    }

    #[test]
    fn wav_output_is_well_formed() {
        let data = pattern(8192, 42);
        let (_f, info) = dsf_with_data(&data);
        let out = tempfile::NamedTempFile::new().unwrap();
        let out_path = out.path().to_path_buf();
        let params = test_params();
        let frames = convert_dsf_info(&info, &out_path, PcmFormat::Wav, &params).unwrap();
        let (rate, ch, bits, tag, pcm) = parse_wav(&out_path);
        assert_eq!(tag, 1);
        assert_eq!(ch, 2);
        assert_eq!(rate, 96_000);
        assert_eq!(bits, 24);
        assert_eq!(pcm.len(), frames as usize * 2 * 3);
        // 8192 bytes = 32768 samples/ch -> 1114 frames at 96k (+ filter tail)
        assert!((1114..1114 + 2000).contains(&frames), "frames {frames}");
    }

    #[test]
    fn flac_output_roundtrips_rusty_and_claxon() {
        let data = pattern(8192, 7);
        let (_f, info) = dsf_with_data(&data);
        let out = tempfile::NamedTempFile::new().unwrap();
        let out_path = out.path().to_path_buf();
        let params = test_params();
        let frames = convert_dsf_info(&info, &out_path, PcmFormat::Flac, &params).unwrap();

        let bytes = std::fs::read(&out_path).unwrap();
        assert_eq!(&bytes[0..4], b"fLaC");
        // rusty_flac decode (planar)
        let (si, planes) = rusty_flac::decode(&bytes).unwrap();
        assert_eq!(si.sample_rate, 96_000);
        assert_eq!(si.channels, 2);
        assert_eq!(si.bits_per_sample, 24);
        let n = planes[0].len();
        assert!((n as i64 - frames as i64).abs() < 64);
        let mut interleaved = Vec::with_capacity(n * 2);
        for (l, r) in planes[0].iter().zip(&planes[1]) {
            interleaved.push(*l);
            interleaved.push(*r);
        }
        // claxon cross-decode (independent implementation)
        let mut reader = claxon::FlacReader::open(&out_path).unwrap();
        let claxon_samples: Vec<i32> = reader.samples().map(|s| s.unwrap()).collect();
        assert_eq!(claxon_samples.len(), interleaved.len());
        assert_eq!(claxon_samples, interleaved, "claxon decode mismatch");
    }

    #[test]
    fn wavpack_output_roundtrips_16bit() {
        let data = pattern(8192, 99);
        let (_f, info) = dsf_with_data(&data);
        let out = tempfile::NamedTempFile::new().unwrap();
        let out_path = out.path().to_path_buf();
        let params = PcmParams {
            rate: 48_000,
            bits: 16,
            lowpass: None,
            gain_db: 0.0,
        };
        let frames = convert_dsf_info(&info, &out_path, PcmFormat::Wv, &params).unwrap();
        let bytes = std::fs::read(&out_path).unwrap();
        assert_eq!(&bytes[0..4], b"wvpk");
        let dec = wavicle::decode_stream(&bytes).unwrap();
        assert_eq!(dec.sample_rate, 48_000);
        assert_eq!(dec.channels, 2);
        assert_eq!(dec.bits_per_sample, 16);
        assert!((dec.samples.len() as i64 / 2 - frames as i64).abs() < 64);
    }

    /// Build a multichannel DFF with `blocks` 4096-byte blocks per channel.
    fn dff_with_channels(channels: u16, blocks: usize) -> (tempfile::NamedTempFile, DsfInfo) {
        let audio = pattern(4096 * channels as usize * blocks, 0x5A5A);
        let bytes = make_test_dff(&audio, channels, Some("mc test"));
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(&bytes).unwrap();
        let info = crate::dff::parse_bytes(&bytes, f.path()).unwrap();
        (f, info)
    }

    #[test]
    fn multichannel_dff_to_wav() {
        let (_f, info) = dff_with_channels(4, 2);
        assert_eq!(info.channels, 4);
        let out = tempfile::NamedTempFile::new().unwrap();
        let out_path = out.path().to_path_buf();
        let params = test_params();
        let frames = convert_dsf_info(&info, &out_path, PcmFormat::Wav, &params).unwrap();
        let (rate, ch, bits, tag, pcm) = parse_wav(&out_path);
        assert_eq!((tag, ch, rate, bits), (1, 4, 96_000, 24));
        assert_eq!(pcm.len(), frames as usize * 4 * 3);
        // 2 blocks * 4096 bytes * 8 = 65536 samples/ch -> 2228 frames at 96k
        assert!((2228..2228 + 3000).contains(&frames), "frames {frames}");
    }

    #[test]
    fn multichannel_dff_to_flac() {
        let (_f, info) = dff_with_channels(6, 1);
        assert_eq!(info.channels, 6);
        let out = tempfile::NamedTempFile::new().unwrap();
        let out_path = out.path().to_path_buf();
        let params = test_params();
        let frames = convert_dsf_info(&info, &out_path, PcmFormat::Flac, &params).unwrap();
        let bytes = std::fs::read(&out_path).unwrap();
        assert_eq!(&bytes[0..4], b"fLaC");
        let (si, planes) = rusty_flac::decode(&bytes).unwrap();
        assert_eq!(si.channels, 6);
        assert_eq!(si.sample_rate, 96_000);
        assert_eq!(si.bits_per_sample, 24);
        assert_eq!(planes.len(), 6);
        assert!((planes[0].len() as i64 - frames as i64).abs() < 64);
        let mut reader = claxon::FlacReader::open(&out_path).unwrap();
        assert_eq!(reader.streaminfo().channels, 6);
        assert_eq!(reader.samples().count(), planes[0].len() * 6);
    }

    #[test]
    fn multichannel_wavpack_is_rejected() {
        let (_f, info) = dff_with_channels(4, 1);
        let out = tempfile::NamedTempFile::new().unwrap();
        let out_path = out.path().to_path_buf();
        let params = test_params();
        let err = convert_dsf_info(&info, &out_path, PcmFormat::Wv, &params).unwrap_err();
        assert!(
            err.to_string().contains("2 channels"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn multichannel_wav_and_flac_agree() {
        let (_f, info) = dff_with_channels(4, 1);
        let params = test_params();
        let wav_out = tempfile::NamedTempFile::new().unwrap();
        let wav_path = wav_out.path().to_path_buf();
        let frames_wav = convert_dsf_info(&info, &wav_path, PcmFormat::Wav, &params).unwrap();
        let (_r, _c, _b, _t, pcm_wav) = parse_wav(&wav_path);
        let flac_out = tempfile::NamedTempFile::new().unwrap();
        let flac_path = flac_out.path().to_path_buf();
        let frames_flac = convert_dsf_info(&info, &flac_path, PcmFormat::Flac, &params).unwrap();
        let bytes = std::fs::read(&flac_path).unwrap();
        let (_si, planes) = rusty_flac::decode(&bytes).unwrap();
        assert_eq!(frames_wav, frames_flac);
        let n = planes[0].len().min(frames_wav as usize);
        for i in 0..n {
            for c in 0..4 {
                let w = u32::from_le_bytes([
                    pcm_wav[(i * 4 + c) * 3],
                    pcm_wav[(i * 4 + c) * 3 + 1],
                    pcm_wav[(i * 4 + c) * 3 + 2],
                    0,
                ]);
                let ws = if w & 0x80_0000 != 0 {
                    w as i32 - 0x100_0000
                } else {
                    w as i32
                };
                assert_eq!(ws, planes[c][i], "sample mismatch at frame {i} ch {c}");
            }
        }
    }

    #[test]
    fn iso_input_matches_dsf_input() {
        let data = pattern(8192, 1234);
        let (dsf_file, info) = dsf_with_data(&data);
        let params = test_params();
        let (direct, frames_direct) = collect(&info, &params);

        let iso_file = tempfile::NamedTempFile::new().unwrap();
        crate::image::build_image(std::slice::from_ref(&info), iso_file.path()).unwrap();
        let out_dir = tempfile::tempdir().unwrap();
        let frames_iso =
            convert_iso(iso_file.path(), out_dir.path(), PcmFormat::Wav, &params).unwrap();
        // the ISO track is padded to a whole SACD frame (9408 > 8192 bytes), so
        // it may emit a few more (zero-padding ring-out) frames than the DSF
        assert!(frames_iso >= frames_direct);
        let entries: Vec<_> = std::fs::read_dir(out_dir.path())
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        assert_eq!(entries.len(), 1);
        let (_rate, _ch, _bits, _tag, pcm) = parse_wav(&entries[0]);
        // direct sink produced full-scale i32; WAV stores 24-bit (>>8)
        let direct24: Vec<u8> = direct
            .iter()
            .flat_map(|s| {
                let v = (*s >> 8) as u32 & 0xFF_FFFF;
                v.to_le_bytes()[..3].to_vec()
            })
            .collect();
        let n = (frames_direct as usize) * 6; // compare the common prefix
        assert_eq!(&pcm[..n], &direct24[..n], "ISO vs DSF PCM prefix mismatch");
        drop(dsf_file);
    }
}

#[cfg(test)]
mod limits_tests {
    use super::tests::{dsf_with_data, parse_wav};
    use super::*;
    use crate::testutil::pattern;

    #[test]
    fn format_limits_are_enforced() {
        let p = |rate, bits| PcmParams {
            rate,
            bits,
            lowpass: None,
            gain_db: 0.0,
        };
        // WAV: 32 bit / 384 kHz allowed
        assert!(validate_params(PcmFormat::Wav, &p(384_000, 32)).is_ok());
        assert!(validate_params(PcmFormat::Wav, &p(44_100, 16)).is_ok());
        // FLAC: capped at 24 bit / 192 kHz
        assert!(validate_params(PcmFormat::Flac, &p(192_000, 24)).is_ok());
        assert!(validate_params(PcmFormat::Flac, &p(384_000, 24)).is_err());
        assert!(validate_params(PcmFormat::Flac, &p(96_000, 32)).is_err());
        // WavPack: 32 bit / 384 kHz allowed
        assert!(validate_params(PcmFormat::Wv, &p(384_000, 32)).is_ok());
        // bogus values rejected everywhere
        assert!(validate_params(PcmFormat::Wav, &p(12345, 24)).is_err());
        assert!(validate_params(PcmFormat::Wav, &p(96_000, 8)).is_err());
    }

    #[test]
    fn wav_32bit_384k() {
        let data = pattern(8192, 555);
        let (_f, info) = dsf_with_data(&data);
        let out = tempfile::NamedTempFile::new().unwrap();
        let out_path = out.path().to_path_buf();
        let params = PcmParams {
            rate: 384_000,
            bits: 32,
            lowpass: None,
            gain_db: 0.0,
        };
        let frames = convert_dsf_info(&info, &out_path, PcmFormat::Wav, &params).unwrap();
        let (rate, ch, bits, tag, pcm) = parse_wav(&out_path);
        assert_eq!((tag, ch, rate, bits), (1, 2, 384_000, 32));
        assert_eq!(pcm.len(), frames as usize * 2 * 4);
        // 32768 samples/ch -> 32768*384000/2822400 = 4457 frames (+ tail)
        assert!((4457..4457 + 3000).contains(&frames), "frames {frames}");
    }

    #[test]
    fn flac_24bit_192k() {
        let data = pattern(8192, 777);
        let (_f, info) = dsf_with_data(&data);
        let out = tempfile::NamedTempFile::new().unwrap();
        let out_path = out.path().to_path_buf();
        let params = PcmParams {
            rate: 192_000,
            bits: 24,
            lowpass: None,
            gain_db: 0.0,
        };
        let frames = convert_dsf_info(&info, &out_path, PcmFormat::Flac, &params).unwrap();
        let bytes = std::fs::read(&out_path).unwrap();
        let (si, planes) = rusty_flac::decode(&bytes).unwrap();
        assert_eq!(
            (si.sample_rate, si.channels, si.bits_per_sample),
            (192_000, 2, 24)
        );
        assert!((planes[0].len() as i64 - frames as i64).abs() < 64);
        // cross-check with claxon
        let mut reader = claxon::FlacReader::open(&out_path).unwrap();
        let si2 = reader.streaminfo();
        assert_eq!(
            (si2.sample_rate, si2.channels, si2.bits_per_sample),
            (192_000, 2, 24)
        );
        assert_eq!(reader.samples().count(), planes[0].len() * 2);
    }
}
