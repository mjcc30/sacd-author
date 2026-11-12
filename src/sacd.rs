//! SACD audio area (2C_Audio) layout and DSD stream conversion.
//!
//! In a DSF file, the `data` chunk stores interleaved blocks of 4096 bytes
//! per channel: [L block 0][R block 0][L block 1][R block 1]...
//! In the SACD 2C_Audio file, stereo DSD is interleaved byte by byte
//! (L0 R0 L1 R1 ...) inside 2048-byte sectors. One DSF block pair
//! (2 * 4096 bytes) therefore maps to exactly 4 SACD sectors (8192 bytes).
//!
//! The last DSF block pair may be partial (the source file usually pads it
//! to full 4096-byte blocks); we pad it ourselves with zeros so every track
//! occupies a whole number of 2048-byte sectors.

use crate::dsf::DsfInfo;
use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::Path;

pub const SECTOR_SIZE: usize = 2048;
pub const DSF_BLOCK_SIZE: usize = 4096;
pub const SECTOR_PAIR_BYTES: usize = 2 * DSF_BLOCK_SIZE; // 8192 = 4 sectors
pub const SECTORS_PER_BLOCK_PAIR: u32 = (SECTOR_PAIR_BYTES / SECTOR_SIZE) as u32; // 4

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
        let block_pairs = info.block_count();
        let sector_count = (block_pairs as u32) * SECTORS_PER_BLOCK_PAIR;
        layouts.push(TrackLayout {
            start_sector: next_sector,
            sector_count,
            frames: info.sacd_frames(),
        });
        next_sector += sector_count;
    }
    layouts
}

/// Convert one DSF `data` chunk into SACD sectors appended to `out`.
/// Returns the number of 2048-byte sectors written.
pub fn convert_track(info: &DsfInfo, out: &mut impl Write) -> std::io::Result<u32> {
    let path = info.path.clone();
    let mut file = File::open(&path)
        .map_err(|e| std::io::Error::new(e.kind(), format!("{}: {e}", path.display())))?;
    file.seek(SeekFrom::Start(info.data_offset))?;
    let mut reader = BufReader::with_capacity(2 * DSF_BLOCK_SIZE, &mut file);
    let mut writer = BufWriter::with_capacity(SECTOR_PAIR_BYTES, out);

    let mut sectors = 0u32;
    let mut block_pair = [0u8; SECTOR_PAIR_BYTES];
    loop {
        let mut l = [0u8; DSF_BLOCK_SIZE];
        let mut r = [0u8; DSF_BLOCK_SIZE];
        let nl = read_padded(&mut reader, &mut l)?;
        let nr = read_padded(&mut reader, &mut r)?;
        if nl == 0 && nr == 0 {
            break;
        }
        // Byte-wise interleave L/R into the output pair buffer.
        for i in 0..DSF_BLOCK_SIZE {
            block_pair[2 * i] = l[i];
            block_pair[2 * i + 1] = r[i];
        }
        writer.write_all(&block_pair)?;
        sectors += SECTORS_PER_BLOCK_PAIR;
    }
    writer.flush()?;
    Ok(sectors)
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

/// Convert a DSF file from disk into a 2C_Audio file. Returns track layouts.
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
        let path = f.path().to_path_buf();
        let info = crate::dsf::parse_bytes(&data, &path).unwrap();
        (f, info)
    }

    #[test]
    fn layouts_accumulate_sectors() {
        let (_fa, a) = write_temp_dsf(2_822_400);
        let (_fb, b) = write_temp_dsf(2_822_400 * 2);
        let layouts = track_layouts(&[a.clone(), b.clone()]);
        assert_eq!(layouts[0].start_sector, 0);
        assert_eq!(layouts[0].sector_count, a.block_count() as u32 * 4);
        assert_eq!(layouts[1].start_sector, layouts[0].sector_count);
        assert_eq!(layouts[1].sector_count, b.block_count() as u32 * 4);
        assert_eq!(layouts[0].frames, 75);
        assert_eq!(layouts[1].frames, 150);
    }

    #[test]
    fn conversion_interleaves_bytes_and_pads() {
        // 32768 samples = exactly one full block pair (4096 bytes per channel)
        let samples = 32768u64;
        let mut data = make_test_dsf(samples, false);
        // fill data chunk with a known pattern: L blocks = 0xAA, R blocks = 0xBB
        let data_offset = 92usize;
        for i in 0..data.len() - data_offset {
            let block_index = i / DSF_BLOCK_SIZE;
            data[data_offset + i] = if block_index.is_multiple_of(2) {
                0xAA
            } else {
                0xBB
            };
        }
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(&data).unwrap();
        let info = crate::dsf::parse_bytes(&data, f.path()).unwrap();

        let mut out = Vec::new();
        let sectors = convert_track(&info, &mut out).unwrap();
        assert_eq!(sectors, 4);
        assert_eq!(out.len(), 8192);
        assert!(out.as_chunks::<2>().0.iter().all(|c| *c == [0xAA, 0xBB]));
    }

    #[test]
    fn conversion_pads_partial_last_block() {
        // 100 samples -> 1 block pair, data chunk padded with 0xAB by the fixture
        let (_f, info) = write_temp_dsf(100);
        let mut out = Vec::new();
        let sectors = convert_track(&info, &mut out).unwrap();
        assert_eq!(sectors, 4);
        assert_eq!(out.len(), 8192);
        assert_eq!(&out[0..4], &[0xAB, 0xAB, 0xAB, 0xAB]);
    }
}
