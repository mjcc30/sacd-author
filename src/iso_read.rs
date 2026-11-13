//! Read back a SACD-R ISO image: master TOC, area TOC, track lists, and
//! unpack the 2C_Audio sectors into DSF-layout data (bit-reversed back,
//! L/R de-interleaved into 4096-byte channel blocks).

use crate::sacd::{FRAME_DATA_BYTES, SECTOR_SIZE};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

pub const START_OF_MASTER_TOC: u32 = 510;

#[derive(Debug, Clone)]
pub struct IsoTrack {
    pub start_lba: u32,
    pub sector_count: u32,
    pub title: Option<String>,
    pub performer: Option<String>,
    /// DSF-layout audio data (L block 4096, R block 4096, ...), bit order
    /// restored to DSF convention.
    pub data: Vec<u8>,
}

pub struct IsoImage {
    pub album_title: Option<String>,
    pub album_artist: Option<String>,
    pub tracks: Vec<IsoTrack>,
}

fn be16(b: &[u8], o: usize) -> u16 {
    u16::from_be_bytes([b[o], b[o + 1]])
}
fn be32(b: &[u8], o: usize) -> u32 {
    u32::from_be_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

fn sec(f: &mut File, lba: u32) -> std::io::Result<Vec<u8>> {
    f.seek(SeekFrom::Start(lba as u64 * SECTOR_SIZE as u64))?;
    let mut buf = vec![0u8; SECTOR_SIZE];
    f.read_exact(&mut buf)?;
    Ok(buf)
}

fn cstr(b: &[u8], off: usize) -> Option<String> {
    if off == 0 || off >= b.len() {
        return None;
    }
    let end = b[off..]
        .iter()
        .position(|&c| c == 0)
        .map(|e| off + e)
        .unwrap_or(b.len());
    let s = String::from_utf8_lossy(&b[off..end]).into_owned();
    if s.is_empty() { None } else { Some(s) }
}

/// Parse master + area TOC and unpack every track of the stereo area.
pub fn read_iso(path: &Path) -> std::io::Result<IsoImage> {
    let mut f = File::open(path)?;
    let m = sec(&mut f, START_OF_MASTER_TOC)?;
    if &m[0..8] != b"SACDMTOC" {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "no SACDMTOC at LBA 510",
        ));
    }
    let area1_start = be32(&m, 0x40);
    let area1_size = be16(&m, 0x54) as u32;

    // master SACDText (sector 511): album title/artist positions
    let mt = sec(&mut f, START_OF_MASTER_TOC + 1)?;
    let album_title = if &mt[0..8] == b"SACDText" {
        cstr(&mt, be16(&mt, 16) as usize)
    } else {
        None
    };
    let album_artist = if &mt[0..8] == b"SACDText" {
        cstr(&mt, be16(&mt, 18) as usize)
    } else {
        None
    };

    // scan area TOC sectors for header, TRL1, TRL2, SACDTTxt
    let mut area_hdr = None;
    let mut trl1 = None;
    let mut ttxt = None;
    for i in 0..area1_size {
        let s = sec(&mut f, area1_start + i)?;
        match &s[0..8] {
            b"TWOCHTOC" | b"MULCHTOC" if area_hdr.is_none() => area_hdr = Some(s),
            b"SACDTRL1" => trl1 = Some(s),
            b"SACDTTxt" => ttxt = Some(s),
            _ => {}
        }
    }
    let a = area_hdr
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "no area TOC"))?;
    let track_count = a[0x45] as usize;
    let trl1 =
        trl1.ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "no SACDTRL1"))?;

    let mut tracks = Vec::with_capacity(track_count);
    for i in 0..track_count {
        let start = be32(&trl1, 8 + i * 4);
        let len = be32(&trl1, 8 + 255 * 4 + i * 4);
        let (title, performer) = read_track_text(ttxt.as_deref(), i);
        let packed = unpack_track(&mut f, start, len)?;
        let data = dsf_layout(&packed);
        tracks.push(IsoTrack {
            start_lba: start,
            sector_count: len,
            title,
            performer,
            data,
        });
    }
    Ok(IsoImage {
        album_title,
        album_artist,
        tracks,
    })
}

fn read_track_text(ttxt: Option<&[u8]>, idx: usize) -> (Option<String>, Option<String>) {
    let Some(t) = ttxt else { return (None, None) };
    let pos = be16(t, 8 + idx * 2) as usize;
    if pos == 0 || pos >= t.len() {
        return (None, None);
    }
    let amount = t[pos] as usize;
    let mut p = pos + 4;
    let mut title = None;
    let mut performer = None;
    for _ in 0..amount {
        if p + 2 >= t.len() {
            break;
        }
        let ttype = t[p];
        p += 2; // type + 0x20
        let end = t[p..]
            .iter()
            .position(|&c| c == 0)
            .map(|e| p + e)
            .unwrap_or(t.len());
        let s = String::from_utf8_lossy(&t[p..end]).into_owned();
        match ttype {
            0x01 => title = Some(s),
            0x02 => performer = Some(s),
            _ => {}
        }
        p = end + 1;
    }
    (title, performer)
}

/// Unpack the sectors of one track into the interleaved (still bit-reversed)
/// payload stream.
fn unpack_track(f: &mut File, start: u32, len: u32) -> std::io::Result<Vec<u8>> {
    let mut out = Vec::with_capacity(len as usize * SECTOR_SIZE);
    let mut cur = start;
    while cur < start + len {
        let s = sec(f, cur)?;
        let h = s[0];
        let fic = ((h >> 2) & 7) as usize;
        let pic = ((h >> 5) & 7) as usize;
        let mut pos = 1usize;
        for _ in 0..pic {
            let b0 = s[pos];
            let plen = (((b0 & 7) as usize) << 8) | s[pos + 1] as usize;
            pos += 2;
            if (b0 >> 7) & 1 == 1 {
                pos += 3 * fic;
            }
            out.extend_from_slice(&s[pos..pos + plen]);
            pos += plen;
        }
        cur += 1;
    }
    Ok(out)
}

/// Undo the SACD bit-reversal and de-interleave L/R bytes into the DSF
/// block layout ([L 4096][R 4096] per pair).
pub fn dsf_layout(packed: &[u8]) -> Vec<u8> {
    let data: Vec<u8> = packed.iter().map(|b| b.reverse_bits()).collect();
    let pairs = data.len() / SECTOR_PAIR_BYTES_LOCAL;
    let mut out = Vec::with_capacity(pairs * SECTOR_PAIR_BYTES_LOCAL);
    for p in 0..pairs {
        let pair = &data[p * SECTOR_PAIR_BYTES_LOCAL..(p + 1) * SECTOR_PAIR_BYTES_LOCAL];
        for i in 0..SECTOR_PAIR_BYTES_LOCAL / 2 {
            out.push(pair[2 * i]);
        }
        for i in 0..SECTOR_PAIR_BYTES_LOCAL / 2 {
            out.push(pair[2 * i + 1]);
        }
    }
    out
}

const SECTOR_PAIR_BYTES_LOCAL: usize = 2 * 4096;

/// Samples per channel for a track from its emitted frame count.
pub fn samples_from_frames(frames: u64) -> u64 {
    frames * (FRAME_DATA_BYTES as u64 / 2) * 8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dsf_layout_roundtrip() {
        // interleaved L/R stream -> dsf layout -> back
        let inter: Vec<u8> = (0..8192u32).map(|i| (i % 251) as u8).collect();
        let rev: Vec<u8> = inter.iter().map(|b| b.reverse_bits()).collect();
        let dsf = dsf_layout(&rev);
        assert_eq!(dsf.len(), 8192);
        assert_eq!(
            &dsf[..4096],
            &inter[0..8192]
                .iter()
                .step_by(2)
                .copied()
                .collect::<Vec<_>>()[..]
        );
        assert_eq!(
            &dsf[4096..],
            &inter[1..8192]
                .iter()
                .step_by(2)
                .copied()
                .collect::<Vec<_>>()[..]
        );
    }
}
