//! Shared in-memory DSF fixture builder for unit tests.

use crate::dsf::{BLOCK_SIZE, DSD64_SAMPLE_RATE};

/// Build a minimal valid stereo DSD64 DSF in memory.
pub fn make_test_dsf(samples_per_channel: u64, with_id3: bool) -> Vec<u8> {
    let block_size = BLOCK_SIZE as u64;
    let n_blocks = samples_per_channel.div_ceil(block_size * 8);
    let data_size = n_blocks * block_size * 2;
    let fmt_offset = 28u64;
    let data_offset = fmt_offset + 52;
    let mut buf = Vec::new();
    buf.extend_from_slice(b"DSD ");
    let total_size_pos = buf.len();
    buf.extend_from_slice(&[0u8; 8]);
    buf.extend_from_slice(&fmt_offset.to_le_bytes());
    buf.extend_from_slice(&0u64.to_le_bytes());
    debug_assert_eq!(buf.len(), 28);
    buf.extend_from_slice(b"fmt ");
    buf.extend_from_slice(&52u64.to_le_bytes());
    buf.extend_from_slice(&1u32.to_le_bytes());
    buf.extend_from_slice(&0u32.to_le_bytes());
    buf.extend_from_slice(&2u32.to_le_bytes());
    buf.extend_from_slice(&2u32.to_le_bytes());
    buf.extend_from_slice(&DSD64_SAMPLE_RATE.to_le_bytes());
    buf.extend_from_slice(&1u32.to_le_bytes());
    buf.extend_from_slice(&samples_per_channel.to_le_bytes());
    buf.extend_from_slice(&BLOCK_SIZE.to_le_bytes());
    buf.extend_from_slice(&0u32.to_le_bytes());
    debug_assert_eq!(buf.len() as u64, data_offset);
    buf.extend_from_slice(b"data");
    buf.extend_from_slice(&data_size.to_le_bytes());
    buf.resize((data_offset + 12 + data_size) as usize, 0xAB);
    if with_id3 {
        let title = b"Test Song";
        let mut payload = vec![3u8];
        payload.extend_from_slice(title);
        let frame_len = payload.len() as u32;
        let synch = [
            ((frame_len >> 21) & 0x7F) as u8,
            ((frame_len >> 14) & 0x7F) as u8,
            ((frame_len >> 7) & 0x7F) as u8,
            (frame_len & 0x7F) as u8,
        ];
        let mut body = Vec::new();
        body.extend_from_slice(b"TIT2");
        body.extend_from_slice(&synch);
        body.extend_from_slice(&[0, 0]);
        body.extend_from_slice(&payload);
        let tag_len = body.len() as u32;
        let tag_synch = [
            ((tag_len >> 21) & 0x7F) as u8,
            ((tag_len >> 14) & 0x7F) as u8,
            ((tag_len >> 7) & 0x7F) as u8,
            (tag_len & 0x7F) as u8,
        ];
        buf.extend_from_slice(b"ID3");
        buf.extend_from_slice(&[4, 0, 0]);
        buf.extend_from_slice(&tag_synch);
        buf.extend_from_slice(&body);
    }
    let total = buf.len() as u64;
    buf[total_size_pos..total_size_pos + 8].copy_from_slice(&total.to_le_bytes());
    buf
}
