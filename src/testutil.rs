//! Shared in-memory fixture builders for unit tests.

use crate::dsf::{BLOCK_SIZE, DSD64_SAMPLE_RATE};

/// Deterministic pseudo-random byte pattern (xorshift64).
pub fn pattern(n: usize, seed: u64) -> Vec<u8> {
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

/// Build a minimal DoP (WAV-DSD) file from a byte-interleaved stereo DSD
/// stream ([L0 R0 L1 R1 ...], 4 bytes per sample pair), with alternating
/// 0x05/0xFA markers in the top byte of each 24-bit sample.
pub fn make_test_dop(interleaved: &[u8], title: Option<&str>) -> Vec<u8> {
    assert_eq!(interleaved.len() % 4, 0);
    fn chunk(id: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let mut c = id.to_vec();
        c.extend_from_slice(&(body.len() as u32).to_le_bytes());
        c.extend_from_slice(body);
        if body.len() % 2 == 1 {
            c.push(0);
        }
        c
    }
    let samples = interleaved.len() / 4;
    let mut data = Vec::with_capacity(samples * 6);
    for s in 0..samples {
        let mk = if s % 2 == 0 { 0x05u8 } else { 0xFAu8 };
        data.extend_from_slice(&[
            interleaved[4 * s],
            interleaved[4 * s + 2],
            mk,
            interleaved[4 * s + 1],
            interleaved[4 * s + 3],
            mk,
        ]);
    }
    let mut fmt = Vec::new();
    fmt.extend_from_slice(&3u16.to_le_bytes()); // IEEE float
    fmt.extend_from_slice(&2u16.to_le_bytes());
    fmt.extend_from_slice(&176_400u32.to_le_bytes());
    fmt.extend_from_slice(&((176_400 * 6) as u32).to_le_bytes());
    fmt.extend_from_slice(&6u16.to_le_bytes());
    fmt.extend_from_slice(&24u16.to_le_bytes());
    let mut out = b"RIFF".to_vec();
    let riff_size_pos = out.len();
    out.extend_from_slice(&[0u8; 4]);
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(&chunk(b"fmt ", &fmt));
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

/// Build a minimal DFF with the given audio bytes (channel blocks of 4096
/// bytes each, interleaved block by block, native SACD bit order), the given
/// channel count (2..=6) and an optional ID3v2.4 title.
pub fn make_test_dff(audio: &[u8], channels: u16, title: Option<&str>) -> Vec<u8> {
    assert!(audio.len().is_multiple_of(4096 * channels as usize));
    fn chunk(id: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let mut c = id.to_vec();
        c.extend_from_slice(&(body.len() as u64).to_be_bytes());
        c.extend_from_slice(body);
        if body.len() % 2 == 1 {
            c.push(0);
        }
        c
    }
    let mut prop = b"SND ".to_vec();
    let fs = {
        let mut c = b"FS  ".to_vec();
        c.extend_from_slice(&4u64.to_be_bytes());
        c.extend_from_slice(&2_822_400u32.to_be_bytes());
        c
    };
    let chnl = {
        let mut c = b"CHNL".to_vec();
        c.extend_from_slice(&2u64.to_be_bytes());
        c.extend_from_slice(&channels.to_be_bytes());
        c
    };
    let cmpr = {
        let mut c = b"CMPR".to_vec();
        c.extend_from_slice(&5u64.to_be_bytes());
        c.extend_from_slice(b"DSD ");
        c.push(0);
        c
    };
    prop.extend_from_slice(&fs);
    prop.extend_from_slice(&chnl);
    prop.extend_from_slice(&cmpr);

    let mut id3_body = Vec::new();
    if let Some(t) = title {
        let mut payload = vec![3u8];
        payload.extend_from_slice(t.as_bytes());
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
        id3_body.extend_from_slice(b"ID3");
        id3_body.extend_from_slice(&[4, 0, 0]);
        id3_body.extend_from_slice(&synch(body.len() as u32));
        id3_body.extend_from_slice(&body);
    }
    let fver = chunk(b"FVER", &0x01050000u32.to_be_bytes());
    let prop_chunk = chunk(b"PROP", &prop);
    let dsd_chunk = chunk(b"DSD ", audio);
    let id3c = if id3_body.is_empty() {
        Vec::new()
    } else {
        chunk(b"ID3 ", &id3_body)
    };
    let body_len = fver.len() + prop_chunk.len() + dsd_chunk.len() + id3c.len();
    let mut out = b"FRM8".to_vec();
    out.extend_from_slice(&(body_len as u64 + 4).to_be_bytes());
    out.extend_from_slice(b"DSD ");
    out.extend_from_slice(&fver);
    out.extend_from_slice(&prop_chunk);
    out.extend_from_slice(&dsd_chunk);
    out.extend_from_slice(&id3c);
    out
}
