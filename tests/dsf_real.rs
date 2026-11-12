//! Integration test against the first 92 bytes of a real DSF file
//! ("02 - Van Halen - Eruption.dsf", DSD64 stereo): DSD header + fmt chunk
//! + data chunk header.

use sacd_author::{dsf, id3};
use std::path::Path;

#[test]
fn parses_real_eruption_header() {
    let bytes = include_bytes!("fixtures/eruption_header.bin");
    let info = dsf::parse_bytes(bytes, Path::new("eruption.dsf")).unwrap();
    assert_eq!(info.sample_rate, 2_822_400);
    assert_eq!(info.channels, 2);
    assert_eq!(info.samples_per_channel, 288_562_176);
    assert_eq!(info.block_size, 4096);
    assert_eq!(info.data_offset, 92);
    assert_eq!(info.data_size, 72_146_956);
    assert!((info.duration_secs() - 102.24).abs() < 0.01);
    assert_eq!(info.sacd_frames(), 7668);
}

#[test]
fn parses_real_eruption_id3v23_utf16() {
    let bytes = include_bytes!("fixtures/eruption_id3.bin");
    let tags = id3::parse_id3_tail(bytes, Path::new("eruption.dsf"))
        .unwrap()
        .unwrap();
    assert_eq!(tags.title.as_deref(), Some("Eruption"));
    assert_eq!(tags.artist.as_deref(), Some("Van Halen"));
    assert_eq!(tags.album.as_deref(), Some("Van Halen"));
}

#[test]
fn parses_real_cc0_goldberg_header_with_bogus_pointers() {
    // decaud.io CC0 sampler: header has total_size=28 and fmt pointer past
    // EOF, but the real `fmt ` chunk sits at offset 28.
    let bytes = include_bytes!("fixtures/goldberg_header.bin");
    let info = dsf::parse_bytes(bytes, Path::new("goldberg.dsf")).unwrap();
    assert_eq!(info.sample_rate, 2_822_400);
    assert_eq!(info.channels, 2);
    assert_eq!(info.samples_per_channel, 265_870_080);
    assert_eq!(info.data_offset, 92);
    assert_eq!(info.sacd_frames(), 7065);
}

#[test]
fn parses_real_cc0_goldberg_id3() {
    let mut bytes = include_bytes!("fixtures/goldberg_header.bin").to_vec();
    bytes.extend_from_slice(include_bytes!("fixtures/goldberg_id3.bin"));
    let tags = id3::parse_id3_tail(&bytes[92..], Path::new("goldberg.dsf"))
        .unwrap()
        .unwrap();
    assert_eq!(
        tags.title.as_deref(),
        Some("Variatio 5 a 1 ovvero 2 Clav. (DSD64)")
    );
    assert_eq!(tags.artist.as_deref(), Some("Kimiko Ishizaka"));
    assert_eq!(
        tags.album.as_deref(),
        Some("The Open Goldberg Variations (Decaud DSD Sampler)")
    );
}
