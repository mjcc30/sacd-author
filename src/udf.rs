//! Minimal UDF 1.02 structures (ECMA-167), little-endian, 2048-byte sectors.
//! Only what a DVD player (e.g. Sony BDP-S490) needs to browse SACD/:
//! Anchor, PVD, Partition Descriptor, Logical Volume Descriptor, Terminating
//! Descriptor, File Set Descriptor, File Entries and File Identifier
//! Descriptors.
//!
//! Every descriptor starts with a 16-byte tag (ECMA-167 4/7.2):
//! - 0x00  2  tag_identifier (LE)
//! - 0x02  2  descriptor_version (LE)
//! - 0x04  1  tag_checksum = sum of the 16 bytes, excluding byte 4, mod 256
//! - 0x05  1  reserved
//! - 0x06  2  tag_serial_number (LE)
//! - 0x08  2  descriptor_crc (CRC-CCITT 0x1021, init 0, over bytes 16..)
//! - 0x0A  2  descriptor_crc_length (LE)
//! - 0x0C  4  tag_location (LE, LBA of the sector holding this descriptor)

pub const SECTOR: usize = 2048;

pub mod tag_id {
    pub const PRIMARY_VOLUME: u16 = 1;
    pub const ANCHOR: u16 = 2;
    pub const PARTITION: u16 = 5;
    pub const LOGICAL_VOLUME: u16 = 6;
    pub const TERMINATING: u16 = 8;
    pub const FILE_SET: u16 = 256;
    pub const FILE_IDENTIFIER: u16 = 257;
    pub const FILE_ENTRY: u16 = 261;
}

/// CRC-CCITT (polynomial 0x1021, init 0, no reflection) as used by UDF tags.
pub fn crc16(data: &[u8]) -> u16 {
    let mut crc = 0u16;
    for &b in data {
        crc ^= (b as u16) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 != 0 {
                (crc << 1) ^ 0x1021
            } else {
                crc << 1
            };
        }
    }
    crc
}

/// Build a 16-byte descriptor tag for a full-sector descriptor at `location`.
pub fn make_tag(id: u16, version: u16, location: u32, body: &[u8]) -> [u8; 16] {
    let crc = crc16(body);
    let mut t = [0u8; 16];
    t[0..2].copy_from_slice(&id.to_le_bytes());
    t[2..4].copy_from_slice(&version.to_le_bytes());
    t[5] = 0;
    t[6..8].copy_from_slice(&1u16.to_le_bytes());
    t[8..10].copy_from_slice(&crc.to_le_bytes());
    t[10..12].copy_from_slice(&(body.len() as u16).to_le_bytes());
    t[12..16].copy_from_slice(&location.to_le_bytes());
    let sum: u8 = t[0..4]
        .iter()
        .chain(t[5..16].iter())
        .copied()
        .fold(0u8, |a, b| a.wrapping_add(b));
    t[4] = sum;
    t
}

/// Finalize a descriptor body into a full 2048-byte sector with its tag.
pub fn sector(id: u16, version: u16, location: u32, mut body: Vec<u8>) -> Vec<u8> {
    body.resize(SECTOR - 16, 0);
    let tag = make_tag(id, version, location, &body);
    let mut out = Vec::with_capacity(SECTOR);
    out.extend_from_slice(&tag);
    out.extend_from_slice(&body);
    out
}

/// EntityID: 32 bytes (flags u8, identifier[23], identifier_suffix[8]).
pub fn entity_id(s: &str) -> [u8; 32] {
    let mut e = [0u8; 32];
    e[0] = 0;
    let bytes = s.as_bytes();
    let n = bytes.len().min(23);
    e[1..1 + n].copy_from_slice(&bytes[..n]);
    e
}

/// UDF compressed dstring of fixed size `size` (1 length byte + chars, padded).
pub fn dstring(s: &str, size: usize) -> Vec<u8> {
    let mut v = vec![0u8; size];
    let bytes = s.as_bytes();
    let n = bytes.len().min(size - 1);
    v[0] = n as u8;
    v[1..1 + n].copy_from_slice(&bytes[..n]);
    v
}

/// UDF timestamp: 12 bytes (type/tz u16, year u16, month..microsecond u8 x8).
pub fn timestamp(year: u16, month: u8, day: u8) -> [u8; 12] {
    let mut t = [0u8; 12];
    t[0..2].copy_from_slice(&0x1000u16.to_le_bytes());
    t[2..4].copy_from_slice(&year.to_le_bytes());
    t[4] = month;
    t[5] = day;
    t
}

/// short_ad (8 bytes): extent length (top 2 bits = type, 0 = recorded) + LBA.
pub fn short_ad(length_bytes: u32, lba: u32) -> [u8; 8] {
    let mut a = [0u8; 8];
    a[0..4].copy_from_slice(&length_bytes.to_le_bytes());
    a[4..8].copy_from_slice(&lba.to_le_bytes());
    a
}

/// long_ad (16 bytes): length u32, LBA u32, partition u16, impl use[6].
pub fn long_ad(length_bytes: u32, lba: u32, partition: u16) -> [u8; 16] {
    let mut a = [0u8; 16];
    a[0..4].copy_from_slice(&length_bytes.to_le_bytes());
    a[4..8].copy_from_slice(&lba.to_le_bytes());
    a[8..10].copy_from_slice(&partition.to_le_bytes());
    a
}

/// Anchor Volume Descriptor Pointer (tag 2) at LBA 256.
/// Points at the main and reserve volume descriptor sequences.
pub fn anchor(
    main_vds_lba: u32,
    main_vds_len: u32,
    reserve_vds_lba: u32,
    reserve_vds_len: u32,
) -> Vec<u8> {
    let mut body = Vec::with_capacity(SECTOR - 16);
    body.extend_from_slice(&main_vds_len.to_le_bytes());
    body.extend_from_slice(&main_vds_lba.to_le_bytes());
    body.extend_from_slice(&reserve_vds_len.to_le_bytes());
    body.extend_from_slice(&reserve_vds_lba.to_le_bytes());
    sector(tag_id::ANCHOR, 2, 256, body)
}

/// UDF Primary Volume Descriptor (tag 1).
pub fn primary_volume(sequence: u32, volume_id: &str) -> Vec<u8> {
    let mut b = vec![0u8; SECTOR - 16];
    b[0..4].copy_from_slice(&sequence.to_le_bytes());
    b[4] = 0; // DescriptorCharacterSet type: 0 = OSTA compressed unicode
    let mut off = 20 - 16;
    let lv = dstring("", 128);
    b[off..off + 128].copy_from_slice(&lv);
    off += 128;
    let v = dstring(volume_id, 32);
    b[off..off + 32].copy_from_slice(&v);
    off += 32;
    b[off..off + 2].copy_from_slice(&1u16.to_le_bytes()); // volume sequence number
    off += 2;
    b[off..off + 2].copy_from_slice(&1u16.to_le_bytes()); // max volume sequence number
    off += 2;
    b[off..off + 2].copy_from_slice(&2u16.to_le_bytes()); // interchange level
    off += 2;
    b[off..off + 2].copy_from_slice(&3u16.to_le_bytes()); // max interchange level
    off += 2;
    b[off..off + 4].copy_from_slice(&1u32.to_le_bytes()); // character set list
    off += 4;
    b[off..off + 4].copy_from_slice(&1u32.to_le_bytes()); // max character set list
    off += 4;
    let vs = dstring("SACD-AUTHOR", 128);
    b[off..off + 128].copy_from_slice(&vs);
    off += 128;
    b[off] = 0; // descriptor character set
    off += 64;
    b[off] = 0; // explanatory character set
    off += 64;
    // volume abstract / copyright: zero extents
    off += 8 + 8;
    let app = entity_id("*SACD-AUTHOR");
    b[off..off + 32].copy_from_slice(&app);
    off += 32;
    let ts = timestamp(2026, 1, 1);
    b[off..off + 12].copy_from_slice(&ts);
    off += 12;
    let impl_id = entity_id("*SACD-AUTHOR UDF");
    b[off..off + 32].copy_from_slice(&impl_id);
    off += 32;
    b[off..off + 64].copy_from_slice(&[0u8; 64]); // implementation use
    off += 64;
    b[off..off + 4].copy_from_slice(&0u32.to_le_bytes()); // predecessor location
    off += 4;
    b[off..off + 2].copy_from_slice(&0u16.to_le_bytes()); // flags
    sector(tag_id::PRIMARY_VOLUME, 1, 0, b)
}

/// Partition Descriptor (tag 5). Partition 0 starts at `start` with `len` sectors.
pub fn partition_descriptor(sequence: u32, start: u32, len: u32) -> Vec<u8> {
    let mut b = vec![0u8; SECTOR - 16];
    b[0..4].copy_from_slice(&sequence.to_le_bytes());
    b[4..6].copy_from_slice(&1u16.to_le_bytes()); // partition flags: allocated
    b[6..8].copy_from_slice(&0u16.to_le_bytes()); // partition number
    let contents = entity_id("+NSR02");
    b[8..40].copy_from_slice(&contents);
    b[40..168].copy_from_slice(&[0u8; 128]); // contents use
    b[168..172].copy_from_slice(&1u32.to_le_bytes()); // access type: read-only
    b[172..176].copy_from_slice(&start.to_le_bytes());
    b[176..180].copy_from_slice(&len.to_le_bytes());
    let impl_id = entity_id("*SACD-AUTHOR UDF");
    b[180..212].copy_from_slice(&impl_id);
    sector(tag_id::PARTITION, 2, 0, b)
}

/// Logical Volume Descriptor (tag 6). `fsd_lba` = File Set Descriptor location.
pub fn logical_volume(sequence: u32, fsd_lba: u32, volume_sectors: u32) -> Vec<u8> {
    let mut b = vec![0u8; SECTOR - 16];
    b[0..4].copy_from_slice(&sequence.to_le_bytes());
    b[4] = 0; // descriptor character set
    let off = 20 - 16;
    let lv = dstring("SACD", 128);
    b[off..off + 128].copy_from_slice(&lv);
    let off = off + 128;
    b[off..off + 4].copy_from_slice(&(SECTOR as u32).to_le_bytes()); // logical block size
    let off = off + 4;
    let domain = entity_id("*OSTA UDF Compliant");
    b[off..off + 32].copy_from_slice(&domain);
    let off = off + 32;
    let contents = long_ad(2048, fsd_lba, 0); // FSD: one sector at fsd_lba
    b[off..off + 16].copy_from_slice(&contents);
    let off = off + 16;
    b[off..off + 4].copy_from_slice(&6u32.to_le_bytes()); // map table length
    let off = off + 4;
    b[off..off + 4].copy_from_slice(&0u32.to_le_bytes()); // reserved
    let off = off + 4;
    b[off..off + 4].copy_from_slice(&1u32.to_le_bytes()); // number of partition maps
    let off = off + 4;
    let impl_id = entity_id("*SACD-AUTHOR UDF");
    b[off..off + 32].copy_from_slice(&impl_id);
    let off = off + 32;
    b[off..off + 128].copy_from_slice(&[0u8; 128]); // implementation use
    let off = off + 128;
    b[off..off + 4].copy_from_slice(&0u32.to_le_bytes()); // integrity seq extent length
    b[off + 4..off + 8].copy_from_slice(&0u32.to_le_bytes()); // integrity seq extent location
    let off = off + 8;
    // partition map type 1: byte type=1, byte length=6, u16 vol seq, u16 partition
    b[off] = 1;
    b[off + 1] = 6;
    b[off + 2..off + 4].copy_from_slice(&1u16.to_le_bytes());
    b[off + 4..off + 6].copy_from_slice(&0u16.to_le_bytes());
    let _ = volume_sectors;
    sector(tag_id::LOGICAL_VOLUME, 2, 0, b)
}

/// Terminating Descriptor (tag 8).
pub fn terminating(location: u32) -> Vec<u8> {
    sector(tag_id::TERMINATING, 1, location, vec![])
}

/// File Set Descriptor (tag 256) pointing at the root directory ICB.
pub fn file_set_descriptor(fsd_lba: u32, root_icb_lba: u32) -> Vec<u8> {
    let mut b = vec![0u8; SECTOR - 16];
    let ts = timestamp(2026, 1, 1);
    b[0..12].copy_from_slice(&ts);
    b[12..14].copy_from_slice(&3u16.to_le_bytes()); // interchange level
    b[14..16].copy_from_slice(&3u16.to_le_bytes()); // max interchange level
    b[16..20].copy_from_slice(&1u32.to_le_bytes()); // character set list
    b[20..24].copy_from_slice(&1u32.to_le_bytes()); // max character set list
    b[24..28].copy_from_slice(&0u32.to_le_bytes()); // flags
    let root = long_ad(2048, root_icb_lba, 0);
    b[28..44].copy_from_slice(&root);
    let domain = entity_id("*OSTA UDF Compliant");
    b[44..76].copy_from_slice(&domain);
    sector(tag_id::FILE_SET, 2, fsd_lba, b)
}

/// File Entry (tag 261). `is_dir` selects file_type 4 vs 5.
/// Allocation descriptors are short_ad: (length_bytes, lba) pairs.
pub fn file_entry(
    location: u32,
    is_dir: bool,
    information_length: u64,
    allocations: &[(u32, u32)],
) -> Vec<u8> {
    let mut b = vec![0u8; SECTOR - 16];
    // ICB tag (20 bytes)
    b[0..4].copy_from_slice(&0u32.to_le_bytes()); // prior recorded entries
    b[4..6].copy_from_slice(&4u16.to_le_bytes()); // strategy type 4
    b[6..8].copy_from_slice(&0u16.to_le_bytes()); // strategy parameter
    b[8..10].copy_from_slice(&0u16.to_le_bytes()); // max entries
    b[10] = 0; // reserved
    b[11] = if is_dir { 4 } else { 5 }; // file type
    b[12..28].copy_from_slice(&long_ad(0, 0, 0)); // parent ICB (unused)
    b[28..30].copy_from_slice(&0u16.to_le_bytes()); // flags: short_ad
    b[30..34].copy_from_slice(&0xFFFF_FFFFu32.to_le_bytes()); // uid
    b[34..38].copy_from_slice(&0xFFFF_FFFFu32.to_le_bytes()); // gid
    b[38..42].copy_from_slice(&0x0000_016Du32.to_le_bytes()); // permissions r-xr-xr-x
    b[42..44].copy_from_slice(&1u16.to_le_bytes()); // file link count
    b[44] = 0; // record format
    b[45] = 0; // record display attributes
    b[46..50].copy_from_slice(&0u32.to_le_bytes()); // record length
    b[50..58].copy_from_slice(&information_length.to_le_bytes());
    b[58..66].copy_from_slice(&(information_length / SECTOR as u64).to_le_bytes());
    let ts = timestamp(2026, 1, 1);
    b[66..78].copy_from_slice(&ts); // access time
    b[78..90].copy_from_slice(&ts); // modification time
    b[90..102].copy_from_slice(&ts); // attribute time
    b[102..106].copy_from_slice(&1u32.to_le_bytes()); // checkpoint
    b[106..122].copy_from_slice(&long_ad(0, 0, 0)); // extended attribute ICB
    b[122..138].copy_from_slice(&long_ad(0, 0, 0)); // stream directory ICB
    let impl_id = entity_id("*SACD-AUTHOR UDF");
    b[138..170].copy_from_slice(&impl_id);
    b[170..178].copy_from_slice(&(location as u64).to_le_bytes()); // unique id
    b[178..182].copy_from_slice(&0u32.to_le_bytes()); // length of extended attributes
    let ad_len = (allocations.len() * 8) as u32;
    b[182..186].copy_from_slice(&ad_len.to_le_bytes()); // length of allocation descriptors
    for (i, &(len, lba)) in allocations.iter().enumerate() {
        let off = 186 + i * 8;
        b[off..off + 8].copy_from_slice(&short_ad(len, lba));
    }
    sector(tag_id::FILE_ENTRY, 2, location, b)
}

/// File Identifier Descriptor (tag 257). `name` is None for "." / ".." codes.
pub fn file_identifier(
    location: u32,
    file_characteristics: u8,
    name: Option<&str>,
    icb_lba: u32,
) -> Vec<u8> {
    let mut b = vec![0u8; SECTOR - 16];
    b[0..2].copy_from_slice(&1u16.to_le_bytes()); // file version number
    b[2] = file_characteristics;
    let (id_bytes, len): (Vec<u8>, u8) = match name {
        None => (vec![0], 1), // caller uses 0x00 for "." / 0x01 for ".."
        Some(s) => (s.as_bytes().to_vec(), s.len() as u8),
    };
    let len = if name.is_some() { len } else { 1 };
    b[3] = len; // length of file identifier
    b[4..20].copy_from_slice(&long_ad(2048, icb_lba, 0)); // ICB
    b[20..22].copy_from_slice(&0u16.to_le_bytes()); // length of implementation use
    b[22..22 + len as usize].copy_from_slice(&id_bytes);
    // pad to 4-byte boundary
    let used = 22 + len as usize;
    let padded = used.div_ceil(4) * 4;
    let mut out = sector(tag_id::FILE_IDENTIFIER, 2, location, b);
    out.truncate(16 + padded);
    out.resize(SECTOR, 0);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc16_known_vector() {
        // CRC-CCITT (0x1021, init 0, no reflection = CRC-16/XMODEM) of "123456789"
        assert_eq!(crc16(b"123456789"), 0x31C3);
    }

    #[test]
    fn tag_checksum_and_crc() {
        let body = vec![0xAAu8; 100];
        let t = make_tag(tag_id::ANCHOR, 2, 256, &body);
        assert_eq!(&t[0..2], &2u16.to_le_bytes());
        assert_eq!(&t[12..16], &256u32.to_le_bytes());
        let sum: u8 = t[0..4]
            .iter()
            .chain(t[5..16].iter())
            .fold(0u8, |a, &b| a.wrapping_add(b));
        assert_eq!(t[4], sum);
        assert_eq!(&t[8..10], &crc16(&body).to_le_bytes());
        assert_eq!(&t[10..12], &100u16.to_le_bytes());
    }

    #[test]
    fn sector_is_2048_and_tag_valid() {
        let s = anchor(257, 4, 261, 4);
        assert_eq!(s.len(), SECTOR);
        assert_eq!(&s[0..2], &2u16.to_le_bytes());
        assert_eq!(&s[16..20], &4u32.to_le_bytes());
        assert_eq!(&s[20..24], &257u32.to_le_bytes());
        let sum: u8 = s[0..4]
            .iter()
            .chain(s[5..16].iter())
            .fold(0u8, |a, &b| a.wrapping_add(b));
        assert_eq!(s[4], sum);
    }

    #[test]
    fn file_entry_has_allocation_descriptors() {
        let fe = file_entry(300, false, 20480, &[(20480, 640)]);
        assert_eq!(fe.len(), SECTOR);
        assert_eq!(&fe[0..2], &261u16.to_le_bytes());
        assert_eq!(fe[16 + 11], 5); // regular file
        assert_eq!(&fe[16 + 50..16 + 58], &20480u64.to_le_bytes());
        assert_eq!(&fe[16 + 182..16 + 186], &8u32.to_le_bytes());
        assert_eq!(&fe[16 + 186..16 + 190], &20480u32.to_le_bytes());
        assert_eq!(&fe[16 + 190..16 + 194], &640u32.to_le_bytes());
    }

    #[test]
    fn fid_pads_to_4_bytes() {
        let fid = file_identifier(262, 0x02, Some("MASTER1.TOC"), 264);
        assert_eq!(fid.len(), SECTOR);
        assert_eq!(&fid[0..2], &257u16.to_le_bytes());
        assert_eq!(fid[16 + 3], 11);
        assert_eq!(&fid[16 + 22..16 + 33], b"MASTER1.TOC");
    }
}
