//! Multichannel SACD area (MULCHTOC/MC_Audio) reading: build a synthetic ISO
//! with a 4-channel MC area, read it back, write DFF, and convert to PCM.

use sacd_author::{dsf_write, image, iso_read, sacd};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

const SECTOR: usize = sacd::SECTOR_SIZE; // 2048
const MASTER_LBA: u32 = 510;
const AREA2_LBA: u32 = 624;
const AREA2_SECTORS: u32 = 40;
const MC_AUDIO_LBA: u32 = AREA2_LBA + AREA2_SECTORS; // 664

fn write_sec(f: &mut File, lba: u32, data: &[u8]) {
    assert_eq!(data.len(), SECTOR);
    f.seek(SeekFrom::Start(lba as u64 * SECTOR as u64)).unwrap();
    f.write_all(data).unwrap();
}

fn pattern(n: usize, seed: u8) -> Vec<u8> {
    (0..n)
        .map(|i| (i as u8).wrapping_mul(37).wrapping_add(seed))
        .collect()
}

/// Build a minimal ISO containing only a multichannel area:
/// master TOC at 510 (area_2 pointers), MULCHTOC + SACDTRL1 + SACDTTxt at
/// AREA2_LBA, MC audio sectors at MC_AUDIO_LBA.
/// `tracks`: (title, interleaved native-bit-order payload).
/// Returns (start_lba, sector_count) per track.
fn build_mc_iso(path: &Path, channels: u8, tracks: &[(&str, Vec<u8>)]) -> Vec<(u32, u32)> {
    let mut f = File::create(path).unwrap();

    // master TOC sector: area_2_toc_1_start @0x48, area_2_toc_size @0x56
    let mut m = vec![0u8; SECTOR];
    m[0..8].copy_from_slice(b"SACDMTOC");
    m[0x40..0x44].copy_from_slice(&544u32.to_be_bytes()); // area_1 (unused here)
    m[0x48..0x4C].copy_from_slice(&AREA2_LBA.to_be_bytes());
    m[0x4C..0x50].copy_from_slice(&0u32.to_be_bytes()); // area_2 copy 2
    m[0x54..0x56].copy_from_slice(&40u16.to_be_bytes());
    m[0x56..0x58].copy_from_slice(&(AREA2_SECTORS as u16).to_be_bytes());
    write_sec(&mut f, MASTER_LBA, &m);

    // MULCHTOC header sector
    let mut a = vec![0u8; SECTOR];
    a[0..8].copy_from_slice(b"MULCHTOC");
    a[0x0A..0x0C].copy_from_slice(&40u16.to_be_bytes()); // size
    a[0x20] = channels;
    a[0x45] = tracks.len() as u8;
    write_sec(&mut f, AREA2_LBA, &a);

    // SACDTRL1: starts @8+i*4, lengths @8+255*4+i*4
    let mut trl = vec![0u8; SECTOR];
    trl[0..8].copy_from_slice(b"SACDTRL1");

    // SACDTTxt at area sector 3: pointer table @8+i*2, entries at 0x100+
    let mut ttxt = vec![0u8; SECTOR];
    ttxt[0..8].copy_from_slice(b"SACDTTxt");
    for (i, (title, _)) in tracks.iter().enumerate() {
        let pos = 0x100 + i * 0x40;
        ttxt[8 + i * 2..8 + i * 2 + 2].copy_from_slice(&(pos as u16).to_be_bytes());
        let mut p = pos;
        ttxt[p] = 1; // amount
        p += 4; // pos+1..pos+3 skipped by the reader
        ttxt[p] = 0x01; // title type
        ttxt[p + 1] = 0x20;
        p += 2;
        ttxt[p..p + title.len()].copy_from_slice(title.as_bytes());
        ttxt[p + title.len()] = 0;
    }
    write_sec(&mut f, AREA2_LBA + 3, &ttxt);

    // MC audio sectors
    let mut layouts = Vec::new();
    let mut next = MC_AUDIO_LBA;
    for (_, payload) in tracks {
        f.seek(SeekFrom::Start(next as u64 * SECTOR as u64))
            .unwrap();
        let sectors = sacd::pack_interleaved_stream(payload, channels as usize, &mut f).unwrap();
        layouts.push((next, sectors));
        next += sectors;
    }
    for (i, (start, len)) in layouts.iter().enumerate() {
        trl[8 + i * 4..8 + i * 4 + 4].copy_from_slice(&start.to_be_bytes());
        trl[8 + 255 * 4 + i * 4..8 + 255 * 4 + i * 4 + 4].copy_from_slice(&len.to_be_bytes());
    }
    write_sec(&mut f, AREA2_LBA + 1, &trl);

    let total = next;
    f.set_len(total as u64 * SECTOR as u64).unwrap();
    f.flush().unwrap();
    layouts
}

fn mc_payload(frames: usize, channels: usize, seed: u8) -> Vec<u8> {
    pattern(frames * sacd::frame_data_bytes(channels), seed)
}

#[test]
fn mc_area_reads_back_bit_exact() {
    let iso = tempfile::Builder::new().suffix(".iso").tempfile().unwrap();
    let ch = 4u8;
    let p1 = mc_payload(2, ch as usize, 11); // 2 frames = 37632 bytes
    let p2 = mc_payload(1, ch as usize, 77); // 1 frame
    let layouts = build_mc_iso(
        iso.path(),
        ch,
        &[("Alpha", p1.clone()), ("Beta", p2.clone())],
    );
    let img = iso_read::read_iso_area(iso.path(), iso_read::Area::MultiChannel).unwrap();
    assert_eq!(img.channels, 4);
    assert_eq!(img.tracks.len(), 2);
    for (i, (p, (start, len))) in [&p1, &p2].iter().zip(&layouts).enumerate() {
        let t = &img.tracks[i];
        assert_eq!(t.channels, 4);
        assert_eq!(t.layout, iso_read::DataLayout::McRaw);
        assert_eq!(t.start_lba, *start);
        assert_eq!(t.sector_count, *len);
        // payload is zero-padded to whole 1/75s frames
        let frame_len = sacd::frame_data_bytes(4);
        let padded = p.len().div_ceil(frame_len) * frame_len;
        assert_eq!(t.data.len(), padded);
        assert_eq!(&t.data[..p.len()], &p[..]);
        assert!(t.data[p.len()..].iter().all(|&b| b == 0));
    }
    assert_eq!(img.tracks[0].title.as_deref(), Some("Alpha"));
    assert_eq!(img.tracks[1].title.as_deref(), Some("Beta"));
}

#[test]
fn mc_area_to_dff_roundtrip() {
    let iso = tempfile::Builder::new().suffix(".iso").tempfile().unwrap();
    let p = mc_payload(2, 6, 5); // 6 channels
    build_mc_iso(iso.path(), 6, &[("Hex", p.clone())]);
    let img = iso_read::read_iso_area(iso.path(), iso_read::Area::MultiChannel).unwrap();
    let t = &img.tracks[0];
    assert_eq!(t.channels, 6);

    let dff = tempfile::NamedTempFile::new().unwrap();
    dsf_write::write_dff(
        dff.path(),
        &t.data,
        t.channels,
        t.title.as_deref(),
        None,
        None,
    )
    .unwrap();
    let bytes = std::fs::read(dff.path()).unwrap();
    let info = sacd_author::dff::parse_bytes(&bytes, dff.path()).unwrap();
    assert_eq!(info.channels, 6);
    assert_eq!(info.data_size as usize, t.data.len());
    assert_eq!(info.samples_per_channel, t.data.len() as u64 / 6 * 8);
    assert_eq!(
        &bytes[info.data_offset as usize..info.data_offset as usize + t.data.len()],
        &t.data[..]
    );
    assert_eq!(info.tags.title.as_deref(), Some("Hex"));
}

#[test]
fn mc_dff_converts_to_multichannel_wav() {
    let iso = tempfile::Builder::new().suffix(".iso").tempfile().unwrap();
    let p = mc_payload(1, 4, 42);
    build_mc_iso(iso.path(), 4, &[("Quad", p)]);
    let img = iso_read::read_iso_area(iso.path(), iso_read::Area::MultiChannel).unwrap();
    let t = &img.tracks[0];

    let dff = tempfile::Builder::new().suffix(".dff").tempfile().unwrap();
    dsf_write::write_dff(dff.path(), &t.data, t.channels, None, None, None).unwrap();
    let wav = tempfile::Builder::new().suffix(".wav").tempfile().unwrap();
    let params = sacd_author::dsd2pcm::PcmParams {
        rate: 96_000,
        bits: 24,
        lowpass: None,
        gain_db: 0.0,
    };
    let frames = sacd_author::dsd2pcm::convert_file(dff.path(), wav.path(), None, &params).unwrap();
    let b = std::fs::read(wav.path()).unwrap();
    assert_eq!(&b[0..4], b"RIFF");
    let ch = u16::from_le_bytes([b[22], b[23]]);
    assert_eq!(ch, 4);
    let dsize = u32::from_le_bytes([b[40], b[41], b[42], b[43]]) as usize;
    assert_eq!(dsize, frames as usize * 4 * 3);
    // 1 frame = 37632 samples/ch -> 37632*96000/2822400 = 1280 frames (+ tail)
    assert!((1280..1280 + 2000).contains(&frames), "frames {frames}");
}

#[test]
fn stereo_image_has_no_mc_area() {
    // build a normal stereo SACD-R image and ask for the MC area
    let samples = 32768u64;
    let block = 4096u64;
    let data_size = samples.div_ceil(block * 8) * block * 2;
    let mut dsf_bytes = Vec::new();
    dsf_bytes.extend_from_slice(b"DSD ");
    dsf_bytes.extend_from_slice(&(28 + 52 + 12 + data_size).to_le_bytes());
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
    dsf_bytes.extend_from_slice(&data_size.to_le_bytes());
    dsf_bytes.extend_from_slice(&vec![0x5Au8; data_size as usize]);
    let mut dsf_file = tempfile::NamedTempFile::new().unwrap();
    dsf_file.write_all(&dsf_bytes).unwrap();
    let info = sacd_author::dsf::parse_bytes(&dsf_bytes, dsf_file.path()).unwrap();
    let iso = tempfile::Builder::new().suffix(".iso").tempfile().unwrap();
    image::build_image(&[info], iso.path()).unwrap();

    let err = iso_read::read_iso_area(iso.path(), iso_read::Area::MultiChannel).unwrap_err();
    assert!(
        err.to_string().contains("no multichannel area"),
        "unexpected error: {err}"
    );
    // the 2C area still reads fine
    let img = iso_read::read_iso(iso.path()).unwrap();
    assert_eq!(img.channels, 2);
    assert_eq!(img.tracks.len(), 1);
    assert_eq!(img.tracks[0].layout, iso_read::DataLayout::Dsf);
}

#[test]
fn dst_compressed_sectors_are_rejected() {
    let iso = tempfile::Builder::new().suffix(".iso").tempfile().unwrap();
    let p = mc_payload(1, 4, 9);
    build_mc_iso(iso.path(), 4, &[("DST", p)]);
    // flip the dst bit (bit 0) of the first MC audio sector header
    let mut f = File::options()
        .read(true)
        .write(true)
        .open(iso.path())
        .unwrap();
    f.seek(SeekFrom::Start(MC_AUDIO_LBA as u64 * SECTOR as u64))
        .unwrap();
    let mut b = [0u8; 1];
    f.read_exact(&mut b).unwrap();
    b[0] |= 1;
    f.seek(SeekFrom::Start(MC_AUDIO_LBA as u64 * SECTOR as u64))
        .unwrap();
    f.write_all(&b).unwrap();
    drop(f);

    let err = iso_read::read_iso_area(iso.path(), iso_read::Area::MultiChannel).unwrap_err();
    assert!(err.to_string().contains("DST"), "unexpected error: {err}");
}

#[test]
fn cli_extract_mch_writes_multichannel_dff() {
    let iso = tempfile::Builder::new().suffix(".iso").tempfile().unwrap();
    let p = mc_payload(1, 4, 3);
    build_mc_iso(iso.path(), 4, &[("CLI MC", p)]);
    let out_dir = tempfile::tempdir().unwrap();

    let status = std::process::Command::new(env!("CARGO_BIN_EXE_sacd-author"))
        .args([
            "extract",
            iso.path().to_str().unwrap(),
            "-o",
            out_dir.path().to_str().unwrap(),
            "--mch",
            "-f",
            "dff",
        ])
        .status()
        .unwrap();
    assert!(status.success());
    let entries: Vec<_> = std::fs::read_dir(out_dir.path())
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    assert_eq!(entries.len(), 1);
    let bytes = std::fs::read(&entries[0]).unwrap();
    let info = sacd_author::dff::parse_bytes(&bytes, &entries[0]).unwrap();
    assert_eq!(info.channels, 4);
    assert_eq!(info.tags.title.as_deref(), Some("CLI MC"));

    // --mch without -f dff must fail with a clear error
    let out2 = tempfile::tempdir().unwrap();
    let status = std::process::Command::new(env!("CARGO_BIN_EXE_sacd-author"))
        .args([
            "extract",
            iso.path().to_str().unwrap(),
            "-o",
            out2.path().to_str().unwrap(),
            "--mch",
        ])
        .status()
        .unwrap();
    assert!(!status.success());
}
