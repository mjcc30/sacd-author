//! Bit-exact round-trip test: fixture DSF (random pattern) -> ISO image ->
//! unpack the 2C_Audio sectors (strip sector/packet/frame headers, undo the
//! bit-reversal, de-interleave L/R) -> must equal the original DSF data chunk.

use sacd_author::{dsf, image, sacd};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};

const SECTOR: usize = 2048;

fn sec(f: &mut File, lba: u32) -> Vec<u8> {
    f.seek(SeekFrom::Start(lba as u64 * SECTOR as u64)).unwrap();
    let mut buf = vec![0u8; SECTOR];
    f.read_exact(&mut buf).unwrap();
    buf
}

fn be32(b: &[u8], off: usize) -> u32 {
    u32::from_be_bytes([b[off], b[off + 1], b[off + 2], b[off + 3]])
}

/// Pseudo-random byte pattern (xorshift64), deterministic.
fn pattern(n: usize) -> Vec<u8> {
    let mut x = 0x9E3779B97F4A7C15u64;
    (0..n)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            (x >> 32) as u8
        })
        .collect()
}

#[test]
fn dsf_iso_roundtrip_is_bit_exact() {
    // 3 block pairs = 24576 interleaved bytes = 2 full frames + 5856 bytes
    let data = pattern(3 * 2 * 4096);
    let samples = (3 * 4096 * 8) as u64; // 3 full blocks per channel
    let mut dsf_bytes = Vec::new();
    dsf_bytes.extend_from_slice(b"DSD ");
    let total_pos = dsf_bytes.len();
    dsf_bytes.extend_from_slice(&[0u8; 8]);
    dsf_bytes.extend_from_slice(&28u64.to_le_bytes());
    dsf_bytes.extend_from_slice(&0u64.to_le_bytes());
    dsf_bytes.extend_from_slice(b"fmt ");
    dsf_bytes.extend_from_slice(&52u64.to_le_bytes());
    dsf_bytes.extend_from_slice(&1u32.to_le_bytes());
    dsf_bytes.extend_from_slice(&0u32.to_le_bytes());
    dsf_bytes.extend_from_slice(&2u32.to_le_bytes());
    dsf_bytes.extend_from_slice(&2u32.to_le_bytes());
    dsf_bytes.extend_from_slice(&2_822_400u32.to_le_bytes());
    dsf_bytes.extend_from_slice(&1u32.to_le_bytes());
    dsf_bytes.extend_from_slice(&samples.to_le_bytes());
    dsf_bytes.extend_from_slice(&4096u32.to_le_bytes());
    dsf_bytes.extend_from_slice(&0u32.to_le_bytes());
    dsf_bytes.extend_from_slice(b"data");
    dsf_bytes.extend_from_slice(&(data.len() as u64).to_le_bytes());
    dsf_bytes.extend_from_slice(&data);
    let total = dsf_bytes.len() as u64;
    dsf_bytes[total_pos..total_pos + 8].copy_from_slice(&total.to_le_bytes());

    let mut f = tempfile::NamedTempFile::new().unwrap();
    f.write_all(&dsf_bytes).unwrap();
    let info = dsf::parse_bytes(&dsf_bytes, f.path()).unwrap();

    let iso_file = tempfile::NamedTempFile::new().unwrap();
    image::build_image(&[info], iso_file.path()).unwrap();

    // --- read the ISO back like a player ---
    let mut iso = File::open(iso_file.path()).unwrap();
    let m = sec(&mut iso, 510);
    let area1 = be32(&m, 0x40);
    let area1_size = u16::from_be_bytes([m[0x54], m[0x55]]) as u32;
    let mut trl1 = None;
    for i in 0..area1_size {
        let s = sec(&mut iso, area1 + i);
        if &s[0..8] == b"SACDTRL1" {
            trl1 = Some(s);
        }
    }
    let trl1 = trl1.unwrap();
    let track_start = be32(&trl1, 8);
    let track_len = be32(&trl1, 8 + 255 * 4);

    // --- unpack audio sectors into the interleaved (bit-reversed) stream ---
    let mut packed = Vec::new();
    let mut cur = track_start;
    while cur < track_start + track_len {
        let s = sec(&mut iso, cur);
        let h = s[0];
        let fic = (h >> 2) & 7;
        let pic = (h >> 5) & 7;
        let mut pos = 1usize;
        for _ in 0..pic {
            let b0 = s[pos];
            let len = (((b0 & 7) as usize) << 8) | s[pos + 1] as usize;
            pos += 2;
            if (b0 >> 7) & 1 == 1 {
                pos += 3 * fic as usize; // frame timecode(s)
            }
            packed.extend_from_slice(&s[pos..pos + len]);
            pos += len;
        }
        cur += 1;
    }
    assert_eq!(packed.len(), sacd::FRAME_DATA_BYTES * 3); // 3 frames (2 full + 1 padded)

    // --- undo bit-reversal and de-interleave L/R into DSF block layout ---
    let mut rebuilt = vec![0u8; data.len()];
    let inter: Vec<u8> = packed[..data.len()]
        .iter()
        .map(|b| b.reverse_bits())
        .collect();
    for block in 0..3 {
        for i in 0..4096 {
            rebuilt[block * 8192 + i] = inter[block * 8192 + 2 * i]; // L
            rebuilt[block * 8192 + 4096 + i] = inter[block * 8192 + 2 * i + 1]; // R
        }
    }
    assert_eq!(rebuilt, data, "round-trip mismatch");
}
