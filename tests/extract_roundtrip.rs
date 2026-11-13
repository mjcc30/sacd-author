//! ISO -> DSF extraction round-trip: build an image from a fixture DSF
//! (with ID3 title), read it back with iso_read, write it with dsf_write,
//! and verify the audio data chunk and tags match the source.

use sacd_author::{dsf, dsf_write, image, iso_read};
use std::io::Write;

fn make_dsf(samples: u64, title: &str) -> Vec<u8> {
    let block = 4096u64;
    let n_blocks = samples.div_ceil(block * 8);
    let data_size = n_blocks * block * 2;
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
    buf.extend_from_slice(&2_822_400u32.to_le_bytes());
    buf.extend_from_slice(&1u32.to_le_bytes());
    buf.extend_from_slice(&samples.to_le_bytes());
    buf.extend_from_slice(&4096u32.to_le_bytes());
    buf.extend_from_slice(&0u32.to_le_bytes());
    buf.extend_from_slice(b"data");
    buf.extend_from_slice(&data_size.to_le_bytes());
    // pseudo-random audio
    let mut x = 0x12345678u64;
    let mut audio = vec![0u8; data_size as usize];
    for b in audio.iter_mut() {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        *b = (x >> 32) as u8;
    }
    buf.extend_from_slice(&audio);
    // ID3v2.4 TIT2
    let mut payload = vec![3u8];
    payload.extend_from_slice(title.as_bytes());
    let synch = |n: u32| {
        [
            (n >> 21 & 0x7F) as u8,
            (n >> 14 & 0x7F) as u8,
            (n >> 7 & 0x7F) as u8,
            (n & 0x7F) as u8,
        ]
    };
    let mut body = Vec::new();
    body.extend_from_slice(b"TIT2");
    body.extend_from_slice(&synch(payload.len() as u32));
    body.extend_from_slice(&[0, 0]);
    body.extend_from_slice(&payload);
    buf.extend_from_slice(b"ID3");
    buf.extend_from_slice(&[4, 0, 0]);
    buf.extend_from_slice(&synch(body.len() as u32));
    buf.extend_from_slice(&body);
    let total = buf.len() as u64;
    buf[total_pos..total_pos + 8].copy_from_slice(&total.to_le_bytes());
    buf
}

#[test]
fn extract_roundtrip_matches_source() {
    let src = make_dsf(32768, "Round Trip Song");
    let mut f = tempfile::NamedTempFile::new().unwrap();
    f.write_all(&src).unwrap();
    let info = dsf::parse_bytes(&src, f.path()).unwrap();

    let iso_file = tempfile::NamedTempFile::new().unwrap();
    image::build_image(&[info], iso_file.path()).unwrap();

    let img = iso_read::read_iso(iso_file.path()).unwrap();
    assert_eq!(img.tracks.len(), 1);
    assert_eq!(img.tracks[0].title.as_deref(), Some("Round Trip Song"));

    // extracted data chunk must equal the source data chunk
    let src_data = &src[92..92 + 8192];
    assert_eq!(img.tracks[0].data, src_data);

    // and writing it back as DSF must parse to the same audio
    let out = tempfile::NamedTempFile::new().unwrap();
    let samples = img.tracks[0].data.len() as u64 / 2 * 8;
    dsf_write::write_dsf(
        out.path(),
        &img.tracks[0].data,
        samples,
        img.tracks[0].title.as_deref(),
        None,
        None,
    )
    .unwrap();
    let back = std::fs::read(out.path()).unwrap();
    let info2 = dsf::parse_bytes(&back, out.path()).unwrap();
    assert_eq!(info2.data_size, 8192);
    assert_eq!(&back[92..92 + 8192], src_data);
    assert_eq!(info2.tags.title.as_deref(), Some("Round Trip Song"));
}
