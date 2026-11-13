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
pub const AREA_TOC_1_START: u32 = MASTER_TOC_3_START + MASTER_TOC_LEN; // 540
/// Area TOC layout (6 sectors): header, SACDTTxt, SACD_IGL (2 sectors),
/// SACDTRL1, SACDTRL2. Area TOC-2 is written after the audio area
/// (Scarlet Book 2.2.6) at AUDIO_START_LBA + audio_sectors.
pub const AREA_TOC_SECTORS: u16 = 6;
/// Plain (uncompressed) DSD frame format code in the area TOC header
/// (low nibble of offset 0x15); 0 would mean DST.
pub const FRAME_FORMAT_DSD_3IN14: u8 = 2;
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
    pub fn new(area1_toc1: u32, area1_toc2: u32, area1_size: u16) -> Self {
        let catalog = |s: &[u8; 16]| *s;
        let genre = GenreTable {
            category: 1,
            reserved: 0,
            genre: 25,
        };
        let mut locales = [LocaleTable {
            language_code: [0; 2],
            character_set: 0,
            reserved: 0,
        }; 8];
        locales[0] = LocaleTable::default(); // one text channel: "en", ISO 646
        Self {
            version_major: VERSION_MAJOR,
            version_minor: VERSION_MINOR,
            reserved01: [0; 6],
            album_set_size: 1,
            album_sequence_number: 1,
            reserved02: [0; 4],
            album_catalog_number: catalog(b"SACD-AUTHOR-0001"),
            album_genre: {
                let mut g = [GenreTable::default(); 4];
                g[0] = genre;
                g
            },
            reserved03: [0; 8],
            area_1_toc_1_start: area1_toc1,
            area_1_toc_2_start: area1_toc2,
            area_2_toc_1_start: 0,
            area_2_toc_2_start: 0,
            disc_type: DISC_TYPE_HYBRID,
            reserved04: [0; 3],
            area_1_toc_size: area1_size,
            area_2_toc_size: 0,
            disc_catalog_number: catalog(b"SACD-AUTHOR-D001"),
            disc_genre: {
                let mut g = [GenreTable::default(); 4];
                g[0] = genre;
                g
            },
            disc_date_year: 2026,
            disc_date_month: 1,
            disc_date_day: 1,
            reserved05: [0; 4],
            text_area_count: 1,
            reserved06: [0; 7],
            locales,
        }
    }

    /// Serialize to exactly one 2048-byte sector (zero-padded).
    pub fn to_sector(&self) -> Vec<u8> {
        to_sector_bytes(self)
    }
}

impl Default for MasterToc {
    fn default() -> Self {
        Self::new(
            AREA_TOC_1_START,
            AREA_TOC_1_START + AREA_TOC_SECTORS as u32,
            AREA_TOC_SECTORS,
        )
    }
}

/// One `SACDText` sector of the master TOC. Layout (matching commercial
/// discs): id[8], reserved[8], 16 x u16 BE text positions at 0x10, in the
/// order of spec 2.5.4: album title/artist/publisher/copyright (0x10-0x17),
/// album phonetic ditto (0x18-0x1F), disc title/artist/publisher/copyright
/// (0x20-0x27), disc phonetic ditto (0x28-0x2F); NUL-terminated strings from
/// 0x40. A position of 0 means "absent".
pub fn master_sacd_text_sector(
    album_title: Option<&str>,
    album_artist: Option<&str>,
    disc_title: Option<&str>,
    disc_artist: Option<&str>,
) -> Vec<u8> {
    let mut s = vec![0u8; SECTOR_SIZE];
    s[0..8].copy_from_slice(b"SACDText");
    let entries: [Option<&str>; 16] = [
        album_title,
        album_artist,
        None,
        None,
        None,
        None,
        None,
        None,
        disc_title,
        disc_artist,
        None,
        None,
        None,
        None,
        None,
        None,
    ];
    let mut data_off = 0x40usize;
    for (i, e) in entries.iter().enumerate() {
        if let Some(txt) = e {
            let bytes = latin1(txt);
            let pos = data_off as u16;
            s[0x10 + i * 2..0x10 + i * 2 + 2].copy_from_slice(&pos.to_be_bytes());
            s[data_off..data_off + bytes.len()].copy_from_slice(&bytes);
            data_off += bytes.len() + 1;
        }
    }
    s
}

/// Lossy Latin-1 encoding for SACDText (character_set 1 = ISO 646; per spec
/// 2.6.2 bytes 0x80-0xFF still decode as ISO 8859-1, so accents survive).
/// Control characters become spaces, anything above U+00FF becomes '?'.
fn latin1(s: &str) -> Vec<u8> {
    s.chars()
        .map(|c| {
            let cp = c as u32;
            if cp < 0x20 {
                b' '
            } else if cp <= 0xFF {
                cp as u8
            } else {
                b'?'
            }
        })
        .collect()
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
    master_toc_area_with_text(
        None,
        None,
        None,
        None,
        AREA_TOC_1_START,
        AREA_TOC_1_START + AREA_TOC_SECTORS as u32,
        AREA_TOC_SECTORS,
    )
}

/// Full 10-sector master TOC area with optional disc-level text.
pub fn master_toc_area_with_text(
    album_title: Option<&str>,
    album_artist: Option<&str>,
    disc_title: Option<&str>,
    disc_artist: Option<&str>,
    area1_toc1: u32,
    area1_toc2: u32,
    area1_size: u16,
) -> Vec<u8> {
    let mut out = MasterToc::new(area1_toc1, area1_toc2, area1_size).to_sector();
    for _ in 0..8 {
        out.extend_from_slice(&master_sacd_text_sector(
            album_title,
            album_artist,
            disc_title,
            disc_artist,
        ));
    }
    out.extend_from_slice(&master_man_sector());
    out
}

/// One track text entry: (type, string).
pub struct TrackText {
    pub title: String,
    pub performer: String,
}

/// The `SACDTTxt` sector of an area TOC: id[8], track_text_position[255]
/// (u16 BE offsets into this sector), then per-track text blocks.
/// Each block: amount u8, reserved[3], then amount entries of
/// [type u8, 0x20 u8, NUL-terminated string].
pub fn area_text_sector(tracks: &[TrackText]) -> Vec<u8> {
    let mut s = vec![0u8; SECTOR_SIZE];
    s[0..8].copy_from_slice(b"SACDTTxt");
    let mut off = 8 + 255 * 2;
    for (i, t) in tracks.iter().enumerate() {
        if off >= SECTOR_SIZE {
            break;
        }
        s[8 + i * 2..10 + i * 2].copy_from_slice(&(off as u16).to_be_bytes());
        let entries: [(u8, &str); 2] = [(0x01, t.title.as_str()), (0x02, t.performer.as_str())];
        s[off] = entries.len() as u8;
        off += 4;
        for (ttype, txt) in entries {
            let bytes = latin1(txt);
            if off + 2 + bytes.len() + 1 > SECTOR_SIZE {
                break;
            }
            s[off] = ttype;
            s[off + 1] = 0x20;
            s[off + 2..off + 2 + bytes.len()].copy_from_slice(&bytes);
            off += 2 + bytes.len() + 1;
        }
    }
    s
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

/// The `SACD_IGL` structure (2 sectors = 4096 bytes): id[8], then 255 x 12
/// bytes ISRC (fixed text), 4 reserved bytes at 3068, then 255 x 4 bytes
/// genre entries at 3072.
pub fn area_igl_sectors(track_count: usize) -> Vec<u8> {
    let mut s = vec![0u8; 2 * SECTOR_SIZE];
    s[0..8].copy_from_slice(b"SACD_IGL");
    for i in 0..track_count.min(255) {
        let isrc = format!("XXSAC26{:05}", i + 1); // CCXXXYYNNNNN, 12 chars
        let b = isrc.as_bytes();
        s[8 + i * 12..8 + i * 12 + 12].copy_from_slice(b);
        // genre entry: category 1, reserved, genre 25 (as on commercial discs)
        s[3072 + i * 4..3072 + i * 4 + 4].copy_from_slice(&[1, 0, 0, 25]);
    }
    s
}

/// Build the stereo area TOC (AREA_TOC_SECTORS sectors) for the given tracks.
/// Layout: header (sector 0), SACDTTxt (1), SACD_IGL (2-3), SACDTRL1 (4),
/// SACDTRL2 (5). `audio_start_lba` is the absolute LBA where the audio area
/// starts on disc.
pub fn build_area_toc(
    layouts: &[TrackLayout],
    audio_start_lba: u32,
    texts: &[TrackText],
) -> Vec<u8> {
    let track_count = layouts.len() as u8;
    let total_frames: u64 = layouts.iter().map(|l| l.frames).sum();
    let first_start = layouts
        .first()
        .map_or(0, |l| audio_start_lba + l.start_sector);
    let last = layouts.last();
    // last audio LSN is INCLUSIVE (Scarlet Book 3.3)
    let track_end = last.map_or(0, |l| {
        audio_start_lba + l.start_sector + l.sector_count.saturating_sub(1)
    });

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

    let mut languages = [LocaleTable {
        language_code: [0; 2],
        character_set: 0,
        reserved: 0,
    }; 10];
    languages[0] = LocaleTable::default(); // one text channel: "en", ISO 646

    let header = AreaToc {
        version_major: VERSION_MAJOR,
        version_minor: VERSION_MINOR,
        size: AREA_TOC_SECTORS,
        reserved01: [0; 4],
        max_byte_rate: MAX_BYTE_RATE_DSD64_STEREO,
        sample_frequency: SAMPLE_FREQUENCY_DSD64,
        frame_format: FRAME_FORMAT_DSD_3IN14, // plain DSD (0 would mean DST)
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
        text_area_count: 1,
        reserved09: [0; 7],
        languages,
        track_text_offset: if texts.is_empty() { 0 } else { 1 },
        index_list_offset: 2,
        access_list_offset: 0,
        reserved10: [0; 10],
        area_description_offset: 0,
        copyright_offset: 0,
        area_description_phonetic_offset: 0,
        copyright_phonetic_offset: 0,
        data: [0; 1896],
    };

    let mut out = to_sector_bytes(&header);
    if !texts.is_empty() {
        out.extend_from_slice(&area_text_sector(texts));
    } else {
        out.extend_from_slice(&vec![0u8; SECTOR_SIZE]);
    }
    out.extend_from_slice(&area_igl_sectors(layouts.len()));
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
        let toc = MasterToc::new(540, 556, 6);
        let bytes = toc.to_sector();
        assert_eq!(bytes.len(), SECTOR_SIZE);
        assert_eq!(&bytes[0x00..0x08], b"SACDMTOC");
        assert_eq!(bytes[0x08], 1);
        assert_eq!(bytes[0x09], 0x14);
        assert_eq!(&bytes[0x40..0x44], &540u32.to_be_bytes());
        assert_eq!(&bytes[0x44..0x48], &556u32.to_be_bytes());
        assert_eq!(bytes[0x50], DISC_TYPE_HYBRID);
        assert_eq!(&bytes[0x54..0x56], &6u16.to_be_bytes());
        assert_eq!(bytes[0x80], 1); // text_area_count
        assert_eq!(&bytes[0x88..0x8C], &b"en\x01\x00"[..]); // one locale
        assert!(bytes[0x8C..].iter().all(|&b| b == 0)); // rest zeroed
        assert_eq!(&bytes[0x18..0x28], b"SACD-AUTHOR-0001");
        assert_eq!(&bytes[0x58..0x68], b"SACD-AUTHOR-D001");
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
    fn area_toc_six_sectors_with_tracks() {
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
        let bytes = build_area_toc(&layouts, 640, &[]);
        assert_eq!(bytes.len(), AREA_TOC_SECTORS as usize * SECTOR_SIZE);
        assert_eq!(&bytes[0x000..0x008], b"TWOCHTOC");
        assert_eq!(&bytes[0x00A..0x00C], &6u16.to_be_bytes()); // size
        assert_eq!(&bytes[0x010..0x014], &337920u32.to_be_bytes());
        assert_eq!(bytes[0x014], 0x04);
        assert_eq!(bytes[0x015] & 0x0F, FRAME_FORMAT_DSD_3IN14); // plain DSD, not DST
        assert_eq!(bytes[0x020], 2); // channel_count
        assert_eq!(bytes[0x045], 2); // track_count
        assert_eq!(&bytes[0x048..0x04C], &640u32.to_be_bytes()[..]); // track_start
        assert_eq!(&bytes[0x04C..0x050], &939u32.to_be_bytes()[..]); // track_end inclusive = 640+100+200-1
        assert_eq!(bytes[0x050], 1); // text_area_count
        assert_eq!(&bytes[0x58..0x5C], &b"en\x01\x00"[..]); // one locale
        assert!(bytes[0x5C..0x80].iter().all(|&b| b == 0));
        assert_eq!(&bytes[0x80..0x82], &0u16.to_be_bytes()); // no track text here
        assert_eq!(&bytes[0x82..0x84], &2u16.to_be_bytes()); // index list at sector 2
        // total playtime = 225 frames = 3 s
        assert_eq!(&bytes[0x40..0x43], &[0, 3, 0]);
        // sector layout: header, (empty TTxt), IGL x2, TRL1, TRL2
        let off = 2 * SECTOR_SIZE;
        assert_eq!(&bytes[off..off + 8], b"SACD_IGL");
        assert_eq!(&bytes[off + 8..off + 20], b"XXSAC2600001");
        let off = 4 * SECTOR_SIZE;
        assert_eq!(&bytes[off..off + 8], b"SACDTRL1");
        assert_eq!(&bytes[off + 8..off + 12], &640u32.to_be_bytes());
        assert_eq!(&bytes[off + 12..off + 16], &740u32.to_be_bytes());
        assert_eq!(
            &bytes[off + 8 + 4 * 255..off + 8 + 4 * 255 + 4],
            &100u32.to_be_bytes()
        );
        // SACDTRL2 at sector 5: start[1] = 75 frames = 0:01:00, duration[1] = 150 = 0:02:00
        let off = 5 * SECTOR_SIZE;
        assert_eq!(&bytes[off..off + 8], b"SACDTRL2");
        assert_eq!(&bytes[off + 8 + 4..off + 8 + 8], &[0, 1, 0, 0]);
        assert_eq!(
            &bytes[off + 8 + 4 * 255 + 4..off + 8 + 4 * 255 + 8],
            &[0, 2, 0, 0]
        );
    }

    #[test]
    fn master_toc_roundtrip() {
        let toc = MasterToc::new(540, 556, 6);
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

    #[test]
    fn sacd_text_sectors_carry_titles() {
        let m = master_sacd_text_sector(
            Some("My Album"),
            Some("An Artist"),
            Some("My Disc"),
            Some("A Disc Artist"),
        );
        assert_eq!(&m[0..8], b"SACDText");
        // album title/artist at 0x10/0x12, disc title/artist at 0x20/0x22
        // (spec 2.5.4), phonetic slots left empty
        let title_pos = u16::from_be_bytes([m[0x10], m[0x11]]) as usize;
        let artist_pos = u16::from_be_bytes([m[0x12], m[0x13]]) as usize;
        let disc_pos = u16::from_be_bytes([m[0x20], m[0x21]]) as usize;
        let disc_artist_pos = u16::from_be_bytes([m[0x22], m[0x23]]) as usize;
        assert_eq!(&m[title_pos..title_pos + 8], b"My Album");
        assert_eq!(&m[artist_pos..artist_pos + 9], b"An Artist");
        assert_eq!(&m[disc_pos..disc_pos + 7], b"My Disc");
        assert_eq!(&m[disc_artist_pos..disc_artist_pos + 13], b"A Disc Artist");
        assert_eq!(&m[0x14..0x20], &[0u8; 12]); // album publisher/copyright + phonetic
        assert_eq!(&m[0x24..0x30], &[0u8; 12]); // disc publisher/copyright + phonetic

        let texts = vec![
            TrackText {
                title: "Song A".into(),
                performer: "Band".into(),
            },
            TrackText {
                title: "Song B".into(),
                performer: "Band".into(),
            },
        ];
        let a = area_text_sector(&texts);
        assert_eq!(&a[0..8], b"SACDTTxt");
        let p0 = u16::from_be_bytes([a[8], a[9]]) as usize;
        let p1 = u16::from_be_bytes([a[10], a[11]]) as usize;
        assert_eq!(a[p0], 2); // amount
        assert_eq!(a[p0 + 4], 0x01); // title type
        assert_eq!(&a[p0 + 6..p0 + 11], b"Song ");
        assert!(p1 > p0);
        assert_eq!(a[p1 + 4], 0x01);
    }

    #[test]
    fn sacd_text_uses_latin1_for_accents() {
        // character_set 1 still decodes bytes 0x80-0xFF as ISO 8859-1 (spec
        // 2.6.2), so "Década" is stored as Latin-1, not "D?cada"
        let m = master_sacd_text_sector(Some("Década"), None, None, None);
        let title_pos = u16::from_be_bytes([m[0x10], m[0x11]]) as usize;
        assert_eq!(&m[title_pos..title_pos + 6], b"D\xe9cada");

        let texts = vec![TrackText {
            title: "Una Sensación".into(),
            performer: String::new(),
        }];
        let a = area_text_sector(&texts);
        let p0 = u16::from_be_bytes([a[8], a[9]]) as usize;
        assert_eq!(&a[p0 + 6..p0 + 6 + 13], b"Una Sensaci\xf3n");
    }
}
