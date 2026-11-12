//! Minimal ID3v2 (v2.3/v2.4) parser for text frames (TIT2, TPE1, TALB).

use crate::error::{Result, SacdError};
use std::path::Path;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Id3Tags {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
}

fn synchsafe_to_u32(bytes: &[u8; 4]) -> u32 {
    ((bytes[0] as u32 & 0x7F) << 21)
        | ((bytes[1] as u32 & 0x7F) << 14)
        | ((bytes[2] as u32 & 0x7F) << 7)
        | (bytes[3] as u32 & 0x7F)
}

fn decode_text(encoding: u8, data: &[u8]) -> String {
    if data.is_empty() {
        return String::new();
    }
    match encoding {
        0 => data
            .iter()
            .map(|&b| b as char)
            .collect::<String>()
            .trim_end_matches('\0')
            .to_string(),
        3 => String::from_utf8_lossy(data)
            .trim_end_matches('\0')
            .to_string(),
        1 | 2 => {
            let has_bom = data.starts_with(&[0xFF, 0xFE]) || data.starts_with(&[0xFE, 0xFF]);
            let be = if has_bom {
                data.starts_with(&[0xFE, 0xFF])
            } else {
                encoding == 2
            };
            let payload = if has_bom { &data[2..] } else { data };
            let units: Vec<u16> = payload
                .as_chunks::<2>()
                .0
                .iter()
                .map(|c| {
                    if be {
                        u16::from_be_bytes([c[0], c[1]])
                    } else {
                        u16::from_le_bytes([c[0], c[1]])
                    }
                })
                .collect();
            String::from_utf16_lossy(&units)
                .trim_end_matches('\0')
                .to_string()
        }
        _ => String::new(),
    }
}

/// Parse ID3v2 tags from the tail of a file. Returns `None` if no ID3v2 tag is present.
pub fn parse_id3_tail(data: &[u8], path: &Path) -> Result<Option<Id3Tags>> {
    if data.len() < 10 {
        return Ok(None);
    }
    let pos = data
        .windows(3)
        .rposition(|w| w == b"ID3")
        .filter(|&p| p + 10 <= data.len());
    let Some(start) = pos else {
        return Ok(None);
    };
    let header = &data[start..start + 10];
    let major = header[3];
    if !(3..=4).contains(&major) {
        return Ok(None);
    }
    let tag_size = synchsafe_to_u32(header[6..10].try_into().map_err(|_| SacdError::Id3 {
        path: path.to_path_buf(),
        msg: "bad synchsafe size".into(),
    })?) as usize;
    let body_end = (start + 10 + tag_size).min(data.len());
    let body = &data[start + 10..body_end];

    let mut tags = Id3Tags::default();
    let mut off = 0usize;
    while off + 10 <= body.len() {
        let frame_id = &body[off..off + 4];
        if frame_id.contains(&0) {
            break;
        }
        let frame_size =
            if major == 4 {
                synchsafe_to_u32(
                    body[off + 4..off + 8]
                        .try_into()
                        .map_err(|_| SacdError::Id3 {
                            path: path.to_path_buf(),
                            msg: "bad frame size".into(),
                        })?,
                ) as usize
            } else {
                u32::from_be_bytes(body[off + 4..off + 8].try_into().map_err(|_| {
                    SacdError::Id3 {
                        path: path.to_path_buf(),
                        msg: "bad frame size".into(),
                    }
                })?) as usize
            };
        if frame_size == 0 || off + 10 + frame_size > body.len() {
            break;
        }
        let content = &body[off + 10..off + 10 + frame_size];
        if !content.is_empty() {
            let text = decode_text(content[0], &content[1..]);
            match frame_id {
                b"TIT2" => tags.title = Some(text),
                b"TPE1" => tags.artist = Some(text),
                b"TALB" => tags.album = Some(text),
                _ => {}
            }
        }
        off += 10 + frame_size;
    }
    Ok(Some(tags))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synchsafe(n: u32) -> [u8; 4] {
        [
            ((n >> 21) & 0x7F) as u8,
            ((n >> 14) & 0x7F) as u8,
            ((n >> 7) & 0x7F) as u8,
            (n & 0x7F) as u8,
        ]
    }

    fn text_frame(id: &[u8; 4], text: &str) -> Vec<u8> {
        let mut f = Vec::new();
        f.extend_from_slice(id);
        let payload = {
            let mut p = vec![3u8];
            p.extend_from_slice(text.as_bytes());
            p
        };
        f.extend_from_slice(&synchsafe(payload.len() as u32));
        f.extend_from_slice(&[0, 0]);
        f.extend_from_slice(&payload);
        f
    }

    #[test]
    fn parses_v24_text_frames() {
        let path = Path::new("test.dsf");
        let mut body = Vec::new();
        body.extend_from_slice(&text_frame(b"TIT2", "A Title"));
        body.extend_from_slice(&text_frame(b"TPE1", "An Artist"));
        body.extend_from_slice(&text_frame(b"TALB", "An Album"));
        let mut file = b"ID3".to_vec();
        file.push(4);
        file.push(0);
        file.push(0);
        file.extend_from_slice(&synchsafe(body.len() as u32));
        file.extend_from_slice(&body);
        let tags = parse_id3_tail(&file, path).unwrap().unwrap();
        assert_eq!(tags.title.as_deref(), Some("A Title"));
        assert_eq!(tags.artist.as_deref(), Some("An Artist"));
        assert_eq!(tags.album.as_deref(), Some("An Album"));
    }

    #[test]
    fn returns_none_without_id3() {
        let path = Path::new("test.dsf");
        assert!(parse_id3_tail(b"no tags here", path).unwrap().is_none());
    }

    #[test]
    fn decodes_utf16_with_bom() {
        let path = Path::new("test.dsf");
        let mut payload = vec![1u8, 0xFF, 0xFE];
        for u in "Hi".encode_utf16() {
            payload.extend_from_slice(&u.to_le_bytes());
        }
        let mut body = Vec::new();
        body.extend_from_slice(b"TIT2");
        body.extend_from_slice(&synchsafe(payload.len() as u32));
        body.extend_from_slice(&[0, 0]);
        body.extend_from_slice(&payload);
        let mut file = b"ID3".to_vec();
        file.extend_from_slice(&[4, 0, 0]);
        file.extend_from_slice(&synchsafe(body.len() as u32));
        file.extend_from_slice(&body);
        let tags = parse_id3_tail(&file, path).unwrap().unwrap();
        assert_eq!(tags.title.as_deref(), Some("Hi"));
    }
}
