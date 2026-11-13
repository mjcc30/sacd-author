//! CLI-level tests for the `dsd2pcm` subcommand: DSF -> FLAC (decoded back
//! with claxon, an independent FLAC implementation) and SACD-R ISO -> WAV dir.

use sacd_author::{dsf, image};
use std::io::Write;
use std::process::Command;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_sacd-author")
}

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

/// Minimal stereo DSD64 DSF in a temp file with the given data chunk.
fn make_dsf_file(data: &[u8]) -> tempfile::NamedTempFile {
    let samples = (data.len() / 2) as u64 * 8;
    let mut b = Vec::new();
    b.extend_from_slice(b"DSD ");
    let total_pos = b.len();
    b.extend_from_slice(&[0u8; 8]);
    b.extend_from_slice(&28u64.to_le_bytes());
    b.extend_from_slice(&0u64.to_le_bytes());
    b.extend_from_slice(b"fmt ");
    b.extend_from_slice(&52u64.to_le_bytes());
    b.extend_from_slice(&1u32.to_le_bytes());
    b.extend_from_slice(&0u32.to_le_bytes());
    b.extend_from_slice(&2u32.to_le_bytes());
    b.extend_from_slice(&2u32.to_le_bytes());
    b.extend_from_slice(&2_822_400u32.to_le_bytes());
    b.extend_from_slice(&1u32.to_le_bytes());
    b.extend_from_slice(&samples.to_le_bytes());
    b.extend_from_slice(&4096u32.to_le_bytes());
    b.extend_from_slice(&0u32.to_le_bytes());
    b.extend_from_slice(b"data");
    b.extend_from_slice(&(data.len() as u64).to_le_bytes());
    b.extend_from_slice(data);
    let total = b.len() as u64;
    b[total_pos..total_pos + 8].copy_from_slice(&total.to_le_bytes());
    let mut f = tempfile::NamedTempFile::new().unwrap();
    f.write_all(&b).unwrap();
    f
}

#[test]
fn cli_dsd2pcm_dsf_to_flac() {
    let data = pattern(4 * 8192, 0xBEEF);
    let dsf_file = make_dsf_file(&data);
    let out_dir = tempfile::tempdir().unwrap();
    let flac = out_dir.path().join("out.flac");
    let out = Command::new(bin())
        .args([
            "dsd2pcm",
            dsf_file.path().to_str().unwrap(),
            "-o",
            flac.to_str().unwrap(),
            "-r",
            "48000",
            "-b",
            "16",
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let mut reader = claxon::FlacReader::open(&flac).unwrap();
    let si = reader.streaminfo();
    assert_eq!(si.sample_rate, 48_000);
    assert_eq!(si.channels, 2);
    assert_eq!(si.bits_per_sample, 16);
    let n = reader.samples().count();
    // 4 block pairs = 131072 samples/ch -> 131072*48000/2822400 = 2228 frames
    assert!((2 * 2228..2 * (2228 + 3000)).contains(&n), "samples {n}");
}

#[test]
fn cli_dsd2pcm_iso_to_wav_dir() {
    let data = pattern(2 * 8192, 0xFEED);
    let dsf_file = make_dsf_file(&data);
    let info = dsf::parse(dsf_file.path()).unwrap();
    let iso_file = tempfile::Builder::new().suffix(".iso").tempfile().unwrap();
    image::build_image(&[info], iso_file.path()).unwrap();

    let out_dir = tempfile::tempdir().unwrap();
    let out = Command::new(bin())
        .args([
            "dsd2pcm",
            iso_file.path().to_str().unwrap(),
            "-o",
            out_dir.path().to_str().unwrap(),
            "-f",
            "wav",
            "-r",
            "44100",
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let entries: Vec<_> = std::fs::read_dir(out_dir.path())
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    assert_eq!(entries.len(), 1);
    assert!(entries[0].extension().unwrap() == "wav");
    let b = std::fs::read(&entries[0]).unwrap();
    assert_eq!(&b[0..4], b"RIFF");
    assert_eq!(&b[8..12], b"WAVE");
    let rate = u32::from_le_bytes([b[24], b[25], b[26], b[27]]);
    assert_eq!(rate, 44_100);
}
