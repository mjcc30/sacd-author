//! SACD-R hybrid ISO image assembler (ISO 9660 + UDF 1.02, 2048-byte sectors).
//!
//! Disc layout (LBA):
//! - 0..16     zeroed system area
//! - 16        ISO 9660 Primary Volume Descriptor
//! - 17        ISO 9660 volume descriptor set terminator
//! - 18        ISO L path table, 19: M path table
//! - 20        ISO root directory, 21: ISO SACD directory
//! - 22..256   zeros
//! - 256       UDF Anchor Volume Descriptor Pointer
//! - 257       UDF Primary Volume Descriptor
//! - 258       UDF Partition Descriptor
//! - 259       UDF Logical Volume Descriptor
//! - 260       UDF Terminating Descriptor
//! - 261       UDF File Set Descriptor
//! - 262       UDF File Entry: root directory
//! - 263       UDF File Entry: SACD directory
//! - 264..268  UDF File Entries: Master1.TOC, Master2.TOC, Master3.TOC, 2C_Audio
//! - 268       root directory File Identifier Descriptors
//! - 269       SACD directory File Identifier Descriptors
//! - 270..510  zeros
//! - 510..520  Master1.TOC (raw, first sector = SACDMTOC)  <- LBA 510 = 0xFF000
//! - 520..530  Master2.TOC
//! - 530..540  Master3.TOC
//! - 540..543  Area TOC copy 1 (TWOCHTOC + SACDTRL1 + SACDTRL2)
//! - 543..546  Area TOC copy 2
//! - 546..640  zeros
//! - 640..     2C_Audio (converted DSD sectors)

use crate::dsf::DsfInfo;
use crate::iso9660 as iso;
use crate::sacd::{self, TrackLayout};
use crate::toc::{self, MasterToc};
use crate::udf::{self, SECTOR};
use std::fs::File;
use std::io::{Seek, SeekFrom, Write};
use std::path::Path;

pub const L_PATH_TABLE_LBA: u32 = 18;
pub const M_PATH_TABLE_LBA: u32 = 19;
pub const ISO_ROOT_DIR_LBA: u32 = 20;
pub const ISO_SACD_DIR_LBA: u32 = 21;
pub const UDF_ANCHOR_LBA: u32 = 256;
pub const UDF_PVD_LBA: u32 = 257;
pub const UDF_PARTITION_LBA: u32 = 258;
pub const UDF_LVD_LBA: u32 = 259;
pub const UDF_TERM_LBA: u32 = 260;
pub const UDF_FSD_LBA: u32 = 261;
pub const UDF_ROOT_FE_LBA: u32 = 262;
pub const UDF_SACD_FE_LBA: u32 = 263;
pub const UDF_MASTER1_FE_LBA: u32 = 264;
pub const UDF_MASTER2_FE_LBA: u32 = 265;
pub const UDF_MASTER3_FE_LBA: u32 = 266;
pub const UDF_AUDIO_FE_LBA: u32 = 267;
pub const UDF_ROOT_FIDS_LBA: u32 = 268;
pub const UDF_SACD_FIDS_LBA: u32 = 269;
pub const AUDIO_START_LBA: u32 = 640;

pub struct ImageSummary {
    pub total_sectors: u32,
    pub audio_sectors: u32,
    pub layouts: Vec<TrackLayout>,
}

fn write_sector(file: &mut File, lba: u32, data: &[u8]) -> std::io::Result<()> {
    assert_eq!(data.len(), SECTOR);
    file.seek(SeekFrom::Start(lba as u64 * SECTOR as u64))?;
    file.write_all(data)
}

/// Build the complete SACD-R ISO image at `out_path` from the given DSF tracks.
pub fn build_image(infos: &[DsfInfo], out_path: &Path) -> std::io::Result<ImageSummary> {
    let layouts = sacd::track_layouts(infos);
    let audio_sectors: u32 = layouts.iter().map(|l| l.sector_count).sum();
    let total_sectors = AUDIO_START_LBA + audio_sectors;

    let mut file = File::create(out_path)?;
    file.set_len(total_sectors as u64 * SECTOR as u64)?;

    // --- ISO 9660 ---
    let root_rec = iso::dir_record(ISO_ROOT_DIR_LBA, SECTOR as u32, true, &[0]);
    let path_le = [
        iso::path_record_le(ISO_ROOT_DIR_LBA, 1, &[0]),
        iso::path_record_le(ISO_SACD_DIR_LBA, 1, b"SACD"),
    ]
    .concat();
    let path_be = [
        iso::path_record_be(ISO_ROOT_DIR_LBA, 1, &[0]),
        iso::path_record_be(ISO_SACD_DIR_LBA, 1, b"SACD"),
    ]
    .concat();
    let path_table_size = path_le.len() as u32;
    write_sector(
        &mut file,
        iso::PVD_LBA,
        &iso::pvd(
            total_sectors,
            path_table_size,
            L_PATH_TABLE_LBA,
            M_PATH_TABLE_LBA,
            &root_rec,
        ),
    )?;
    write_sector(&mut file, iso::TERMINATOR_LBA, &iso::terminator())?;
    let mut lpt = path_le.clone();
    lpt.resize(SECTOR, 0);
    write_sector(&mut file, L_PATH_TABLE_LBA, &lpt)?;
    let mut mpt = path_be;
    mpt.resize(SECTOR, 0);
    write_sector(&mut file, M_PATH_TABLE_LBA, &mpt)?;

    let audio_bytes = audio_sectors as u64 * SECTOR as u64;
    let mut root_dir = Vec::new();
    root_dir.extend_from_slice(&iso::dir_record(
        ISO_ROOT_DIR_LBA,
        SECTOR as u32,
        true,
        &[0],
    ));
    root_dir.extend_from_slice(&iso::dir_record(
        ISO_ROOT_DIR_LBA,
        SECTOR as u32,
        true,
        &[1],
    ));
    root_dir.extend_from_slice(&iso::dir_record(
        ISO_SACD_DIR_LBA,
        SECTOR as u32,
        true,
        b"SACD",
    ));
    root_dir.resize(SECTOR, 0);
    write_sector(&mut file, ISO_ROOT_DIR_LBA, &root_dir)?;

    let mut sacd_dir = Vec::new();
    sacd_dir.extend_from_slice(&iso::dir_record(
        ISO_SACD_DIR_LBA,
        SECTOR as u32,
        true,
        &[0],
    ));
    sacd_dir.extend_from_slice(&iso::dir_record(
        ISO_ROOT_DIR_LBA,
        SECTOR as u32,
        true,
        &[1],
    ));
    sacd_dir.extend_from_slice(&iso::dir_record(
        toc::START_OF_MASTER_TOC,
        10 * SECTOR as u32,
        false,
        b"MASTER1.TOC;1",
    ));
    sacd_dir.extend_from_slice(&iso::dir_record(
        toc::MASTER_TOC_2_START,
        10 * SECTOR as u32,
        false,
        b"MASTER2.TOC;1",
    ));
    sacd_dir.extend_from_slice(&iso::dir_record(
        toc::MASTER_TOC_3_START,
        10 * SECTOR as u32,
        false,
        b"MASTER3.TOC;1",
    ));
    sacd_dir.extend_from_slice(&iso::dir_record(
        AUDIO_START_LBA,
        audio_bytes as u32,
        false,
        b"2C_AUDIO.;1",
    ));
    sacd_dir.resize(SECTOR, 0);
    write_sector(&mut file, ISO_SACD_DIR_LBA, &sacd_dir)?;

    // --- UDF 1.02 ---
    write_sector(
        &mut file,
        UDF_ANCHOR_LBA,
        &udf::anchor(UDF_PVD_LBA, 4, UDF_PVD_LBA, 4),
    )?;
    write_sector(&mut file, UDF_PVD_LBA, &udf::primary_volume(1, "SACD"))?;
    write_sector(
        &mut file,
        UDF_PARTITION_LBA,
        &udf::partition_descriptor(2, 0, total_sectors),
    )?;
    write_sector(
        &mut file,
        UDF_LVD_LBA,
        &udf::logical_volume(3, UDF_FSD_LBA, total_sectors),
    )?;
    write_sector(&mut file, UDF_TERM_LBA, &udf::terminating(UDF_TERM_LBA))?;
    write_sector(
        &mut file,
        UDF_FSD_LBA,
        &udf::file_set_descriptor(UDF_FSD_LBA, UDF_ROOT_FE_LBA),
    )?;
    write_sector(
        &mut file,
        UDF_ROOT_FE_LBA,
        &udf::file_entry(
            UDF_ROOT_FE_LBA,
            true,
            SECTOR as u64,
            &[(SECTOR as u32, UDF_ROOT_FIDS_LBA)],
        ),
    )?;
    write_sector(
        &mut file,
        UDF_SACD_FE_LBA,
        &udf::file_entry(
            UDF_SACD_FE_LBA,
            true,
            SECTOR as u64,
            &[(SECTOR as u32, UDF_SACD_FIDS_LBA)],
        ),
    )?;
    let master_len = (toc::MASTER_TOC_LEN * SECTOR as u32) as u64;
    write_sector(
        &mut file,
        UDF_MASTER1_FE_LBA,
        &udf::file_entry(
            UDF_MASTER1_FE_LBA,
            false,
            master_len,
            &[(master_len as u32, toc::START_OF_MASTER_TOC)],
        ),
    )?;
    write_sector(
        &mut file,
        UDF_MASTER2_FE_LBA,
        &udf::file_entry(
            UDF_MASTER2_FE_LBA,
            false,
            master_len,
            &[(master_len as u32, toc::MASTER_TOC_2_START)],
        ),
    )?;
    write_sector(
        &mut file,
        UDF_MASTER3_FE_LBA,
        &udf::file_entry(
            UDF_MASTER3_FE_LBA,
            false,
            master_len,
            &[(master_len as u32, toc::MASTER_TOC_3_START)],
        ),
    )?;
    write_sector(
        &mut file,
        UDF_AUDIO_FE_LBA,
        &udf::file_entry(
            UDF_AUDIO_FE_LBA,
            false,
            audio_bytes,
            &[(audio_bytes as u32, AUDIO_START_LBA)],
        ),
    )?;

    // UDF file identifier descriptor extents
    let mut root_fids = Vec::new();
    root_fids.extend_from_slice(&udf::file_identifier(
        UDF_ROOT_FIDS_LBA,
        0x02,
        None,
        UDF_ROOT_FE_LBA,
    ));
    let mut dotdot = udf::file_identifier(UDF_ROOT_FIDS_LBA, 0x0A, None, UDF_ROOT_FE_LBA);
    dotdot[16 + 22] = 1; // ".." code
    root_fids.extend_from_slice(&dotdot);
    root_fids.extend_from_slice(&udf::file_identifier(
        UDF_ROOT_FIDS_LBA,
        0x02,
        Some("SACD"),
        UDF_SACD_FE_LBA,
    ));
    root_fids.truncate(SECTOR);
    root_fids.resize(SECTOR, 0);
    write_sector(&mut file, UDF_ROOT_FIDS_LBA, &root_fids)?;

    let mut sacd_fids = Vec::new();
    sacd_fids.extend_from_slice(&udf::file_identifier(
        UDF_SACD_FIDS_LBA,
        0x02,
        None,
        UDF_SACD_FE_LBA,
    ));
    let mut dotdot = udf::file_identifier(UDF_SACD_FIDS_LBA, 0x0A, None, UDF_ROOT_FE_LBA);
    dotdot[16 + 22] = 1;
    sacd_fids.extend_from_slice(&dotdot);
    sacd_fids.extend_from_slice(&udf::file_identifier(
        UDF_SACD_FIDS_LBA,
        0x00,
        Some("Master1.TOC"),
        UDF_MASTER1_FE_LBA,
    ));
    sacd_fids.extend_from_slice(&udf::file_identifier(
        UDF_SACD_FIDS_LBA,
        0x00,
        Some("Master2.TOC"),
        UDF_MASTER2_FE_LBA,
    ));
    sacd_fids.extend_from_slice(&udf::file_identifier(
        UDF_SACD_FIDS_LBA,
        0x00,
        Some("Master3.TOC"),
        UDF_MASTER3_FE_LBA,
    ));
    sacd_fids.extend_from_slice(&udf::file_identifier(
        UDF_SACD_FIDS_LBA,
        0x00,
        Some("2C_Audio"),
        UDF_AUDIO_FE_LBA,
    ));
    sacd_fids.truncate(SECTOR);
    sacd_fids.resize(SECTOR, 0);
    write_sector(&mut file, UDF_SACD_FIDS_LBA, &sacd_fids)?;

    // --- Scarlet Book TOCs (raw sectors) ---
    let master = MasterToc::new().to_sector();
    for lba in toc::START_OF_MASTER_TOC..toc::START_OF_MASTER_TOC + toc::MASTER_TOC_LEN {
        let data = if lba == toc::START_OF_MASTER_TOC {
            &master
        } else {
            &[0u8; SECTOR][..]
        };
        write_sector(&mut file, lba, data)?;
    }
    for lba in toc::MASTER_TOC_2_START..toc::MASTER_TOC_2_START + toc::MASTER_TOC_LEN {
        let data = if lba == toc::MASTER_TOC_2_START {
            &master
        } else {
            &[0u8; SECTOR][..]
        };
        write_sector(&mut file, lba, data)?;
    }
    for lba in toc::MASTER_TOC_3_START..toc::MASTER_TOC_3_START + toc::MASTER_TOC_LEN {
        let data = if lba == toc::MASTER_TOC_3_START {
            &master
        } else {
            &[0u8; SECTOR][..]
        };
        write_sector(&mut file, lba, data)?;
    }
    let area_toc = toc::build_area_toc(&layouts, AUDIO_START_LBA);
    for (i, chunk) in area_toc.chunks(SECTOR).enumerate() {
        write_sector(&mut file, toc::AREA_TOC_1_START + i as u32, chunk)?;
        write_sector(&mut file, toc::AREA_TOC_2_START + i as u32, chunk)?;
    }

    // --- 2C_Audio: stream the converted DSD sectors at AUDIO_START_LBA ---
    file.seek(SeekFrom::Start(AUDIO_START_LBA as u64 * SECTOR as u64))?;
    for info in infos {
        sacd::convert_track(info, &mut file)?;
    }
    file.flush()?;

    Ok(ImageSummary {
        total_sectors,
        audio_sectors,
        layouts,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::make_test_dsf;
    use std::io::Read;

    fn build_test_image(
        samples: u64,
    ) -> (
        tempfile::NamedTempFile,
        tempfile::NamedTempFile,
        ImageSummary,
    ) {
        let dsf_bytes = make_test_dsf(samples, false);
        let mut dsf = tempfile::NamedTempFile::new().unwrap();
        dsf.write_all(&dsf_bytes).unwrap();
        let info = crate::dsf::parse_bytes(&dsf_bytes, dsf.path()).unwrap();
        let iso_file = tempfile::NamedTempFile::new().unwrap();
        let summary = build_image(&[info], iso_file.path()).unwrap();
        (dsf, iso_file, summary)
    }

    fn read_at(path: &Path, lba: u32, len: usize) -> Vec<u8> {
        let mut f = File::open(path).unwrap();
        f.seek(SeekFrom::Start(lba as u64 * SECTOR as u64)).unwrap();
        let mut buf = vec![0u8; len];
        f.read_exact(&mut buf).unwrap();
        buf
    }

    #[test]
    fn image_has_sacd_signature_at_lba_510() {
        let (_d, iso, summary) = build_test_image(32768); // 1 frame = 5 sectors
        assert_eq!(summary.audio_sectors, 5);
        assert_eq!(summary.total_sectors, AUDIO_START_LBA + 5);
        assert_eq!(
            std::fs::metadata(iso.path()).unwrap().len(),
            summary.total_sectors as u64 * SECTOR as u64
        );
        // The BDP-S490 hardware constraint: Master1.TOC at byte offset 0xFF000
        let at_510 = read_at(iso.path(), 510, 8);
        assert_eq!(&at_510, b"SACDMTOC");
        let sector = read_at(iso.path(), 510, SECTOR);
        assert_eq!(&sector[0x40..0x44], &544u32.to_be_bytes());
    }

    #[test]
    fn image_has_iso_and_udf_structures() {
        let (_d, iso, _s) = build_test_image(32768);
        let pvd = read_at(iso.path(), 16, 7);
        assert_eq!(&pvd[0..1], &[1]);
        assert_eq!(&pvd[1..6], b"CD001");
        let anchor = read_at(iso.path(), 256, 2);
        assert_eq!(&anchor, &2u16.to_le_bytes());
        let area = read_at(iso.path(), 544, 8);
        assert_eq!(&area, b"TWOCHTOC");
        let trl1 = read_at(iso.path(), 545, 8);
        assert_eq!(&trl1, b"SACDTRL1");
        let trl2 = read_at(iso.path(), 546, 8);
        assert_eq!(&trl2, b"SACDTRL2");
        // UDF tag checksums valid on VDS sectors
        for lba in 256..=260 {
            let s = read_at(iso.path(), lba, SECTOR);
            let sum: u8 = s[0..4]
                .iter()
                .chain(s[5..16].iter())
                .copied()
                .fold(0u8, u8::wrapping_add);
            assert_eq!(s[4], sum, "bad tag checksum at LBA {lba}");
        }
    }

    #[test]
    fn image_audio_area_matches_conversion() {
        let (dsf, iso, summary) = build_test_image(100);
        let mut expected = Vec::new();
        let info = crate::dsf::parse(dsf.path()).unwrap();
        sacd::convert_track(&info, &mut expected).unwrap();
        let audio = read_at(iso.path(), AUDIO_START_LBA, expected.len());
        assert_eq!(audio, expected);
        assert_eq!(summary.layouts[0].start_sector, 0);
        assert_eq!(summary.layouts[0].sector_count, 5);
    }
}
