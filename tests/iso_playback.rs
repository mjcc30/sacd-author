//! End-to-end "playback" test: build an ISO from two fixture DSF tracks,
//! then read it back the way foo_input_sacd/libsacdread do:
//! master TOC (SACDMTOC + 8x SACDText + SACD_Man), area TOC (40 sectors,
//! scan for SACDTRL1/SACDTRL2), then walk every audio sector parsing
//! sector/packet/frame headers and verify timecodes + payload round-trip.

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

fn be16(b: &[u8], off: usize) -> u16 {
    u16::from_be_bytes([b[off], b[off + 1]])
}
fn be32(b: &[u8], off: usize) -> u32 {
    u32::from_be_bytes([b[off], b[off + 1], b[off + 2], b[off + 3]])
}

#[test]
fn iso_plays_back_like_a_sacd_reader() {
    // Two tracks: 32768 samples each (fixture fills data chunk with 0xAB).
    let mut files = Vec::new();
    let mut infos = Vec::new();
    for _ in 0..2 {
        let bytes = make_dsf(32768);
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(&bytes).unwrap();
        infos.push(dsf::parse_bytes(&bytes, f.path()).unwrap());
        files.push(f);
    }
    let iso_file = tempfile::NamedTempFile::new().unwrap();
    let summary = image::build_image(&infos, iso_file.path()).unwrap();
    assert_eq!(summary.layouts.len(), 2);
    assert_eq!(summary.layouts[0].frames, 1);
    assert_eq!(summary.layouts[1].frames, 1);

    let mut iso = File::open(iso_file.path()).unwrap();

    // --- master TOC: SACDMTOC + 8x SACDText + SACD_Man (LBA 510..519) ---
    let m = sec(&mut iso, 510);
    assert_eq!(&m[0..8], b"SACDMTOC");
    assert_eq!(m[8], 1);
    assert_eq!(m[9], 0x14);
    let area1_start = be32(&m, 0x40);
    let area1_size = be16(&m, 0x54);
    assert_eq!(area1_start, 544);
    assert_eq!(area1_size, 40);
    for i in 1..9u32 {
        assert_eq!(&sec(&mut iso, 510 + i)[0..8], b"SACDText");
    }
    assert_eq!(&sec(&mut iso, 519)[0..8], b"SACD_Man");

    // --- area TOC: scan its 40 sectors for the track lists ---
    let mut trl1 = None;
    let mut trl2 = None;
    for i in 0..area1_size as u32 {
        let s = sec(&mut iso, area1_start + i);
        match &s[0..8] {
            b"SACDTRL1" => trl1 = Some(s),
            b"SACDTRL2" => trl2 = Some(s),
            _ => {}
        }
    }
    let trl1 = trl1.expect("SACDTRL1 missing");
    let trl2 = trl2.expect("SACDTRL2 missing");
    let a = sec(&mut iso, area1_start);
    assert_eq!(&a[0..8], b"TWOCHTOC");
    assert_eq!(a[0x20], 2); // channel_count
    assert_eq!(a[0x45], 2); // track_count
    let track_start = be32(&a, 0x48);
    assert_eq!(track_start, image::AUDIO_START_LBA);

    let t0_start = be32(&trl1, 8);
    let t0_len = be32(&trl1, 8 + 255 * 4);
    let t1_start = be32(&trl1, 12);
    assert_eq!(t0_start, image::AUDIO_START_LBA);
    assert_eq!(t0_len, 5); // 1 frame = 5 sectors
    assert_eq!(t1_start, image::AUDIO_START_LBA + 5);

    // --- walk every audio sector of track 0 like process_frames ---
    let mut payload = Vec::new();
    let mut frames = 0u64;
    let mut cur = t0_start;
    let end = t0_start + t0_len;
    while cur < end {
        let s = sec(&mut iso, cur);
        let h = s[0];
        assert_eq!(h & 1, 0, "unexpected DST encoding"); // we emit plain DSD
        let fic = (h >> 2) & 7;
        let pic = (h >> 5) & 7;
        let mut pos = 1usize;
        for _ in 0..pic {
            let b0 = s[pos];
            let b1 = s[pos + 1];
            let frame_start = (b0 >> 7) & 1;
            let data_type = (b0 >> 3) & 7;
            let len = (((b0 & 7) as usize) << 8) | b1 as usize;
            pos += 2;
            assert_eq!(data_type, 2, "audio packet expected");
            if frame_start == 1 {
                frames += 1;
                assert!(fic >= 1, "frame_start without frame info");
                let tc = (s[pos], s[pos + 1], s[pos + 2]);
                let e = frames - 1;
                assert_eq!(
                    tc,
                    ((e / 75 / 60) as u8, ((e / 75) % 60) as u8, (e % 75) as u8)
                );
                pos += 3;
            }
            payload.extend_from_slice(&s[pos..pos + len]);
            pos += len;
        }
        cur += 1;
    }
    assert_eq!(frames, 1);
    assert_eq!(payload.len(), sacd::FRAME_DATA_BYTES);
    // fixture data chunk (1 block pair = 8192 bytes) is 0xAB; the frame is
    // then zero-padded up to FRAME_DATA_BYTES
    assert!(
        payload[..8192].iter().all(|&b| b == 0xAB),
        "payload mismatch head={:02x?} tail={:02x?}",
        &payload[..8],
        &payload[8184..8192]
    );
    assert!(
        payload[8192..].iter().all(|&b| b == 0),
        "padding must be zero"
    );

    // TRL2 duration of track 0 = 1 frame = (0,0,1)
    let dur = &trl2[8 + 255 * 4..8 + 255 * 4 + 4];
    assert_eq!(&dur[..3], &[0, 0, 1]);
}

fn make_dsf(samples: u64) -> Vec<u8> {
    // Reuse the lib testutil through a tiny local copy: build via dsf constants.
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
    buf.extend_from_slice(&(block as u32).to_le_bytes());
    buf.extend_from_slice(&0u32.to_le_bytes());
    buf.extend_from_slice(b"data");
    buf.extend_from_slice(&data_size.to_le_bytes());
    buf.resize((92 + data_size) as usize, 0xAB);
    let total = buf.len() as u64;
    buf[total_pos..total_pos + 8].copy_from_slice(&total.to_le_bytes());
    buf
}
