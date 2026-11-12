//! SACD audio area (2C_Audio) layout and DSF -> SACD sector conversion.
//!
//! DSF `data` chunk: interleaved 4096-byte blocks per channel
//! ([L blk0][R blk0][L blk1][R blk1]...). We first re-interleave byte-wise
//! (L0 R0 L1 R1 ...) to get the plain stereo DSD stream.
//!
//! SACD 2C_Audio sectors (2048 bytes each) are NOT raw DSD: each sector is
//! packetized (Scarlet Book, see sacd-ripper's scarletbook.h):
//! - byte 0: sector header — bit0 dst_encoded, bit1 reserved,
//!   bits 2-4 frame_info_count, bits 5-7 packet_info_count
//! - then packet_info_count * 2 bytes packet info (big-endian bitfield, as
//!   decoded by sacd-ripper: `(b0 & 7) << 8 | b1`) — bit7 frame_start,
//!   bits 3-5 data_type (2 = audio), bits 0-10 packet_length (11 bits)
//! - then frame_info_count * 3 bytes frame timecode (minutes, seconds,
//!   frames @75fps); present on the first sector of each audio frame only
//! - then packet_length bytes of DSD payload
//!
//! One audio frame (1/75 s) = 37632 samples/channel = 4704 bytes/channel =
//! 9408 bytes of interleaved stereo = 5 sectors of 2048 bytes
//! (2042 + 3*2045 + 1231 payload bytes).
//!
//! Note: SACD stores DSD bytes bit-reversed compared to DSF (see
//! sacd-ripper's bit_reverse_table in dsf.c), so every payload byte is
//! bit-reversed during conversion.

use crate::dsf::DsfInfo;
use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::Path;

pub const SECTOR_SIZE: usize = 2048;
pub const DSF_BLOCK_SIZE: usize = 4096;
pub const SECTOR_PAIR_BYTES: usize = 2 * DSF_BLOCK_SIZE; // 8192
pub const SAMPLES_PER_FRAME: u64 = 588 * 64; // 37632 @ DSD64
pub const FRAME_DATA_BYTES: usize = (SAMPLES_PER_FRAME / 8 * 2) as usize; // 9408
pub const SECTORS_PER_FRAME: u32 = 5;
pub const SACD_FRAME_RATE: u64 = 75;
pub const DATA_TYPE_AUDIO: u8 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrackLayout {
    /// LBA of the first sector of the track inside 2C_Audio.
    pub start_sector: u32,
    /// Number of 2048-byte sectors occupied by the track (padding included).
    pub sector_count: u32,
    /// Duration in SACD frames (75 per second).
    pub frames: u64,
}

/// Compute the layout of every track inside the 2C_Audio file.
pub fn track_layouts(infos: &[DsfInfo]) -> Vec<TrackLayout> {
    let mut layouts = Vec::with_capacity(infos.len());
    let mut next_sector = 0u32;
    for info in infos {
        let frames = info.sacd_frames();
        let sector_count = (frames as u32) * SECTORS_PER_FRAME;
        layouts.push(TrackLayout {
            start_sector: next_sector,
            sector_count,
            frames,
        });
        next_sector += sector_count;
    }
    layouts
}

/// Timecode (minutes, seconds, frames) of audio frame `idx` at 75 fps.
pub fn frame_timecode(idx: u64) -> (u8, u8, u8) {
    let frames = (idx % SACD_FRAME_RATE) as u8;
    let total_seconds = idx / SACD_FRAME_RATE;
    let seconds = (total_seconds % 60) as u8;
    let minutes = (total_seconds / 60) as u8;
    (minutes, seconds, frames)
}

/// Pack one full audio frame (FRAME_DATA_BYTES bytes) into SECTORS_PER_FRAME
/// 2048-byte sectors and write them to `out`.
fn emit_frame(out: &mut impl Write, idx: u64, data: &[u8]) -> std::io::Result<()> {
    debug_assert_eq!(data.len(), FRAME_DATA_BYTES);
    let (m, s, f) = frame_timecode(idx);
    let mut pos = 0usize;
    let mut first = true;
    while pos < data.len() {
        let mut sector = [0u8; SECTOR_SIZE];
        let frame_info_count: u8 = if first { 1 } else { 0 };
        sector[0] = (1u8 << 5) | (frame_info_count << 2); // packet_info_count=1, dst=0
        let header_len = 1 + 2 + if first { 3 } else { 0 };
        let len = (data.len() - pos).min(SECTOR_SIZE - header_len);
        let frame_start: u8 = if first { 1 } else { 0 };
        sector[1] = (frame_start << 7) | (DATA_TYPE_AUDIO << 3) | ((len >> 8) as u8 & 0x07);
        sector[2] = (len & 0xFF) as u8;
        if first {
            sector[3] = m;
            sector[4] = s;
            sector[5] = f;
        }
        sector[header_len..header_len + len].copy_from_slice(&data[pos..pos + len]);
        out.write_all(&sector)?;
        pos += len;
        first = false;
    }
    Ok(())
}

/// Accumulates the interleaved DSD stream and emits packed frames.
struct FramePacker<W: Write> {
    out: W,
    buf: Vec<u8>,
    frame_idx: u64,
    sectors: u32,
}

impl<W: Write> FramePacker<W> {
    fn new(out: W) -> Self {
        Self {
            out,
            buf: Vec::with_capacity(FRAME_DATA_BYTES),
            frame_idx: 0,
            sectors: 0,
        }
    }

    fn push(&mut self, data: &[u8]) -> std::io::Result<()> {
        self.buf.extend_from_slice(data);
        while self.buf.len() >= FRAME_DATA_BYTES {
            let frame: Vec<u8> = self.buf.drain(..FRAME_DATA_BYTES).collect();
            emit_frame(&mut self.out, self.frame_idx, &frame)?;
            self.frame_idx += 1;
            self.sectors += SECTORS_PER_FRAME;
        }
        Ok(())
    }

    fn finish(mut self) -> std::io::Result<u32> {
        if !self.buf.is_empty() {
            self.buf.resize(FRAME_DATA_BYTES, 0); // pad last frame with zeros
            emit_frame(&mut self.out, self.frame_idx, &self.buf)?;
            self.frame_idx += 1;
            self.sectors += SECTORS_PER_FRAME;
        }
        Ok(self.sectors)
    }
}

/// Read up to `buf.len()` bytes; zero-pads the remainder. Returns bytes read.
fn read_padded(reader: &mut impl Read, buf: &mut [u8]) -> std::io::Result<usize> {
    let mut read = 0;
    while read < buf.len() {
        match reader.read(&mut buf[read..])? {
            0 => break,
            n => read += n,
        }
    }
    for b in &mut buf[read..] {
        *b = 0;
    }
    Ok(read)
}

/// Convert one DSF `data` chunk into packed SACD sectors appended to `out`.
/// Returns the number of 2048-byte sectors written.
pub fn convert_track(info: &DsfInfo, out: &mut impl Write) -> std::io::Result<u32> {
    let path = info.path.clone();
    let mut file = File::open(&path)
        .map_err(|e| std::io::Error::new(e.kind(), format!("{}: {e}", path.display())))?;
    file.seek(SeekFrom::Start(info.data_offset))?;
    let mut reader = BufReader::with_capacity(SECTOR_PAIR_BYTES, &mut file);
    let writer = BufWriter::with_capacity(SECTORS_PER_FRAME as usize * SECTOR_SIZE, out);
    let mut packer = FramePacker::new(writer);

    let mut pair = [0u8; SECTOR_PAIR_BYTES];
    loop {
        let mut l = [0u8; DSF_BLOCK_SIZE];
        let mut r = [0u8; DSF_BLOCK_SIZE];
        let nl = read_padded(&mut reader, &mut l)?;
        let nr = read_padded(&mut reader, &mut r)?;
        if nl == 0 && nr == 0 {
            break;
        }
        for i in 0..DSF_BLOCK_SIZE {
            pair[2 * i] = l[i].reverse_bits();
            pair[2 * i + 1] = r[i].reverse_bits();
        }
        packer.push(&pair)?;
    }
    let sectors = packer.finish()?;
    Ok(sectors)
}

/// Convert DSF files from disk into a 2C_Audio file. Returns track layouts.
pub fn build_audio_area(infos: &[DsfInfo], out_path: &Path) -> std::io::Result<Vec<TrackLayout>> {
    let layouts = track_layouts(infos);
    let out = File::create(out_path)?;
    let mut writer = BufWriter::with_capacity(1024 * 1024, out);
    for info in infos {
        convert_track(info, &mut writer)?;
    }
    writer.flush()?;
    Ok(layouts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::make_test_dsf;
    use std::io::Write;

    fn write_temp_dsf(samples: u64) -> (tempfile::NamedTempFile, DsfInfo) {
        let data = make_test_dsf(samples, false);
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(&data).unwrap();
        let info = crate::dsf::parse_bytes(&data, f.path()).unwrap();
        (f, info)
    }

    #[test]
    fn layouts_use_five_sectors_per_frame() {
        let (_fa, a) = write_temp_dsf(2_822_400); // 1 s -> 75 frames
        let (_fb, b) = write_temp_dsf(2_822_400 * 2);
        let layouts = track_layouts(&[a.clone(), b.clone()]);
        assert_eq!(layouts[0].frames, 75);
        assert_eq!(layouts[0].sector_count, 75 * 5);
        assert_eq!(layouts[0].start_sector, 0);
        assert_eq!(layouts[1].start_sector, 75 * 5);
        assert_eq!(layouts[1].frames, 150);
        assert_eq!(layouts[1].sector_count, 150 * 5);
    }

    #[test]
    fn frame_timecode_at_75fps() {
        assert_eq!(frame_timecode(0), (0, 0, 0));
        assert_eq!(frame_timecode(75), (0, 1, 0));
        assert_eq!(frame_timecode(7065), (1, 34, 15));
    }

    #[test]
    fn emit_frame_packs_five_sectors() {
        let data = vec![0xABu8; FRAME_DATA_BYTES];
        let mut out = Vec::new();
        emit_frame(&mut out, 75, &data).unwrap();
        assert_eq!(out.len(), 5 * SECTOR_SIZE);
        // sector 0: header 0x24, packet info (BE bitfield) frame_start=1 data_type=2 len=2042
        assert_eq!(out[0], 0x24);
        assert_eq!(out[1], (1 << 7) | (2 << 3) | 7); // len 2042 = 0x7FA
        assert_eq!(out[2], 0xFA);
        assert_eq!(&out[3..6], &[0, 1, 0]); // timecode of frame 75 = 0:01:00
        assert!(out[6..6 + 2042].iter().all(|&b| b == 0xAB));
        // sector 1: header 0x20, frame_start=0, len=2045
        let s1 = SECTOR_SIZE;
        assert_eq!(out[s1], 0x20);
        assert_eq!(out[s1 + 1], (2 << 3) | 7); // len 2045 = 0x7FD
        assert_eq!(out[s1 + 2], 0xFD);
        // last sector: len = 9408 - 2042 - 3*2045 = 1231 = 0x4CF
        let s4 = 4 * SECTOR_SIZE;
        assert_eq!(out[s4], 0x20);
        assert_eq!(out[s4 + 1], (2 << 3) | 4); // len 1231 = 0x4CF
        assert_eq!(out[s4 + 2], 0xCF);
        assert!(out[s4 + 3..].iter().all(|&b| b == 0 || b == 0xAB));
    }

    #[test]
    fn conversion_matches_frame_packing() {
        // 32768 samples = 1 DSF block pair = 8192 interleaved bytes < 1 frame
        let (_f, info) = write_temp_dsf(32768);
        let mut out = Vec::new();
        let sectors = convert_track(&info, &mut out).unwrap();
        assert_eq!(sectors, SECTORS_PER_FRAME); // 1 frame, padded
        assert_eq!(out.len(), 5 * SECTOR_SIZE);
        assert_eq!(out[0], 0x24);
        // 2 full block pairs = 16384 bytes = 1 frame + 6976 bytes -> 2 frames
        let (_f2, info2) = write_temp_dsf(65536);
        let mut out2 = Vec::new();
        let sectors2 = convert_track(&info2, &mut out2).unwrap();
        assert_eq!(sectors2, 2 * SECTORS_PER_FRAME);
    }
}
