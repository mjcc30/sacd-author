//! Write a stereo DSD64 DSF file (little-endian) from DSF-layout audio data,
//! with an optional ID3v2.4 tag (TIT2/TPE1/TALB, UTF-8).

use std::io::Write;
use std::path::Path;

pub const DSD64_SAMPLE_RATE: u32 = 2_822_400;
pub const BLOCK_SIZE: u32 = 4096;

fn synchsafe(n: u32) -> [u8; 4] {
    [
        ((n >> 21) & 0x7F) as u8,
        ((n >> 14) & 0x7F) as u8,
        ((n >> 7) & 0x7F) as u8,
        (n & 0x7F) as u8,
    ]
}

fn text_frame(id: &[u8; 4], text: &str) -> Vec<u8> {
    let mut payload = vec![3u8]; // UTF-8
    payload.extend_from_slice(text.as_bytes());
    let mut f = Vec::new();
    f.extend_from_slice(id);
    f.extend_from_slice(&synchsafe(payload.len() as u32));
    f.extend_from_slice(&[0, 0]);
    f.extend_from_slice(&payload);
    f
}

pub fn id3v24(title: Option<&str>, artist: Option<&str>, album: Option<&str>) -> Vec<u8> {
    let mut body = Vec::new();
    if let Some(t) = title {
        body.extend_from_slice(&text_frame(b"TIT2", t));
    }
    if let Some(a) = artist {
        body.extend_from_slice(&text_frame(b"TPE1", a));
    }
    if let Some(a) = album {
        body.extend_from_slice(&text_frame(b"TALB", a));
    }
    if body.is_empty() {
        return Vec::new();
    }
    let mut tag = b"ID3".to_vec();
    tag.extend_from_slice(&[4, 0, 0]);
    tag.extend_from_slice(&synchsafe(body.len() as u32));
    tag.extend_from_slice(&body);
    tag
}

/// Write a DSF file. `data` is the DSF-layout audio (block pairs of 8192).
pub fn write_dsf(
    path: &Path,
    data: &[u8],
    samples_per_channel: u64,
    title: Option<&str>,
    artist: Option<&str>,
    album: Option<&str>,
) -> std::io::Result<()> {
    let id3 = id3v24(title, artist, album);
    let data_size = data.len() as u64;
    let total = 28 + 52 + 12 + data_size + id3.len() as u64;
    let mut out = std::fs::File::create(path)?;
    let mut w = Vec::new();
    w.extend_from_slice(b"DSD ");
    w.extend_from_slice(&total.to_le_bytes());
    w.extend_from_slice(&28u64.to_le_bytes()); // fmt offset
    w.extend_from_slice(
        &(if id3.is_empty() {
            0
        } else {
            28 + 52 + 12 + data_size
        })
        .to_le_bytes(),
    ); // meta offset
    w.extend_from_slice(b"fmt ");
    w.extend_from_slice(&52u64.to_le_bytes());
    w.extend_from_slice(&1u32.to_le_bytes()); // version
    w.extend_from_slice(&0u32.to_le_bytes()); // DSD raw
    w.extend_from_slice(&2u32.to_le_bytes()); // stereo
    w.extend_from_slice(&2u32.to_le_bytes()); // channels
    w.extend_from_slice(&DSD64_SAMPLE_RATE.to_le_bytes());
    w.extend_from_slice(&1u32.to_le_bytes()); // bits
    w.extend_from_slice(&samples_per_channel.to_le_bytes());
    w.extend_from_slice(&BLOCK_SIZE.to_le_bytes());
    w.extend_from_slice(&0u32.to_le_bytes());
    w.extend_from_slice(b"data");
    w.extend_from_slice(&data_size.to_le_bytes());
    w.extend_from_slice(data);
    w.extend_from_slice(&id3);
    out.write_all(&w)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsf;

    #[test]
    fn written_dsf_parses_back() {
        let data = vec![0xABu8; 8192];
        let f = tempfile::NamedTempFile::new().unwrap();
        write_dsf(f.path(), &data, 32768, Some("T"), Some("A"), Some("AL")).unwrap();
        let bytes = std::fs::read(f.path()).unwrap();
        let info = dsf::parse_bytes(&bytes, f.path()).unwrap();
        assert_eq!(info.samples_per_channel, 32768);
        assert_eq!(info.data_size, 8192);
        assert_eq!(info.tags.title.as_deref(), Some("T"));
        assert_eq!(info.tags.artist.as_deref(), Some("A"));
        assert_eq!(info.tags.album.as_deref(), Some("AL"));
    }
}
