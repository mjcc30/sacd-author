//! DSF (DSD Stream File) parser, Little-Endian.
//!
//! Layout:
//! - 0x00  4  magic "DSD "
//! - 0x04  8  u64 total file size (includes this 28-byte header)
//! - 0x0C  8  u64 offset of "fmt " chunk
//! - 0x14  8  u64 offset of metadata chunk (0 if none)
//! - fmt chunk:
//!   - 0x00  4  magic "fmt "
//!   - 0x04  8  u64 chunk size (52)
//!   - 0x0C  4  u32 format version (1)
//!   - 0x10  4  u32 format id (0 = DSD raw)
//!   - 0x14  4  u32 channel type (2 = stereo)
//!   - 0x18  4  u32 channel count
//!   - 0x1C  4  u32 sampling frequency (2822400 = DSD64)
//!   - 0x20  4  u32 bits per sample (1)
//!   - 0x24  8  u64 samples per channel
//!   - 0x2C  4  u32 block size per channel (4096)
//!   - 0x30  4  u32 reserved
//! - data chunk:
//!   - 0x00  4  magic "data"
//!   - 0x04  8  u64 chunk size = n_blocks * block_size * channels
//!   - 0x0C  .. interleaved 4096-byte blocks, one per channel, per block index

use crate::error::{Result, SacdError};
use crate::id3::{self, Id3Tags};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

pub const DSD64_SAMPLE_RATE: u32 = 2_822_400;
pub const BLOCK_SIZE: u32 = 4096;

/// How the raw DSD stream is packed inside the input file's data area.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioPacking {
    /// DSF: interleaved 4096-byte blocks per channel ([L blk][R blk]...).
    Blocks,
    /// DFF (DSDIFF 1.5, "clustered frames"): channel bytes interleaved
    /// sample-wise (ch0[0], ch1[0], ..., ch0[1], ...), no 4096-byte blocks.
    Interleaved,
    /// DoP (WAV-DSD): 24-bit PCM frames, 3 bytes per channel per sample
    /// ([dsd_lo, dsd_hi, marker]), channels interleaved per frame.
    Dop,
}

#[derive(Debug, Clone)]
pub struct DsfInfo {
    pub path: PathBuf,
    pub sample_rate: u32,
    pub channels: u32,
    pub samples_per_channel: u64,
    pub block_size: u32,
    pub data_offset: u64,
    /// For `AudioPacking::Blocks`/`Interleaved`: bytes of the data chunk.
    /// For `AudioPacking::Dop`: virtual size of the unpacked stream, rounded
    /// up to whole 8192-byte block pairs (what `sacd::stream_frames` needs).
    pub data_size: u64,
    pub tags: Id3Tags,
    /// DSF and DoP store DSD bytes bit-reversed vs the SACD/DFF native order;
    /// DFF does not. The SACD converter bit-reverses only when this is true.
    pub bit_reverse_needed: bool,
    pub packing: AudioPacking,
}

impl DsfInfo {
    /// Duration in seconds (float).
    pub fn duration_secs(&self) -> f64 {
        self.samples_per_channel as f64 / self.sample_rate as f64
    }

    /// Number of interleaved 4096-byte blocks per channel.
    pub fn block_count(&self) -> u64 {
        self.samples_per_channel
            .div_ceil((self.block_size as u64) * 8)
    }

    /// SACD frames (75 per second) for this track, rounded up.
    pub fn sacd_frames(&self) -> u64 {
        self.samples_per_channel
            .div_ceil(self.sample_rate as u64 / 75)
    }
}

fn read_u32_le(buf: &[u8], off: usize) -> u32 {
    u32::from_le_bytes(buf[off..off + 4].try_into().expect("slice len checked"))
}

fn read_u64_le(buf: &[u8], off: usize) -> u64 {
    u64::from_le_bytes(buf[off..off + 8].try_into().expect("slice len checked"))
}

/// Locate the `fmt ` chunk: trust the header pointer if valid, otherwise
/// scan the first bytes (some ripping tools write bogus header pointers).
fn find_fmt(head: &[u8]) -> Option<usize> {
    if head.len() >= 28 {
        let p = read_u64_le(head, 0x0C) as usize;
        if p + 4 <= head.len() && &head[p..p + 4] == b"fmt " {
            return Some(p);
        }
    }
    head.windows(4).position(|w| w == b"fmt ")
}

/// Parse DSF metadata from an in-memory buffer (used by tests and by `parse`).
pub fn parse_bytes(data: &[u8], path: &Path) -> Result<DsfInfo> {
    if data.len() < 28 {
        return Err(SacdError::TruncatedHeader {
            path: path.to_path_buf(),
            needed: 28,
            got: data.len(),
        });
    }
    if &data[0..4] != b"DSD " {
        return Err(SacdError::BadMagic {
            path: path.to_path_buf(),
        });
    }
    let f = find_fmt(data).ok_or_else(|| SacdError::MissingFmtChunk {
        path: path.to_path_buf(),
        offset: read_u64_le(data, 0x0C),
    })?;
    if f + 0x34 > data.len() {
        return Err(SacdError::MissingFmtChunk {
            path: path.to_path_buf(),
            offset: f as u64,
        });
    }
    let version = read_u32_le(data, f + 0x0C);
    if version != 1 {
        return Err(SacdError::UnsupportedVersion {
            path: path.to_path_buf(),
            version,
        });
    }
    let channels = read_u32_le(data, f + 0x18);
    if channels != 2 {
        return Err(SacdError::UnsupportedChannels {
            path: path.to_path_buf(),
            channels,
        });
    }
    let sample_rate = read_u32_le(data, f + 0x1C);
    if sample_rate != DSD64_SAMPLE_RATE {
        return Err(SacdError::UnsupportedSampleRate {
            path: path.to_path_buf(),
            rate: sample_rate,
        });
    }
    let samples_per_channel = read_u64_le(data, f + 0x24);
    let block_size = read_u32_le(data, f + 0x2C);

    // In DSF the fmt chunk size (52) includes its own 12-byte chunk header.
    let data_header_offset = f + read_u64_le(data, f + 0x04) as usize;
    if data_header_offset + 12 > data.len()
        || &data[data_header_offset..data_header_offset + 4] != b"data"
    {
        return Err(SacdError::MissingDataChunk {
            path: path.to_path_buf(),
        });
    }
    let data_size = read_u64_le(data, data_header_offset + 4);
    let data_offset = (data_header_offset + 12) as u64;

    let tags = id3::parse_id3_tail(data, path)?.unwrap_or_default();

    Ok(DsfInfo {
        path: path.to_path_buf(),
        sample_rate,
        channels,
        samples_per_channel,
        block_size,
        data_offset,
        data_size,
        tags,
        bit_reverse_needed: true,
        packing: AudioPacking::Blocks,
    })
}

/// Parse a DSF file from disk: reads the 28-byte header, the `fmt ` chunk,
/// the `data` chunk header, and the last MiB for the ID3v2 tag.
pub fn parse(path: &Path) -> Result<DsfInfo> {
    fn io(path: &Path, source: std::io::Error) -> SacdError {
        SacdError::Io {
            path: path.to_path_buf(),
            source,
        }
    }
    let mut file = File::open(path).map_err(|e| io(path, e))?;
    let file_len = file.metadata().map_err(|e| io(path, e))?.len();

    let mut header = [0u8; 28];
    file.read_exact(&mut header).map_err(|e| io(path, e))?;
    if &header[0..4] != b"DSD " {
        return Err(SacdError::BadMagic {
            path: path.to_path_buf(),
        });
    }
    // Read the start of the file and locate `fmt ` (header pointer first,
    // then scan: some tools write bogus pointers, e.g. total_size = 28).
    let head_len = file_len.min(8192) as usize;
    file.seek(SeekFrom::Start(0)).map_err(|e| io(path, e))?;
    let mut head = vec![0u8; head_len];
    file.read_exact(&mut head).map_err(|e| io(path, e))?;
    let fmt_offset = find_fmt(&head).ok_or_else(|| SacdError::MissingFmtChunk {
        path: path.to_path_buf(),
        offset: read_u64_le(&header, 0x0C),
    })? as u64;
    file.seek(SeekFrom::Start(fmt_offset))
        .map_err(|e| io(path, e))?;
    let mut fmt = vec![0u8; 0x34];
    file.read_exact(&mut fmt).map_err(|e| io(path, e))?;
    if &fmt[0..4] != b"fmt " {
        return Err(SacdError::MissingFmtChunk {
            path: path.to_path_buf(),
            offset: fmt_offset,
        });
    }
    let version = read_u32_le(&fmt, 0x0C);
    if version != 1 {
        return Err(SacdError::UnsupportedVersion {
            path: path.to_path_buf(),
            version,
        });
    }
    let channels = read_u32_le(&fmt, 0x18);
    if channels != 2 {
        return Err(SacdError::UnsupportedChannels {
            path: path.to_path_buf(),
            channels,
        });
    }
    let sample_rate = read_u32_le(&fmt, 0x1C);
    if sample_rate != DSD64_SAMPLE_RATE {
        return Err(SacdError::UnsupportedSampleRate {
            path: path.to_path_buf(),
            rate: sample_rate,
        });
    }
    let samples_per_channel = read_u64_le(&fmt, 0x24);
    let block_size = read_u32_le(&fmt, 0x2C);

    let data_header_offset = fmt_offset + read_u64_le(&fmt, 0x04);
    file.seek(SeekFrom::Start(data_header_offset))
        .map_err(|e| io(path, e))?;
    let mut data_header = [0u8; 12];
    file.read_exact(&mut data_header).map_err(|e| io(path, e))?;
    if &data_header[0..4] != b"data" {
        return Err(SacdError::MissingDataChunk {
            path: path.to_path_buf(),
        });
    }
    let data_size = read_u64_le(&data_header, 4);

    let tail_len = file_len.min(1024 * 1024) as usize;
    file.seek(SeekFrom::Start(file_len - tail_len as u64))
        .map_err(|e| io(path, e))?;
    let mut tail = vec![0u8; tail_len];
    file.read_exact(&mut tail).map_err(|e| io(path, e))?;
    let tags = id3::parse_id3_tail(&tail, path)?.unwrap_or_default();

    Ok(DsfInfo {
        path: path.to_path_buf(),
        sample_rate,
        channels,
        samples_per_channel,
        block_size,
        data_offset: data_header_offset + 12,
        data_size,
        tags,
        bit_reverse_needed: true,
        packing: AudioPacking::Blocks,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::make_test_dsf;

    #[test]
    fn parses_valid_dsf_header() {
        let path = Path::new("test.dsf");
        let samples = 2_822_400 * 10; // 10 seconds
        let data = make_test_dsf(samples, false);
        let info = parse_bytes(&data, path).unwrap();
        assert_eq!(info.sample_rate, 2_822_400);
        assert_eq!(info.channels, 2);
        assert_eq!(info.samples_per_channel, samples);
        assert_eq!(info.block_size, 4096);
        assert_eq!(info.data_offset, 28 + 52 + 12);
        assert_eq!(info.data_size, info.block_count() * 4096 * 2);
        assert!((info.duration_secs() - 10.0).abs() < 1e-9);
    }

    #[test]
    fn sacd_frames_rounds_up() {
        let path = Path::new("test.dsf");
        // 1 sample = ceil(1 / (2822400/75)) frames -> 1 frame
        let data = make_test_dsf(1, false);
        let info = parse_bytes(&data, path).unwrap();
        assert_eq!(info.sacd_frames(), 1);
        // exactly 75 frames worth of samples at DSD64 = 2822400 samples -> 75 frames
        let data = make_test_dsf(2_822_400, false);
        let info = parse_bytes(&data, path).unwrap();
        assert_eq!(info.sacd_frames(), 75);
    }

    #[test]
    fn rejects_bad_magic() {
        let path = Path::new("bad.dsf");
        let mut data = make_test_dsf(1000, false);
        data[0] = b'X';
        assert!(matches!(
            parse_bytes(&data, path),
            Err(SacdError::BadMagic { .. })
        ));
    }

    #[test]
    fn rejects_truncated_header() {
        let path = Path::new("tiny.dsf");
        assert!(matches!(
            parse_bytes(b"DSD ", path),
            Err(SacdError::TruncatedHeader { .. })
        ));
    }

    #[test]
    fn rejects_wrong_sample_rate() {
        let path = Path::new("dsd128.dsf");
        let mut data = make_test_dsf(1000, false);
        let off = 28 + 0x1C;
        data[off..off + 4].copy_from_slice(&5_644_800u32.to_le_bytes());
        assert!(matches!(
            parse_bytes(&data, path),
            Err(SacdError::UnsupportedSampleRate {
                rate: 5_644_800,
                ..
            })
        ));
    }

    #[test]
    fn rejects_mono() {
        let path = Path::new("mono.dsf");
        let mut data = make_test_dsf(1000, false);
        let off = 28 + 0x18;
        data[off..off + 4].copy_from_slice(&1u32.to_le_bytes());
        assert!(matches!(
            parse_bytes(&data, path),
            Err(SacdError::UnsupportedChannels { channels: 1, .. })
        ));
    }

    #[test]
    fn falls_back_to_scan_when_fmt_pointer_bogus() {
        let path = Path::new("bogus.dsf");
        let mut data = make_test_dsf(32768, false);
        // Some ripping tools write total_size = 28 and a bogus fmt pointer;
        // the real `fmt ` chunk still sits at offset 28.
        data[4..12].copy_from_slice(&28u64.to_le_bytes());
        data[12..20].copy_from_slice(&999_999u64.to_le_bytes());
        let info = parse_bytes(&data, path).unwrap();
        assert_eq!(info.sample_rate, 2_822_400);
        assert_eq!(info.samples_per_channel, 32768);
        assert_eq!(info.data_offset, 92);
    }

    #[test]
    fn extracts_id3_title() {
        let path = Path::new("tagged.dsf");
        let data = make_test_dsf(2_822_400, true);
        let info = parse_bytes(&data, path).unwrap();
        assert_eq!(info.tags.title.as_deref(), Some("Test Song"));
    }
}
