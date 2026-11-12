//! Scarlet Book TOC structures (Big-Endian), one 2048-byte sector each.
//! Field layout follows the ScarletBook format as parsed by sacd-ripper
//! (libs/libsacd/scarletbook.h). All multi-byte integers are big-endian.
//!
//! Master TOC (master_toc_t, 168 bytes used, padded to 2048):
//! - 0x000  8   id "SACDMTOC"
//! - 0x008  1   version major (1)
//! - 0x009  1   version minor (0x14 = 20 -> "1.20")
//! - 0x00A  6   reserved
//! - 0x010  2   album_set_size
//! - 0x012  2   album_sequence_number
//! - 0x014  4   reserved
//! - 0x018  16  album_catalog_number
//! - 0x028  16  album_genre[4] (u8 category, u16 reserved, u8 genre)
//! - 0x038  8   reserved
//! - 0x040  4   area_1_toc_1_start (LSN)
//! - 0x044  4   area_1_toc_2_start (LSN)
//! - 0x048  4   area_2_toc_1_start (LSN)
//! - 0x04C  4   area_2_toc_2_start (LSN)
//! - 0x050  1   disc_type (bit 7 = hybrid, big-endian bitfield)
//! - 0x051  3   reserved
//! - 0x054  2   area_1_toc_size (sectors)
//! - 0x056  2   area_2_toc_size (sectors)
//! - 0x058  16  disc_catalog_number
//! - 0x068  16  disc_genre[4]
//! - 0x078  2   disc_date_year
//! - 0x07A  1   disc_date_month
//! - 0x07B  1   disc_date_day
//! - 0x07C  4   reserved
//! - 0x080  1   text_area_count
//! - 0x081  7   reserved
//! - 0x088  32  locales[8] (u8 lang[2], u8 char_set, u8 reserved)
//!
//! Area TOC header (area_toc_t, exactly 2048 bytes), id "TWOCHTOC":
//! - 0x000  8   id
//! - 0x008  2   version 1.20
//! - 0x00A  2   size
//! - 0x00C  4   reserved
//! - 0x010  4   max_byte_rate
//! - 0x014  1   sample_frequency (0x04 = 64*44.1 kHz)
//! - 0x015  1   frame_format (high nibble; 0 = plain DSD)
//! - 0x016  10  reserved
//! - 0x020  1   channel_count (2)
//! - 0x021  1   loudspeaker_config/extra_settings
//! - 0x022  1   max_available_channels
//! - 0x023  1   area_mute_flags
//! - 0x024  12  reserved
//! - 0x030  1   track_attribute (high nibble)
//! - 0x031  15  reserved
//! - 0x040  3   total_playtime (minutes, seconds, frames @75fps)
//! - 0x043  1   reserved
//! - 0x044  1   track_offset (0)
//! - 0x045  1   track_count
//! - 0x046  2   reserved
//! - 0x048  4   track_start (LSN of first track)
//! - 0x04C  4   track_end (LSN, end of last track)
//! - 0x050  1   text_area_count
//! - 0x051  7   reserved
//! - 0x058  40  languages[10]
//! - 0x080  2   track_text_offset
//! - 0x082  2   index_list_offset
//! - 0x084  2   access_list_offset
//! - 0x086  10  reserved
//! - 0x090  2   area_description_offset
//! - 0x092  2   copyright_offset
//! - 0x094  2   area_description_phonetic_offset
//! - 0x096  2   copyright_phonetic_offset
//! - 0x098  1896 data (zero)
//!
//! Track lists, in the following sectors of the area TOC:
//! - "SACDTRL1": track_start_lsn[255] (u32), track_length_lsn[255] (u32)
//! - "SACDTRL2": start[255] and duration[255], each 4 bytes
//!   (minutes, seconds, frames, flags)

use crate::sacd::TrackLayout;
use binrw::{BinWrite, binrw};
use std::io::Cursor;

pub const SECTOR_SIZE: usize = 2048;
pub const START_OF_MASTER_TOC: u32 = 510;
pub const MASTER_TOC_LEN: u32 = 10;
pub const MASTER_TOC_2_START: u32 = START_OF_MASTER_TOC + MASTER_TOC_LEN; // 520
pub const MASTER_TOC_3_START: u32 = MASTER_TOC_2_START + MASTER_TOC_LEN; // 530
pub const AREA_TOC_1_START: u32 = MASTER_TOC_3_START + MASTER_TOC_LEN + 4; // 544
pub const AREA_TOC_SECTORS: u16 = 40; // header + SACDTRL1 + SACDTRL2 + text/ISRC/access areas
pub const AREA_TOC_2_START: u32 = AREA_TOC_1_START + AREA_TOC_SECTORS as u32; // 584
pub const SACD_FRAME_RATE: u32 = 75;
pub const VERSION_MAJOR: u8 = 1;
pub const VERSION_MINOR: u8 = 0x14;
pub const SAMPLE_FREQUENCY_DSD64: u8 = 0x04;
pub const MAX_BYTE_RATE_DSD64_STEREO: u32 = 337_920; // observed on commercial SACD area TOC
pub const DISC_TYPE_HYBRID: u8 = 0x80;

#[binrw]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[brw(big)]
pub struct GenreTable {
    pub category: u8,
    pub reserved: u16,
    pub genre: u8,
}

#[binrw]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[brw(big)]
pub struct LocaleTable {
    pub language_code: [u8; 2],
    pub character_set: u8,
    pub reserved: u8,
}

impl Default for LocaleTable {
    fn default() -> Self {
        Self {
            language_code: *b"en",
            character_set: 1,
            reserved: 0,
        }
    }
}

#[binrw]
#[derive(Debug, Clone, PartialEq, Eq)]
#[brw(big)]
pub struct MasterToc {
    #[brw(magic = b"SACDMTOC")]
    pub version_major: u8,
    pub version_minor: u8,
    pub reserved01: [u8; 6],
    pub album_set_size: u16,
    pub album_sequence_number: u16,
    pub reserved02: [u8; 4],
    pub album_catalog_number: [u8; 16],
    pub album_genre: [GenreTable; 4],
    pub reserved03: [u8; 8],
    pub area_1_toc_1_start: u32,
    pub area_1_toc_2_start: u32,
    pub area_2_toc_1_start: u32,
    pub area_2_toc_2_start: u32,
    pub disc_type: u8,
    pub reserved04: [u8; 3],
    pub area_1_toc_size: u16,
    pub area_2_toc_size: u16,
    pub disc_catalog_number: [u8; 16],
    pub disc_genre: [GenreTable; 4],
    pub disc_date_year: u16,
    pub disc_date_month: u8,
    pub disc_date_day: u8,
    pub reserved05: [u8; 4],
    pub text_area_count: u8,
    pub reserved06: [u8; 7],
    pub locales: [LocaleTable; 8],
}

impl MasterToc {
    pub fn new() -> Self {
        Self {
            version_major: VERSION_MAJOR,
            version_minor: VERSION_MINOR,
            reserved01: [0; 6],
            album_set_size: 1,
            album_sequence_number: 1,
            reserved02: [0; 4],
            album_catalog_number: [0; 16],
            album_genre: [GenreTable::default(); 4],
            reserved03: [0; 8],
            area_1_toc_1_start: AREA_TOC_1_START,
            area_1_toc_2_start: AREA_TOC_2_START,
            area_2_toc_1_start: 0,
            area_2_toc_2_start: 0,
            disc_type: DISC_TYPE_HYBRID,
            reserved04: [0; 3],
            area_1_toc_size: AREA_TOC_SECTORS,
            area_2_toc_size: 0,
            disc_catalog_number: [0; 16],
            disc_genre: [GenreTable::default(); 4],
            disc_date_year: 2026,
            disc_date_month: 1,
            disc_date_day: 1,
            reserved05: [0; 4],
            text_area_count: 0,
            reserved06: [0; 7],
            locales: [LocaleTable::default(); 8],
        }
    }

    /// Serialize to exactly one 2048-byte sector (zero-padded).
    pub fn to_sector(&self) -> Vec<u8> {
        to_sector_bytes(self)
    }
}

impl Default for MasterToc {
    fn default() -> Self {
        Self::new()
    }
}

/// One `SACDText` sector of the master TOC (all text positions = 0).
pub fn master_sacd_text_sector() -> Vec<u8> {
    let mut s = vec![0u8; SECTOR_SIZE];
    s[0..8].copy_from_slice(b"SACDText");
    s
}

/// The `SACD_Man` (manufacturer info) sector of the master TOC.
pub fn master_man_sector() -> Vec<u8> {
    let mut s = vec![0u8; SECTOR_SIZE];
    s[0..8].copy_from_slice(b"SACD_Man");
    s
}

/// Full 10-sector master TOC area: SACDMTOC header + 8 SACDText sectors
/// (one per locale) + SACD_Man. Readers such as foo_input_sacd require all
/// three magics to be present.
pub fn master_toc_area() -> Vec<u8> {
    let mut out = MasterToc::new().to_sector();
    for _ in 0..8 {
        out.extend_from_slice(&master_sacd_text_sector());
    }
    out.extend_from_slice(&master_man_sector());
    out
}

#[binrw]
#[derive(Debug, Clone, PartialEq, Eq)]
#[brw(big)]
pub struct AreaToc {
    #[brw(magic = b"TWOCHTOC")]
    pub version_major: u8,
    pub version_minor: u8,
    pub size: u16,
    pub reserved01: [u8; 4],
    pub max_byte_rate: u32,
    pub sample_frequency: u8,
    pub frame_format: u8,
    pub reserved03: [u8; 10],
    pub channel_count: u8,
    pub loudspeaker_config: u8,
    pub max_available_channels: u8,
    pub area_mute_flags: u8,
    pub reserved04: [u8; 12],
    pub track_attribute: u8,
    pub reserved06: [u8; 15],
    pub total_playtime: [u8; 3],
    pub reserved07: u8,
    pub track_offset: u8,
    pub track_count: u8,
    pub reserved08: [u8; 2],
    pub track_start: u32,
    pub track_end: u32,
    pub text_area_count: u8,
    pub reserved09: [u8; 7],
    pub languages: [LocaleTable; 10],
    pub track_text_offset: u16,
    pub index_list_offset: u16,
    pub access_list_offset: u16,
    pub reserved10: [u8; 10],
    pub area_description_offset: u16,
    pub copyright_offset: u16,
    pub area_description_phonetic_offset: u16,
    pub copyright_phonetic_offset: u16,
    pub data: [u8; 1896],
}

#[binrw]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[brw(big)]
pub struct TrackTime {
    pub minutes: u8,
    pub seconds: u8,
    pub frames: u8,
    pub flags: u8,
}

impl TrackTime {
    /// Convert a frame count (75 fps) to (minutes, seconds, frames).
    pub fn from_frames(total_frames: u64) -> Self {
        let frames = (total_frames % SACD_FRAME_RATE as u64) as u8;
        let total_seconds = total_frames / SACD_FRAME_RATE as u64;
        let seconds = (total_seconds % 60) as u8;
        let minutes = (total_seconds / 60) as u8;
        Self {
            minutes,
            seconds,
            frames,
            flags: 0,
        }
    }
}

#[binrw]
#[derive(Debug, Clone, PartialEq, Eq)]
#[brw(big)]
pub struct TrackList1 {
    #[brw(magic = b"SACDTRL1")]
    pub _magic: (),
    pub track_start_lsn: [u32; 255],
    pub track_length_lsn: [u32; 255],
}

#[binrw]
#[derive(Debug, Clone, PartialEq, Eq)]
#[brw(big)]
pub struct TrackList2 {
    #[brw(magic = b"SACDTRL2")]
    pub _magic: (),
    pub start: [TrackTime; 255],
    pub duration: [TrackTime; 255],
}

/// Build the 3-sector stereo area TOC for the given tracks.
/// `audio_start_lba` is the absolute LBA where the 2C_Audio file starts on disc.
pub fn build_area_toc(layouts: &[TrackLayout], audio_start_lba: u32) -> Vec<u8> {
    let track_count = layouts.len() as u8;
    let total_frames: u64 = layouts.iter().map(|l| l.frames).sum();
    let first_start = layouts
        .first()
        .map_or(0, |l| audio_start_lba + l.start_sector);
    let last = layouts.last();
    let track_end = last.map_or(0, |l| audio_start_lba + l.start_sector + l.sector_count);

    let mut trl1 = TrackList1 {
        _magic: (),
        track_start_lsn: [0; 255],
        track_length_lsn: [0; 255],
    };
    let mut trl2 = TrackList2 {
        _magic: (),
        start: [TrackTime::default(); 255],
        duration: [TrackTime::default(); 255],
    };
    let mut elapsed = 0u64;
    for (i, layout) in layouts.iter().enumerate() {
        trl1.track_start_lsn[i] = audio_start_lba + layout.start_sector;
        trl1.track_length_lsn[i] = layout.sector_count;
        trl2.start[i] = TrackTime::from_frames(elapsed);
        trl2.duration[i] = TrackTime::from_frames(layout.frames);
        elapsed += layout.frames;
    }

    let header = AreaToc {
        version_major: VERSION_MAJOR,
        version_minor: VERSION_MINOR,
        size: 40,
        reserved01: [0; 4],
        max_byte_rate: MAX_BYTE_RATE_DSD64_STEREO,
        sample_frequency: SAMPLE_FREQUENCY_DSD64,
        frame_format: 0,
        reserved03: [0; 10],
        channel_count: 2,
        loudspeaker_config: 0,
        max_available_channels: 2,
        area_mute_flags: 0,
        reserved04: [0; 12],
        track_attribute: 0,
        reserved06: [0; 15],
        total_playtime: {
            let t = TrackTime::from_frames(total_frames);
            [t.minutes, t.seconds, t.frames]
        },
        reserved07: 0,
        track_offset: 0,
        track_count,
        reserved08: [0; 2],
        track_start: first_start,
        track_end,
        text_area_count: 0,
        reserved09: [0; 7],
        languages: [LocaleTable::default(); 10],
        track_text_offset: 0,
        index_list_offset: 0,
        access_list_offset: 0,
        reserved10: [0; 10],
        area_description_offset: 0,
        copyright_offset: 0,
        area_description_phonetic_offset: 0,
        copyright_phonetic_offset: 0,
        data: [0; 1896],
    };

    let mut out = to_sector_bytes(&header);
    out.extend_from_slice(&to_sector_bytes(&trl1));
    out.extend_from_slice(&to_sector_bytes(&trl2));
    out.resize(AREA_TOC_SECTORS as usize * SECTOR_SIZE, 0);
    out
}

fn to_sector_bytes<T>(value: &T) -> Vec<u8>
where
    T: for<'a> BinWrite<Args<'a> = ()>,
{
    let mut cursor = Cursor::new(Vec::with_capacity(SECTOR_SIZE));
    value
        .write_be(&mut cursor)
        .expect("serializing to an in-memory cursor cannot fail");
    let mut buf = cursor.into_inner();
    buf.resize(SECTOR_SIZE, 0);
    buf
}

#[cfg(test)]
mod tests {
    use super::*;
    use binrw::BinReaderExt;

    #[test]
    fn master_toc_sector_layout() {
        let toc = MasterToc::new();
        let bytes = toc.to_sector();
        assert_eq!(bytes.len(), SECTOR_SIZE);
        assert_eq!(&bytes[0x00..0x08], b"SACDMTOC");
        assert_eq!(bytes[0x08], 1);
        assert_eq!(bytes[0x09], 0x14);
        assert_eq!(&bytes[0x40..0x44], &544u32.to_be_bytes());
        assert_eq!(&bytes[0x44..0x48], &584u32.to_be_bytes());
        assert_eq!(bytes[0x50], DISC_TYPE_HYBRID);
        assert_eq!(&bytes[0x54..0x56], &40u16.to_be_bytes());
        assert!(bytes[0xA8..].iter().all(|&b| b == 0));
    }

    #[test]
    fn track_time_conversion() {
        let t = TrackTime::from_frames(75);
        assert_eq!((t.minutes, t.seconds, t.frames), (0, 1, 0));
        let t = TrackTime::from_frames(7668); // 102.24 s
        assert_eq!((t.minutes, t.seconds, t.frames), (1, 42, 18));
        assert_eq!(TrackTime::from_frames(0).frames, 0);
    }

    #[test]
    fn area_toc_three_sectors_with_tracks() {
        let layouts = vec![
            TrackLayout {
                start_sector: 0,
                sector_count: 100,
                frames: 75,
            },
            TrackLayout {
                start_sector: 100,
                sector_count: 200,
                frames: 150,
            },
        ];
        let bytes = build_area_toc(&layouts, 640);
        assert_eq!(bytes.len(), 40 * SECTOR_SIZE);
        assert_eq!(&bytes[0x000..0x008], b"TWOCHTOC");
        assert_eq!(&bytes[0x010..0x014], &337920u32.to_be_bytes());
        assert_eq!(bytes[0x014], 0x04);
        assert_eq!(bytes[0x020], 2); // channel_count
        assert_eq!(bytes[0x045], 2); // track_count
        assert_eq!(&bytes[0x048..0x04C], &640u32.to_be_bytes()); // track_start
        assert_eq!(&bytes[0x04C..0x050], &940u32.to_be_bytes()); // track_end = 640+100+200
        // total playtime = 225 frames = 3 s
        assert_eq!(&bytes[0x40..0x43], &[0, 3, 0]);
        // SACDTRL1 at sector 1
        let off = SECTOR_SIZE;
        assert_eq!(&bytes[off..off + 8], b"SACDTRL1");
        assert_eq!(&bytes[off + 8..off + 12], &640u32.to_be_bytes());
        assert_eq!(&bytes[off + 12..off + 16], &740u32.to_be_bytes());
        assert_eq!(
            &bytes[off + 8 + 4 * 255..off + 8 + 4 * 255 + 4],
            &100u32.to_be_bytes()
        );
        // SACDTRL2 at sector 2: start[1] = 75 frames = 0:01:00, duration[1] = 150 = 0:02:00
        let off = 2 * SECTOR_SIZE;
        assert_eq!(&bytes[off..off + 8], b"SACDTRL2");
        assert_eq!(&bytes[off + 8 + 4..off + 8 + 8], &[0, 1, 0, 0]);
        assert_eq!(
            &bytes[off + 8 + 4 * 255 + 4..off + 8 + 4 * 255 + 8],
            &[0, 2, 0, 0]
        );
    }

    #[test]
    fn master_toc_roundtrip() {
        let toc = MasterToc::new();
        let bytes = toc.to_sector();
        let mut cursor = Cursor::new(&bytes);
        let parsed: MasterToc = cursor.read_be().unwrap();
        assert_eq!(parsed, toc);
    }

    #[test]
    fn master_toc_area_has_text_and_man_sectors() {
        let area = master_toc_area();
        assert_eq!(area.len(), 10 * SECTOR_SIZE);
        assert_eq!(&area[0..8], b"SACDMTOC");
        for i in 1..9 {
            assert_eq!(&area[i * SECTOR_SIZE..i * SECTOR_SIZE + 8], b"SACDText");
        }
        assert_eq!(&area[9 * SECTOR_SIZE..9 * SECTOR_SIZE + 8], b"SACD_Man");
    }
}
