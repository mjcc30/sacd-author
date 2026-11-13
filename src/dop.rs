//! DoP ("WAV-DSD", DSD over PCM) parser: a RIFF/WAVE container carrying a raw
//! DSD stream inside 24-bit PCM frames, per the DoP Open Standard v1.1.
//!
//! Container layout (little-endian):
//! - 0x00  4  "RIFF"
//! - 0x04  4  u32 riff chunk size
//! - 0x08  4  "WAVE"
//! - then chunks: id[4] + u32 LE size + body (padded to an even offset)
//!   - "fmt " (>= 16 bytes): u16 format tag, u16 channels, u32 sample rate,
//!     u32 byte rate, u16 block align, u16 bits per sample. DoP uses tag 3
//!     (IEEE float) or 0xFFFE (extensible) with the IEEE-float subformat GUID,
//!     24 bits per sample.
//!   - "data": the DoP payload.
//!   - "LIST" with type "INFO": INAM (title), IART (artist), IPRD (album).
//!   - "id3 ": an ID3v2 tag.
//!
//! DoP payload: one 24-bit PCM frame per channel per sample. The 8 most
//! significant bits of each frame are a marker alternating 0x05/0xFA between
//! samples (identical on all channels of a frame); the lower 16 bits carry
//! 2 DSD bytes, oldest bit first — i.e. the same LSB-first bit order as DSF,
//! so like DSF the bytes must be bit-reversed for SACD (unlike DFF).
//! DSD64 rides in a 176.4 kHz carrier (DSD rate = carrier rate x 16).
//!
//! In the little-endian WAV storage each 24-bit sample is 3 bytes
//! [dsd_lo, dsd_hi, marker]; stereo frames interleave the channels:
//! [L lo, L hi, L mk, R lo, R hi, R mk] = 6 bytes per sample frame.

use crate::dsf::{AudioPacking, BLOCK_SIZE, DSD64_SAMPLE_RATE, DsfInfo};
use crate::error::{Result, SacdError};
use crate::id3::{self, Id3Tags};
use std::fs::File;
use std::io::{Cursor, Read, Seek, SeekFrom};
use std::path::Path;

pub const DOP_MARKER_A: u8 = 0x05;
pub const DOP_MARKER_B: u8 = 0xFA;
/// DSD64 carrier rate: DSD rate = carrier x 16 = 2822400 Hz.
pub const DOP_CARRIER_DSD64: u32 = 176_400;
/// Bytes per stereo sample frame in the DoP payload: 2 ch x 3 bytes.
pub const BYTES_PER_SAMPLE_FRAME: u64 = 6;
/// DSD bytes per channel per 4096-byte block pair = 2048 samples x 2 bytes.
pub const SAMPLES_PER_BLOCK_PAIR: usize = 2048;
/// DoP payload bytes unpacking to one 8192-byte block pair (4096 B/channel).
pub const DOP_GROUP_BYTES: usize = SAMPLES_PER_BLOCK_PAIR * BYTES_PER_SAMPLE_FRAME as usize;

const IEEE_FLOAT_GUID: [u8; 16] = [
    0x03, 0x00, 0x00, 0x00, 0x00, 0x00, 0x10, 0x00, 0x80, 0x00, 0x00, 0xAA, 0x00, 0x38, 0x9B, 0x71,
];

fn is_marker(b: u8) -> bool {
    b == DOP_MARKER_A || b == DOP_MARKER_B
}

struct WavFmt {
    tag: u16,
    channels: u16,
    sample_rate: u32,
    bits: u16,
    extensible_float: bool,
}

fn read_up_to<R: Read>(r: &mut R, n: u64) -> std::io::Result<Vec<u8>> {
    let mut buf = vec![0u8; n as usize];
    let mut read = 0usize;
    while read < buf.len() {
        match r.read(&mut buf[read..])? {
            0 => break,
            n => read += n,
        }
    }
    buf.truncate(read);
    Ok(buf)
}

fn parse_info_list(body: &[u8], tags: &mut Id3Tags) {
    if body.len() < 4 || &body[0..4] != b"INFO" {
        return;
    }
    let mut q = 4usize;
    while q + 8 <= body.len() {
        let sid = &body[q..q + 4];
        let ssize =
            u32::from_le_bytes(body[q + 4..q + 8].try_into().expect("len checked")) as usize;
        let start = q + 8;
        let end = (start + ssize).min(body.len());
        if start <= end {
            let val = &body[start..end];
            let nul = val.iter().position(|&b| b == 0).unwrap_or(val.len());
            let text = String::from_utf8_lossy(&val[..nul]).to_string();
            match sid {
                b"INAM" => tags.title = Some(text),
                b"IART" => tags.artist = Some(text),
                b"IPRD" => tags.album = Some(text),
                _ => {}
            }
        }
        q = start + ssize + (ssize % 2);
    }
}

fn merge_tags(tags: &mut Id3Tags, other: Id3Tags) {
    if tags.title.is_none() {
        tags.title = other.title;
    }
    if tags.artist.is_none() {
        tags.artist = other.artist;
    }
    if tags.album.is_none() {
        tags.album = other.album;
    }
}

/// Walk the RIFF chunks of `r` (length `len`) and build the track info.
fn parse_stream<R: Read + Seek>(r: &mut R, len: u64, path: &Path) -> Result<DsfInfo> {
    let io = |source: std::io::Error| SacdError::Io {
        path: path.to_path_buf(),
        source,
    };
    let not_dop = |msg: String| SacdError::NotDop {
        path: path.to_path_buf(),
        msg,
    };

    let mut head = [0u8; 12];
    r.read_exact(&mut head).map_err(io)?;
    if &head[0..4] != b"RIFF" || &head[8..12] != b"WAVE" {
        return Err(not_dop("not a RIFF/WAVE file".to_string()));
    }

    let mut fmt: Option<WavFmt> = None;
    let mut data_offset = 0u64;
    let mut data_size = 0u64;
    let mut tags = Id3Tags::default();

    let mut pos = 12u64;
    while pos + 8 <= len {
        r.seek(SeekFrom::Start(pos)).map_err(io)?;
        let mut ch = [0u8; 8];
        if r.read_exact(&mut ch).is_err() {
            break;
        }
        let id = &ch[0..4];
        let size = u32::from_le_bytes(ch[4..8].try_into().expect("len checked")) as u64;
        let body = pos + 8;
        match id {
            b"fmt " => {
                let buf = read_up_to(r, size.min(1024)).map_err(io)?;
                if buf.len() >= 16 {
                    let tag = u16::from_le_bytes([buf[0], buf[1]]);
                    let channels = u16::from_le_bytes([buf[2], buf[3]]);
                    let sample_rate = u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]);
                    let bits = u16::from_le_bytes([buf[14], buf[15]]);
                    let extensible_float =
                        tag == 0xFFFE && buf.len() >= 40 && buf[24..40] == IEEE_FLOAT_GUID;
                    fmt = Some(WavFmt {
                        tag,
                        channels,
                        sample_rate,
                        bits,
                        extensible_float,
                    });
                }
            }
            b"LIST" => {
                let buf = read_up_to(r, size.min(256 * 1024)).map_err(io)?;
                parse_info_list(&buf, &mut tags);
            }
            b"id3 " => {
                let buf = read_up_to(r, size.min(1024 * 1024)).map_err(io)?;
                if let Ok(Some(t)) = id3::parse_id3_tail(&buf, path) {
                    merge_tags(&mut tags, t);
                }
            }
            b"data" => {
                if body >= len {
                    return Err(SacdError::MissingDataChunk {
                        path: path.to_path_buf(),
                    });
                }
                data_offset = body;
                data_size = size.min(len - body);
                break;
            }
            _ => {}
        }
        pos = body + size + (size % 2);
    }

    let fmt = fmt.ok_or_else(|| not_dop("missing 'fmt ' chunk".to_string()))?;
    if data_offset == 0 {
        return Err(SacdError::MissingDataChunk {
            path: path.to_path_buf(),
        });
    }
    if !(fmt.tag == 3 || (fmt.tag == 0xFFFE && fmt.extensible_float)) {
        return Err(not_dop(format!(
            "format tag {:#06x} is not an IEEE-float carrier (plain PCM WAV is not DSD)",
            fmt.tag
        )));
    }
    if fmt.bits != 24 {
        return Err(not_dop(format!(
            "bits per sample {} != 24 (DoP needs a 24-bit carrier)",
            fmt.bits
        )));
    }
    if fmt.channels != 2 {
        return Err(SacdError::UnsupportedChannels {
            path: path.to_path_buf(),
            channels: fmt.channels as u32,
        });
    }
    let dsd_rate = fmt.sample_rate as u64 * 16;
    if dsd_rate != DSD64_SAMPLE_RATE as u64 {
        return Err(SacdError::UnsupportedSampleRate {
            path: path.to_path_buf(),
            rate: dsd_rate as u32,
        });
    }

    // Sniff the DoP markers in the first sample frames (the standard requires
    // 32 consecutive marker bytes per channel to lock onto DSD mode).
    r.seek(SeekFrom::Start(data_offset)).map_err(io)?;
    let probe = read_up_to(r, 64 * BYTES_PER_SAMPLE_FRAME).map_err(io)?;
    let groups = probe.len() / BYTES_PER_SAMPLE_FRAME as usize;
    if groups < 32 {
        return Err(not_dop(
            "data chunk too short to contain DoP markers".to_string(),
        ));
    }
    for g in 0..groups {
        let b = &probe[g * 6..g * 6 + 6];
        if !is_marker(b[2]) || !is_marker(b[5]) {
            return Err(not_dop(
                "data chunk lacks DoP markers 0x05/0xFA (not a WAV-DSD file)".to_string(),
            ));
        }
    }

    let pcm_samples = data_size / BYTES_PER_SAMPLE_FRAME;
    let dsd_bytes_per_channel = pcm_samples * 2;
    let pairs = dsd_bytes_per_channel.div_ceil(BLOCK_SIZE as u64);
    Ok(DsfInfo {
        path: path.to_path_buf(),
        sample_rate: DSD64_SAMPLE_RATE,
        channels: 2,
        samples_per_channel: dsd_bytes_per_channel * 8,
        block_size: BLOCK_SIZE,
        data_offset,
        // Virtual unpacked size in whole block pairs, so stream_frames()
        // computes the same frame count as for a DSF of the same length.
        data_size: pairs * 2 * BLOCK_SIZE as u64,
        tags,
        bit_reverse_needed: true,
        packing: AudioPacking::Dop,
    })
}

/// Parse a DoP WAV from an in-memory buffer (used by tests and by `parse`).
pub fn parse_bytes(data: &[u8], path: &Path) -> Result<DsfInfo> {
    let mut cursor = Cursor::new(data);
    parse_stream(&mut cursor, data.len() as u64, path)
}

/// Parse a DoP WAV file from disk (streams the chunks; never loads the audio).
pub fn parse(path: &Path) -> Result<DsfInfo> {
    let mut file = File::open(path).map_err(|source| SacdError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let len = file
        .metadata()
        .map_err(|source| SacdError::Io {
            path: path.to_path_buf(),
            source,
        })?
        .len();
    parse_stream(&mut file, len, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn chunk(id: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let mut c = id.to_vec();
        c.extend_from_slice(&(body.len() as u32).to_le_bytes());
        c.extend_from_slice(body);
        if body.len() % 2 == 1 {
            c.push(0);
        }
        c
    }

    fn fmt_chunk(tag: u16, channels: u16, rate: u32, bits: u16, extensible: bool) -> Vec<u8> {
        let mut fmt = Vec::new();
        fmt.extend_from_slice(&tag.to_le_bytes());
        fmt.extend_from_slice(&channels.to_le_bytes());
        fmt.extend_from_slice(&rate.to_le_bytes());
        fmt.extend_from_slice(&(rate * channels as u32 * bits as u32 / 8).to_le_bytes());
        fmt.extend_from_slice(&(channels * bits / 8).to_le_bytes());
        fmt.extend_from_slice(&bits.to_le_bytes());
        if extensible {
            fmt.extend_from_slice(&22u16.to_le_bytes()); // cbSize
            fmt.extend_from_slice(&bits.to_le_bytes()); // valid bits
            fmt.extend_from_slice(&0x3u32.to_le_bytes()); // channel mask L/R
            fmt.extend_from_slice(&IEEE_FLOAT_GUID);
        }
        chunk(b"fmt ", &fmt)
    }

    /// Build a DoP WAV from a byte-interleaved stereo DSD stream
    /// ([L0 R0 L1 R1 ...], 2 bytes per channel per sample = 4 bytes/sample).
    pub fn make_test_dop(interleaved: &[u8], title: Option<&str>) -> Vec<u8> {
        assert_eq!(interleaved.len() % 4, 0);
        let samples = interleaved.len() / 4;
        let mut data = Vec::with_capacity(samples * 6);
        for s in 0..samples {
            let l0 = interleaved[4 * s];
            let l1 = interleaved[4 * s + 2];
            let r0 = interleaved[4 * s + 1];
            let r1 = interleaved[4 * s + 3];
            let mk = if s % 2 == 0 {
                DOP_MARKER_A
            } else {
                DOP_MARKER_B
            };
            data.extend_from_slice(&[l0, l1, mk, r0, r1, mk]);
        }
        let mut out = b"RIFF".to_vec();
        let riff_size_pos = out.len();
        out.extend_from_slice(&[0u8; 4]);
        out.extend_from_slice(b"WAVE");
        out.extend_from_slice(&fmt_chunk(3, 2, DOP_CARRIER_DSD64, 24, false));
        if let Some(t) = title {
            let mut inam = b"INAM".to_vec();
            inam.extend_from_slice(&((t.len() + 1) as u32).to_le_bytes());
            inam.extend_from_slice(t.as_bytes());
            inam.push(0);
            if (t.len() + 1) % 2 == 1 {
                inam.push(0);
            }
            let mut info = b"INFO".to_vec();
            info.extend_from_slice(&inam);
            out.extend_from_slice(&chunk(b"LIST", &info));
        }
        out.extend_from_slice(&chunk(b"data", &data));
        let size = (out.len() - 8) as u32;
        out[riff_size_pos..riff_size_pos + 4].copy_from_slice(&size.to_le_bytes());
        out
    }

    /// Deterministic pseudo-random byte pattern (xorshift64).
    fn pattern(n: usize, seed: u64) -> Vec<u8> {
        let mut x = seed;
        (0..n)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                (x >> 32) as u8
            })
            .collect()
    }

    /// Build a DSF whose data chunk is exactly [l_block][r_block] (one pair).
    fn make_dsf_pair(l: &[u8], r: &[u8]) -> Vec<u8> {
        assert_eq!(l.len(), 4096);
        assert_eq!(r.len(), 4096);
        let mut buf = Vec::new();
        buf.extend_from_slice(b"DSD ");
        let total_pos = buf.len();
        buf.extend_from_slice(&[0u8; 8]);
        buf.extend_from_slice(&28u64.to_le_bytes());
        buf.extend_from_slice(&0u64.to_le_bytes());
        buf.extend_from_slice(b"fmt ");
        buf.extend_from_slice(&52u64.to_le_bytes());
        buf.extend_from_slice(&1u32.to_le_bytes());
        buf.extend_from_slice(&0u32.to_le_bytes());
        buf.extend_from_slice(&2u32.to_le_bytes());
        buf.extend_from_slice(&2u32.to_le_bytes());
        buf.extend_from_slice(&DSD64_SAMPLE_RATE.to_le_bytes());
        buf.extend_from_slice(&1u32.to_le_bytes());
        buf.extend_from_slice(&32768u64.to_le_bytes());
        buf.extend_from_slice(&4096u32.to_le_bytes());
        buf.extend_from_slice(&0u32.to_le_bytes());
        buf.extend_from_slice(b"data");
        buf.extend_from_slice(&8192u64.to_le_bytes());
        buf.extend_from_slice(l);
        buf.extend_from_slice(r);
        let total = buf.len() as u64;
        buf[total_pos..total_pos + 8].copy_from_slice(&total.to_le_bytes());
        buf
    }

    #[test]
    fn parses_dop_header_and_info() {
        let path = Path::new("t.wav");
        let interleaved = pattern(4 * 2048, 0x1234);
        let wav = make_test_dop(&interleaved, Some("DoP Song"));
        let info = parse_bytes(&wav, path).unwrap();
        assert_eq!(info.sample_rate, DSD64_SAMPLE_RATE);
        assert_eq!(info.channels, 2);
        assert_eq!(info.samples_per_channel, 32768);
        assert_eq!(info.data_size, 8192);
        // 12 (RIFF) + 24 (fmt chunk) + 30 (LIST/INFO chunk) + 8 (data header)
        assert_eq!(info.data_offset, 74);
        assert!(info.bit_reverse_needed);
        assert_eq!(info.packing, AudioPacking::Dop);
        assert_eq!(info.tags.title.as_deref(), Some("DoP Song"));
    }

    #[test]
    fn accepts_extensible_float_fmt() {
        let path = Path::new("t.wav");
        let interleaved = pattern(4 * 2048, 7);
        let mut wav = make_test_dop(&interleaved, None);
        // swap the fmt chunk for an extensible one
        let fmt_ext = fmt_chunk(0xFFFE, 2, DOP_CARRIER_DSD64, 24, true);
        let data_pos = 12 + 8 + 16;
        let mut out = wav[..12].to_vec();
        out.extend_from_slice(&fmt_ext);
        out.extend_from_slice(&wav[data_pos..]);
        let size = (out.len() - 8) as u32;
        out[4..8].copy_from_slice(&size.to_le_bytes());
        wav = out;
        let info = parse_bytes(&wav, path).unwrap();
        assert_eq!(info.sample_rate, DSD64_SAMPLE_RATE);
    }

    #[test]
    fn rejects_pcm_wav() {
        let path = Path::new("pcm.wav");
        let interleaved = pattern(4 * 2048, 9);
        let mut wav = make_test_dop(&interleaved, None);
        // rewrite format tag at offset 20 (fmt body starts at 12+8=20)
        wav[20..22].copy_from_slice(&1u16.to_le_bytes());
        assert!(matches!(
            parse_bytes(&wav, path),
            Err(SacdError::NotDop { .. })
        ));
    }

    #[test]
    fn rejects_non_dsd64_carrier() {
        let path = Path::new("cd.wav");
        let interleaved = pattern(4 * 2048, 11);
        let mut wav = make_test_dop(&interleaved, None);
        wav[24..28].copy_from_slice(&44_100u32.to_le_bytes()); // fmt body +4
        assert!(matches!(
            parse_bytes(&wav, path),
            Err(SacdError::UnsupportedSampleRate { rate: 705_600, .. })
        ));
    }

    #[test]
    fn rejects_missing_markers() {
        let path = Path::new("fake.wav");
        let interleaved = pattern(4 * 2048, 13);
        let mut wav = make_test_dop(&interleaved, None);
        // corrupt the marker of the first sample frame (data starts at 12+24+8=44)
        wav[44 + 2] = 0x00;
        assert!(matches!(
            parse_bytes(&wav, path),
            Err(SacdError::NotDop { .. })
        ));
    }

    #[test]
    fn dop_and_dsf_produce_same_sacd_stream() {
        let l = pattern(4096, 0xAAAA);
        let r = pattern(4096, 0xBBBB);
        // byte-interleaved stream: L0 R0 L1 R1 ...
        let mut interleaved = Vec::with_capacity(8192);
        for i in 0..4096 {
            interleaved.push(l[i]);
            interleaved.push(r[i]);
        }
        let dsf_bytes = make_dsf_pair(&l, &r);
        let wav_bytes = make_test_dop(&interleaved, None);

        let mut dsf_file = tempfile::NamedTempFile::new().unwrap();
        dsf_file.write_all(&dsf_bytes).unwrap();
        let mut wav_file = tempfile::NamedTempFile::new().unwrap();
        wav_file.write_all(&wav_bytes).unwrap();

        let dsf_info = crate::dsf::parse_bytes(&dsf_bytes, dsf_file.path()).unwrap();
        let dop_info = parse_bytes(&wav_bytes, wav_file.path()).unwrap();
        assert_eq!(dsf_info.data_size, dop_info.data_size);
        assert_eq!(dsf_info.samples_per_channel, dop_info.samples_per_channel);

        let mut a = Vec::new();
        let mut b = Vec::new();
        crate::sacd::convert_track(&dsf_info, &mut a).unwrap();
        crate::sacd::convert_track(&dop_info, &mut b).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn dop_wav_audio_area_matches_dsf_iso() {
        use std::io::{Read, Seek, SeekFrom};
        let l = pattern(4096, 0xAAAA);
        let r = pattern(4096, 0xBBBB);
        let mut interleaved = Vec::with_capacity(8192);
        for i in 0..4096 {
            interleaved.push(l[i]);
            interleaved.push(r[i]);
        }
        let dsf_bytes = make_dsf_pair(&l, &r);
        let wav_bytes = make_test_dop(&interleaved, None);

        let mut dsf_file = tempfile::NamedTempFile::new().unwrap();
        dsf_file.write_all(&dsf_bytes).unwrap();
        let mut wav_file = tempfile::NamedTempFile::new().unwrap();
        wav_file.write_all(&wav_bytes).unwrap();

        let dsf_info = crate::dsf::parse_bytes(&dsf_bytes, dsf_file.path()).unwrap();
        let dop_info = parse_bytes(&wav_bytes, wav_file.path()).unwrap();

        let iso_a = tempfile::NamedTempFile::new().unwrap();
        let iso_b = tempfile::NamedTempFile::new().unwrap();
        crate::image::build_image(&[dsf_info], iso_a.path()).unwrap();
        crate::image::build_image(&[dop_info], iso_b.path()).unwrap();

        let audio_offset = crate::image::AUDIO_START_LBA as u64 * 2048;
        let mut a = Vec::new();
        let mut fa = std::fs::File::open(iso_a.path()).unwrap();
        fa.seek(SeekFrom::Start(audio_offset)).unwrap();
        fa.read_to_end(&mut a).unwrap();
        let mut b = Vec::new();
        let mut fb = std::fs::File::open(iso_b.path()).unwrap();
        fb.seek(SeekFrom::Start(audio_offset)).unwrap();
        fb.read_to_end(&mut b).unwrap();
        assert_eq!(a, b, "2C_Audio area differs between DSF and DoP inputs");
    }
}
