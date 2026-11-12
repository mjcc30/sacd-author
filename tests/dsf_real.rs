//! Integration test against the first 92 bytes of a real DSF file
//! ("02 - Van Halen - Eruption.dsf", DSD64 stereo): DSD header + fmt chunk
//! + data chunk header.

use sacd_author::dsf;
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
