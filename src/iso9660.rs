//! Minimal ISO 9660 (level 2) structures, 2048-byte sectors, both-endian fields.
//!
//! Primary Volume Descriptor (type 1) layout:
//! - 0x00  1   type = 1
//! - 0x01  5   "CD001"
//! - 0x06  1   version = 1
//! - 0x08  32  system identifier
//! - 0x28  32  volume identifier
//! - 0x50  8   volume space size (both-endian u32)
//! - 0x78  2   volume set size (both-endian u16)
//! - 0x7C  2   volume sequence number
//! - 0x80  2   logical block size = 2048
//! - 0x84  8   path table size (both-endian u32)
//! - 0x8C  4   L path table LBA (LE)
//! - 0x90  4   optional L path table (0)
//! - 0x94  4   M path table LBA (BE)
//! - 0x98  4   optional M path table (0)
//! - 0x9C  34  root directory record
//! - ...     identifiers, dates (17-byte ASCII timestamps), etc.

use crate::udf::SECTOR;

pub const PVD_LBA: u32 = 16;
pub const TERMINATOR_LBA: u32 = 17;

pub fn both_u16(v: u16) -> [u8; 4] {
    let mut b = [0u8; 4];
    b[0..2].copy_from_slice(&v.to_le_bytes());
    b[2..4].copy_from_slice(&v.to_be_bytes());
    b
}

pub fn both_u32(v: u32) -> [u8; 8] {
    let mut b = [0u8; 8];
    b[0..4].copy_from_slice(&v.to_le_bytes());
    b[4..8].copy_from_slice(&v.to_be_bytes());
    b
}

pub fn both_u64(v: u64) -> [u8; 16] {
    let mut b = [0u8; 16];
    b[0..8].copy_from_slice(&v.to_le_bytes());
    b[8..16].copy_from_slice(&v.to_be_bytes());
    b
}

fn a_string(s: &str, len: usize) -> Vec<u8> {
    let mut v = vec![b' '; len];
    let bytes = s.as_bytes();
    let n = bytes.len().min(len);
    v[..n].copy_from_slice(&bytes[..n]);
    v
}

/// 17-byte ISO 9660 ASCII date-time: "YYYYMMDDHHMMSScc" + signed tz byte.
pub fn iso_date(year: u16, month: u8, day: u8) -> [u8; 17] {
    let s = format!("{year:04}{month:02}{day:02}00000000");
    let mut d = [b'0'; 17];
    d[..16].copy_from_slice(s.as_bytes());
    d[16] = 0;
    d
}

/// 7-byte directory record date: years since 1900, month, day, h, m, s, tz.
pub fn dir_date(year: u16, month: u8, day: u8) -> [u8; 7] {
    [(year - 1900) as u8, month, day, 0, 0, 0, 0]
}

/// ISO 9660 directory record.
pub fn dir_record(extent_lba: u32, data_len: u32, is_dir: bool, id: &[u8]) -> Vec<u8> {
    let mut r = vec![0u8; 33 + id.len() + (id.len() % 2)];
    r[1] = 0; // ext attr length
    r[2..10].copy_from_slice(&both_u32(extent_lba));
    r[10..26].copy_from_slice(&both_u64(data_len as u64));
    r[18..25].copy_from_slice(&dir_date(2026, 1, 1));
    r[25] = if is_dir { 0x02 } else { 0x00 };
    r[26] = 0;
    r[27] = 0;
    r[28..32].copy_from_slice(&both_u16(1));
    r[32] = id.len() as u8;
    r[33..33 + id.len()].copy_from_slice(id);
    r[0] = r.len() as u8;
    r
}

/// One path table record (little-endian table variant).
pub fn path_record_le(extent_lba: u32, parent: u16, id: &[u8]) -> Vec<u8> {
    let mut r = vec![0u8; 8 + id.len() + (id.len() % 2)];
    r[0] = id.len() as u8;
    r[1] = 0;
    r[2..6].copy_from_slice(&extent_lba.to_le_bytes());
    r[6..8].copy_from_slice(&parent.to_le_bytes());
    r[8..8 + id.len()].copy_from_slice(id);
    r
}

/// One path table record (big-endian "M" table variant).
pub fn path_record_be(extent_lba: u32, parent: u16, id: &[u8]) -> Vec<u8> {
    let mut r = vec![0u8; 8 + id.len() + (id.len() % 2)];
    r[0] = id.len() as u8;
    r[1] = 0;
    r[2..6].copy_from_slice(&extent_lba.to_be_bytes());
    r[6..8].copy_from_slice(&parent.to_be_bytes());
    r[8..8 + id.len()].copy_from_slice(id);
    r
}

/// Primary Volume Descriptor sector.
pub fn pvd(
    volume_sectors: u32,
    path_table_size: u32,
    l_path_lba: u32,
    m_path_lba: u32,
    root_record: &[u8],
) -> Vec<u8> {
    let mut s = vec![0u8; SECTOR];
    s[0] = 1;
    s[1..6].copy_from_slice(b"CD001");
    s[6] = 1;
    s[8..40].copy_from_slice(&a_string("SACD-AUTHOR", 32));
    s[40..72].copy_from_slice(&a_string("SACD", 32));
    s[80..88].copy_from_slice(&both_u32(volume_sectors));
    s[120..124].copy_from_slice(&both_u16(1));
    s[124..128].copy_from_slice(&both_u16(1));
    s[128..132].copy_from_slice(&both_u16(SECTOR as u16));
    s[132..140].copy_from_slice(&both_u32(path_table_size));
    s[140..144].copy_from_slice(&l_path_lba.to_le_bytes());
    s[148..152].copy_from_slice(&m_path_lba.to_be_bytes());
    s[156..156 + root_record.len()].copy_from_slice(root_record);
    s[190..318].copy_from_slice(&a_string("SACD-AUTHOR", 128));
    s[318..446].copy_from_slice(&a_string("SACD-AUTHOR", 128));
    s[446..574].copy_from_slice(&a_string("SACD-AUTHOR", 128));
    s[574..702].copy_from_slice(&a_string("SACD-AUTHOR ISO9660/UDF HYBRID", 128));
    s[813..830].copy_from_slice(&iso_date(2026, 1, 1));
    s[830..847].copy_from_slice(&iso_date(2026, 1, 1));
    s[864..881].copy_from_slice(&iso_date(2026, 1, 1));
    s[881] = 1;
    s
}

/// Volume descriptor set terminator (type 255).
pub fn terminator() -> Vec<u8> {
    let mut s = vec![0u8; SECTOR];
    s[0] = 255;
    s[1..6].copy_from_slice(b"CD001");
    s[6] = 1;
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pvd_layout() {
        let root = dir_record(20, 2048, true, &[0]);
        let p = pvd(1000, 20, 18, 19, &root);
        assert_eq!(p.len(), SECTOR);
        assert_eq!(p[0], 1);
        assert_eq!(&p[1..6], b"CD001");
        assert_eq!(&p[40..44], b"SACD");
        assert_eq!(&p[80..84], &1000u32.to_le_bytes());
        assert_eq!(&p[84..88], &1000u32.to_be_bytes());
        assert_eq!(&p[128..130], &2048u16.to_le_bytes());
        assert_eq!(&p[140..144], &18u32.to_le_bytes());
        assert_eq!(&p[148..152], &19u32.to_be_bytes());
        assert_eq!(p[156], root.len() as u8);
        assert_eq!(p[881], 1);
    }

    #[test]
    fn dir_record_both_endian() {
        let r = dir_record(510, 20480, false, b"MASTER1.TOC;1");
        assert_eq!(r[0] as usize, r.len());
        assert_eq!(&r[2..6], &510u32.to_le_bytes());
        assert_eq!(&r[6..10], &510u32.to_be_bytes());
        assert_eq!(&r[10..18], &20480u64.to_le_bytes());
        assert_eq!(r[25], 0);
        assert_eq!(r[32], 13);
        assert_eq!(&r[33..], b"MASTER1.TOC;1\0");
    }

    #[test]
    fn path_records_le_be() {
        let l = path_record_le(20, 1, &[0]);
        let b = path_record_be(20, 1, &[0]);
        assert_eq!(&l[2..6], &20u32.to_le_bytes());
        assert_eq!(&b[2..6], &20u32.to_be_bytes());
        assert_eq!(l.len(), b.len());
    }
}
