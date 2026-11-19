//! Plain ISO 9660 + Joliet data image builder: packages arbitrary files
//! (FLAC, WAV, ...) at the root of a 2048-byte-sector ISO image that any modern
//! player reads from DVD-R (e.g. Sony BDP-S490 plays FLAC 24/96 from disc).
//!
//! Layout (LBA):
//! - 16  PVD (ISO 9660, 8.3 mangled names)
//! - 17  SVD (Joliet, UCS-2BE names, escape "%/E") — preserves real file names
//! - 18  volume descriptor set terminator
//! - 19  L path table (PVD), 20  M path table (PVD)
//! - 21  L path table (SVD), 22  M path table (SVD)
//! - 23  root directory (PVD), then root directory (SVD), then file extents
//!
//! The tree is flat (root only), so each path table holds a single record.

use crate::iso9660::{a_string, dir_record, path_record_be, path_record_le, svd, terminator};
use crate::udf::SECTOR;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

pub const PVD_LBA: u32 = 16;
pub const SVD_LBA: u32 = 17;
pub const TERM_LBA: u32 = 18;
pub const PT_L_PVD_LBA: u32 = 19;
pub const PT_M_PVD_LBA: u32 = 20;
pub const PT_L_SVD_LBA: u32 = 21;
pub const PT_M_SVD_LBA: u32 = 22;
pub const ROOT_PVD_LBA: u32 = 23;

/// One file to place at the root of the image.
pub struct DataEntry {
    /// File name as it will appear on the disc (Joliet view).
    pub name: String,
    pub path: PathBuf,
}

fn is_d_char(c: char) -> bool {
    c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_'
}

/// Mangle a file name to ISO 9660 level 1 (8.3, uppercase, ";1" version).
pub fn mangle_iso9660(name: &str) -> String {
    let (base_full, ext_full) = match name.rsplit_once('.') {
        Some((b, e)) if !b.is_empty() => (b, e),
        _ => (name, ""),
    };
    let mangle_part = |p: &str, max: usize| -> String {
        if p.is_empty() {
            return String::new();
        }
        let mut out = String::with_capacity(max);
        for c in p.chars() {
            if out.len() >= max {
                break;
            }
            let c = c.to_ascii_uppercase();
            out.push(if is_d_char(c) { c } else { '_' });
        }
        if out.is_empty() {
            out.push('_');
        }
        out
    };
    let base = mangle_part(base_full, 8);
    let ext = mangle_part(ext_full, 3);
    if ext.is_empty() {
        format!("{base};1")
    } else {
        format!("{base}.{ext};1")
    }
}

/// Make mangled names unique by replacing the tail with "~N" before ";1".
fn dedupe(names: Vec<String>) -> Vec<String> {
    let mut seen: Vec<String> = Vec::new();
    names
        .into_iter()
        .map(|n| {
            let mut candidate = n;
            let mut i = 1u32;
            while seen.contains(&candidate) {
                let stem = candidate.split(';').next().unwrap_or("");
                let suffix = format!("~{i};1");
                let keep = stem.len().saturating_sub(suffix.len());
                candidate = format!("{}{suffix}", &stem[..keep]);
                i += 1;
            }
            seen.push(candidate.clone());
            candidate
        })
        .collect()
}

/// Encode a name for the Joliet view as UCS-2 big-endian. Joliet keeps the
/// '.' separator as the 16-bit code point U+002E and has no ";1" suffix;
/// names are limited to 64 UCS-2 units (128 bytes).
fn joliet_id(name: &str) -> Vec<u8> {
    let cleaned: String = name
        .chars()
        .filter(|&c| c != '/' && c != '\\' && (c as u32) >= 0x20)
        .collect();
    let units: Vec<u16> = cleaned.encode_utf16().collect();
    let units = &units[..units.len().min(64)];
    let mut out = Vec::with_capacity(units.len() * 2);
    for u in units {
        out.extend_from_slice(&u.to_be_bytes());
    }
    out
}

/// Joliet ids, de-duplicated by appending "~N" before the extension.
fn joliet_ids(names: &[String]) -> Vec<Vec<u8>> {
    let mut seen: Vec<Vec<u8>> = Vec::new();
    names
        .iter()
        .map(|n| {
            let mut id = joliet_id(n);
            let mut i = 1u32;
            while seen.contains(&id) {
                let suffix: Vec<u16> = format!("~{i}").encode_utf16().collect();
                let (base, ext) = match n.rsplit_once('.') {
                    Some((b, e)) if !b.is_empty() => (b, Some(e)),
                    _ => (n.as_str(), None),
                };
                let mut units: Vec<u16> = base.encode_utf16().collect();
                if units.len() + suffix.len() > 64 {
                    units.truncate(64 - suffix.len());
                }
                units.extend_from_slice(&suffix);
                if let Some(ext) = ext {
                    units.push('.' as u16);
                    units.extend(ext.encode_utf16());
                }
                units.truncate(64);
                id = units.iter().flat_map(|u| u.to_be_bytes()).collect();
                i += 1;
            }
            seen.push(id.clone());
            id
        })
        .collect()
}

/// Pack directory records into whole 2048-byte sectors (records must not cross
/// a sector boundary). Returns (sector bytes, sector count).
fn pack_dir_sectors(records: &[Vec<u8>]) -> (Vec<u8>, usize) {
    let mut out = Vec::new();
    let mut cur = 0usize;
    for r in records {
        if cur + r.len() > SECTOR {
            out.resize(out.len() + (SECTOR - cur), 0);
            cur = 0;
        }
        out.extend_from_slice(r);
        cur += r.len();
    }
    if cur > 0 {
        out.resize(out.len() + (SECTOR - cur), 0);
    }
    let n = (out.len() / SECTOR).max(1);
    (out, n)
}

fn write_sector(file: &mut File, lba: u32, data: &[u8]) -> std::io::Result<()> {
    assert_eq!(data.len(), SECTOR);
    file.seek(SeekFrom::Start(lba as u64 * SECTOR as u64))?;
    file.write_all(data)
}

/// Volume id derived from the output file stem (uppercase d-characters, <= 16).
pub fn volume_id_from(path: &Path) -> String {
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("DATA");
    let mut v = String::new();
    for c in stem.chars() {
        let u = c.to_ascii_uppercase();
        if is_d_char(u) {
            v.push(u);
        }
        if v.len() >= 16 {
            break;
        }
    }
    if v.is_empty() {
        v.push_str("DATA");
    }
    v
}

/// Build a flat ISO 9660 + Joliet image containing `entries` at the root.
/// Returns the total number of 2048-byte sectors.
pub fn build_data_iso(
    entries: &[DataEntry],
    out_path: &Path,
    volume_id: &str,
) -> std::io::Result<u32> {
    if entries.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "no input files",
        ));
    }
    let mut sizes = Vec::with_capacity(entries.len());
    for e in entries {
        let sz = std::fs::metadata(&e.path)?.len();
        if sz > u32::MAX as u64 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("{}: file larger than 4 GiB-1 is not representable", e.name),
            ));
        }
        sizes.push(sz);
    }

    let pvd_names = dedupe(entries.iter().map(|e| mangle_iso9660(&e.name)).collect());
    let joliet_names: Vec<String> = entries.iter().map(|e| e.name.clone()).collect();
    let joliet_ids = joliet_ids(&joliet_names);

    // Size the root directories first (record lengths don't depend on LBAs).
    let pvd_dir_sectors = {
        let mut recs = vec![
            dir_record(ROOT_PVD_LBA, SECTOR as u32, true, &[0]),
            dir_record(ROOT_PVD_LBA, SECTOR as u32, true, &[1]),
        ];
        for (i, name) in pvd_names.iter().enumerate() {
            recs.push(dir_record(0, sizes[i] as u32, false, name.as_bytes()));
        }
        pack_dir_sectors(&recs).1
    };
    let svd_root_lba = ROOT_PVD_LBA + pvd_dir_sectors as u32;
    let svd_dir_sectors = {
        let mut recs = vec![
            dir_record(svd_root_lba, SECTOR as u32, true, &[0]),
            dir_record(svd_root_lba, SECTOR as u32, true, &[1]),
        ];
        for (i, id) in joliet_ids.iter().enumerate() {
            recs.push(dir_record(0, sizes[i] as u32, false, id));
        }
        pack_dir_sectors(&recs).1
    };
    let first_file_lba = svd_root_lba + svd_dir_sectors as u32;

    let mut extents = Vec::with_capacity(entries.len());
    let mut next = first_file_lba;
    for &sz in &sizes {
        extents.push((next, sz));
        next += sz.div_ceil(SECTOR as u64) as u32;
    }
    let total_sectors = next;

    // --- root directory records with real LBAs, sorted by PVD identifier ---
    let pvd_root_size = (pvd_dir_sectors * SECTOR) as u32;
    let svd_root_size = (svd_dir_sectors * SECTOR) as u32;
    let mut order: Vec<usize> = (0..entries.len()).collect();
    order.sort_by(|&a, &b| pvd_names[a].cmp(&pvd_names[b]));
    let mut pvd_records = vec![
        dir_record(ROOT_PVD_LBA, pvd_root_size, true, &[0]),
        dir_record(ROOT_PVD_LBA, pvd_root_size, true, &[1]),
    ];
    let mut svd_records = vec![
        dir_record(svd_root_lba, svd_root_size, true, &[0]),
        dir_record(svd_root_lba, svd_root_size, true, &[1]),
    ];
    for i in order {
        pvd_records.push(dir_record(
            extents[i].0,
            sizes[i] as u32,
            false,
            pvd_names[i].as_bytes(),
        ));
        svd_records.push(dir_record(
            extents[i].0,
            sizes[i] as u32,
            false,
            &joliet_ids[i],
        ));
    }
    let (pvd_root_dir, _) = pack_dir_sectors(&pvd_records);
    let (svd_root_dir, _) = pack_dir_sectors(&svd_records);

    let path_table_size = 10u32; // one root record: 8 + 1 byte id + 1 pad
    let pvd_root_rec = dir_record(ROOT_PVD_LBA, pvd_root_size, true, &[0]);
    let svd_root_rec = dir_record(svd_root_lba, svd_root_size, true, &[0]);

    let mut pvd_sector = crate::iso9660::pvd(
        total_sectors,
        path_table_size,
        PT_L_PVD_LBA,
        PT_M_PVD_LBA,
        &pvd_root_rec,
    );
    pvd_sector[40..72].copy_from_slice(&a_string(volume_id, 32));

    let mut file = File::create(out_path)?;
    file.set_len(total_sectors as u64 * SECTOR as u64)?;

    write_sector(&mut file, PVD_LBA, &pvd_sector)?;
    write_sector(
        &mut file,
        SVD_LBA,
        &svd(
            volume_id,
            total_sectors,
            path_table_size,
            PT_L_SVD_LBA,
            PT_M_SVD_LBA,
            &svd_root_rec,
        ),
    )?;
    write_sector(&mut file, TERM_LBA, &terminator())?;

    for (lba, rec) in [
        (PT_L_PVD_LBA, path_record_le(ROOT_PVD_LBA, 1, &[0])),
        (PT_M_PVD_LBA, path_record_be(ROOT_PVD_LBA, 1, &[0])),
        (PT_L_SVD_LBA, path_record_le(svd_root_lba, 1, &[0])),
        (PT_M_SVD_LBA, path_record_be(svd_root_lba, 1, &[0])),
    ] {
        let mut s = vec![0u8; SECTOR];
        s[..rec.len()].copy_from_slice(&rec);
        write_sector(&mut file, lba, &s)?;
    }

    let mut root_pvd_padded = pvd_root_dir.clone();
    root_pvd_padded.resize(pvd_dir_sectors * SECTOR, 0);
    for i in 0..pvd_dir_sectors {
        write_sector(
            &mut file,
            ROOT_PVD_LBA + i as u32,
            &root_pvd_padded[i * SECTOR..(i + 1) * SECTOR],
        )?;
    }
    let mut svd_root_padded = svd_root_dir.clone();
    svd_root_padded.resize(svd_dir_sectors * SECTOR, 0);
    for i in 0..svd_dir_sectors {
        write_sector(
            &mut file,
            svd_root_lba + i as u32,
            &svd_root_padded[i * SECTOR..(i + 1) * SECTOR],
        )?;
    }

    // --- file data ---
    let mut buf = vec![0u8; SECTOR];
    for (i, e) in entries.iter().enumerate() {
        let (lba, size) = extents[i];
        file.seek(SeekFrom::Start(lba as u64 * SECTOR as u64))?;
        let mut src = File::open(&e.path)?;
        let mut remaining = size;
        while remaining > 0 {
            let want = (remaining as usize).min(SECTOR);
            src.read_exact(&mut buf[..want])?;
            for b in &mut buf[want..] {
                *b = 0;
            }
            file.write_all(&buf)?;
            remaining -= want as u64;
        }
    }
    file.flush()?;
    Ok(total_sectors)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mangles_to_8_3() {
        assert_eq!(mangle_iso9660("track.flac"), "TRACK.FLA;1");
        assert_eq!(
            mangle_iso9660("01. La Telaraña de Dylan.flac"),
            "01__LA_T.FLA;1"
        );
        assert_eq!(mangle_iso9660("noext"), "NOEXT;1");
    }

    #[test]
    fn dedupe_collisions() {
        let v = dedupe(vec![
            mangle_iso9660("track one.flac"),
            mangle_iso9660("track_one.flac"),
        ]);
        assert_ne!(v[0], v[1]);
        assert!(v[0].ends_with(";1") && v[1].ends_with(";1"));
    }

    #[test]
    fn joliet_id_is_ucs2be() {
        let id = joliet_id("AB.flac");
        assert_eq!(
            id,
            [
                0x00, 0x41, 0x00, 0x42, 0x00, 0x2E, 0x00, 0x66, 0x00, 0x6C, 0x00, 0x61, 0x00, 0x63
            ]
        );
        let long = "x".repeat(200) + ".flac";
        assert_eq!(joliet_id(&long).len(), 128);
    }

    #[test]
    fn pack_dir_sectors_never_crosses_boundary() {
        let rec = vec![0xAAu8; 2000];
        let (data, n) = pack_dir_sectors(&[rec.clone(), rec.clone()]);
        assert_eq!(n, 2);
        assert_eq!(data.len(), 2 * SECTOR);
        assert_eq!(&data[..2000], &rec[..]);
        assert!(data[2000..SECTOR].iter().all(|&b| b == 0));
    }
}
