//! DFF/DSDIFF parser (big-endian chunks). Produces the same DsfInfo as the
//! DSF parser, but with bit_reverse_needed = false: DFF stores DSD bytes in
//! the native SACD bit order, while DSF stores them bit-reversed
//! (see sacd-ripper's bit_reverse_table in dsf.c).
//!
//! Layout: "FRM8" + u64 BE size + "DSD " form, then chunks (id[4] + u64 BE
//! size + data, padded to even): "FVER", "PROP" ("SND" + "FS  "/"CHNL"/
//! "CMPR"/"LSCO"), "DSD " (audio blocks, 4096 bytes per channel interleaved
//! block by block, same as DSF), optional "ID3 " chunk with an ID3v2 tag.

use crate::dsf::DsfInfo;
use crate::error::{Result, SacdError};
use crate::id3;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

fn be32(b: &[u8], o: usize) -> u32 {
    u32::from_be_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}
fn be16(b: &[u8], o: usize) -> u16 {
    u16::from_be_bytes([b[o], b[o + 1]])
}
fn be64(b: &[u8], o: usize) -> u64 {
    u64::from_be_bytes(b[o..o + 8].try_into().expect("len checked"))
}

/// Parse a DFF from an in-memory buffer.
pub fn parse_bytes(data: &[u8], path: &Path) -> Result<DsfInfo> {
    if data.len() < 16 || &data[0..4] != b"FRM8" || &data[12..16] != b"DSD " {
        return Err(SacdError::BadMagic {
            path: path.to_path_buf(),
        });
    }
    let mut sample_rate = 0u32;
    let mut channels = 0u16;
    let mut data_offset = 0u64;
    let mut data_size = 0u64;
    let mut id3_chunk = None;

    let mut pos = 16usize;
    while pos + 12 <= data.len() {
        let id = &data[pos..pos + 4];
        let size = be64(data, pos + 4) as usize;
        let body = pos + 12;
        if body + size > data.len() {
            break;
        }
        #[cfg(test)]
        if pos == 32 {
            eprintln!("hex 28..80: {:02x?}", &data[28..80]);
        }
        match id {
            b"PROP" => {
                // PROP: "SND" then sub-chunks
                let mut q = body + 4;
                while q + 12 <= body + size {
                    let sid = &data[q..q + 4];
                    let ssize = be64(data, q + 4) as usize;
                    match sid {
                        b"FS  " => sample_rate = be32(data, q + 12),
                        b"CHNL" => channels = be16(data, q + 12),
                        _ => {}
                    }
                    q += 12 + ssize + (ssize % 2);
                }
            }
            b"DSD " => {
                data_offset = body as u64;
                data_size = size as u64;
            }
            b"ID3 " => id3_chunk = Some(&data[body..body + size]),
            _ => {}
        }
        pos = body + size + (size % 2);
    }

    if data_offset == 0 {
        return Err(SacdError::MissingDataChunk {
            path: path.to_path_buf(),
        });
    }
    if sample_rate != crate::dsf::DSD64_SAMPLE_RATE {
        return Err(SacdError::UnsupportedSampleRate {
            path: path.to_path_buf(),
            rate: sample_rate,
        });
    }
    if channels != 2 {
        return Err(SacdError::UnsupportedChannels {
            path: path.to_path_buf(),
            channels: channels as u32,
        });
    }
    let samples_per_channel = data_size / channels as u64 * 8;
    let tags = id3_chunk
        .and_then(|c| id3::parse_id3_tail(c, path).ok().flatten())
        .unwrap_or_default();

    Ok(DsfInfo {
        path: path.to_path_buf(),
        sample_rate,
        channels: channels as u32,
        samples_per_channel,
        block_size: crate::dsf::BLOCK_SIZE,
        data_offset,
        data_size,
        tags,
        bit_reverse_needed: false,
        packing: crate::dsf::AudioPacking::Blocks,
    })
}

/// Parse a DFF file from disk.
pub fn parse(path: &Path) -> Result<DsfInfo> {
    let mut file = File::open(path).map_err(|source| SacdError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let len = file
        .metadata()
        .map_err(|source| SacdError::Io {
            path: path.to_path_buf(),
            source,
        })?
        .len() as usize;
    // header + TOC area; read whole file if small, else read progressively
    let head_len = len.min(1024 * 1024);
    file.seek(SeekFrom::Start(0))
        .map_err(|source| SacdError::Io {
            path: path.to_path_buf(),
            source,
        })?;
    let mut head = vec![0u8; head_len];
    file.read_exact(&mut head).map_err(|source| SacdError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    // scan chunks in the head; if the DSD chunk extends beyond, parse_bytes on
    // head still finds offsets; data_size may be truncated — recompute from file len
    let mut info = parse_bytes(&head, path)?;
    if info.data_offset + info.data_size > len as u64 {
        info.data_size = len as u64 - info.data_offset;
        info.samples_per_channel = info.data_size / info.channels as u64 * 8;
    }
    // ID3 may live at the end of the file
    if info.tags.title.is_none() {
        let tail_len = len.min(1024 * 1024);
        file.seek(SeekFrom::Start(len as u64 - tail_len as u64))
            .map_err(|source| SacdError::Io {
                path: path.to_path_buf(),
                source,
            })?;
        let mut tail = vec![0u8; tail_len];
        file.read_exact(&mut tail).map_err(|source| SacdError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        if let Ok(Some(tags)) = id3::parse_id3_tail(&tail, path) {
            info.tags = tags;
        }
    }
    Ok(info)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// Build a minimal DFF with one block pair of `fill` audio data.
    pub fn make_test_dff(fill: u8, title: Option<&str>) -> Vec<u8> {
        let audio = vec![fill; 8192];
        let mut prop = b"SND ".to_vec();
        let fs = {
            let mut c = b"FS  ".to_vec();
            c.extend_from_slice(&4u64.to_be_bytes());
            c.extend_from_slice(&2_822_400u32.to_be_bytes());
            c
        };
        let chnl = {
            let mut c = b"CHNL".to_vec();
            c.extend_from_slice(&2u64.to_be_bytes());
            c.extend_from_slice(&2u16.to_be_bytes());
            c
        };
        let cmpr = {
            let mut c = b"CMPR".to_vec();
            c.extend_from_slice(&5u64.to_be_bytes());
            c.extend_from_slice(b"DSD ");
            c.push(0);
            c
        };
        prop.extend_from_slice(&fs);
        prop.extend_from_slice(&chnl);
        prop.extend_from_slice(&cmpr);
        let mut prop_chunk = b"PROP".to_vec();
        prop_chunk.extend_from_slice(&(prop.len() as u64).to_be_bytes());
        prop_chunk.extend_from_slice(&prop);

        let mut id3_body = Vec::new();
        if let Some(t) = title {
            let mut payload = vec![3u8];
            payload.extend_from_slice(t.as_bytes());
            let synch = |n: u32| {
                [
                    (n >> 21 & 0x7F) as u8,
                    (n >> 14 & 0x7F) as u8,
                    (n >> 7 & 0x7F) as u8,
                    (n & 0x7F) as u8,
                ]
            };
            let mut body = Vec::new();
            body.extend_from_slice(b"TIT2");
            body.extend_from_slice(&synch(payload.len() as u32));
            body.extend_from_slice(&[0, 0]);
            body.extend_from_slice(&payload);
            id3_body.extend_from_slice(b"ID3");
            id3_body.extend_from_slice(&[4, 0, 0]);
            id3_body.extend_from_slice(&synch(body.len() as u32));
            id3_body.extend_from_slice(&body);
        }
        let id3 = id3_body;

        fn chunk(id: &[u8; 4], body: &[u8]) -> Vec<u8> {
            let mut c = id.to_vec();
            c.extend_from_slice(&(body.len() as u64).to_be_bytes());
            c.extend_from_slice(body);
            if body.len() % 2 == 1 {
                c.push(0);
            }
            c
        }
        let fver = chunk(b"FVER", &0x01050000u32.to_be_bytes());
        let prop_chunk = chunk(b"PROP", &prop);
        let dsd_chunk = chunk(b"DSD ", &audio);
        let id3c = if id3.is_empty() {
            Vec::new()
        } else {
            chunk(b"ID3 ", &id3)
        };
        let body_len = fver.len() + prop_chunk.len() + dsd_chunk.len() + id3c.len();
        let mut out = b"FRM8".to_vec();
        out.extend_from_slice(&(body_len as u64 + 4).to_be_bytes());
        out.extend_from_slice(b"DSD ");
        out.extend_from_slice(&fver);
        out.extend_from_slice(&prop_chunk);
        out.extend_from_slice(&dsd_chunk);
        out.extend_from_slice(&id3c);
        out
    }

    #[test]
    fn parses_dff_header_and_audio() {
        let path = Path::new("t.dff");
        let d = make_test_dff(0xD5, Some("DFF Song"));
        let info = parse_bytes(&d, path).unwrap();
        assert_eq!(info.sample_rate, 2_822_400);
        assert_eq!(info.channels, 2);
        assert_eq!(info.samples_per_channel, 32768);
        assert_eq!(info.data_size, 8192);
        assert!(!info.bit_reverse_needed);
        assert_eq!(info.tags.title.as_deref(), Some("DFF Song"));
    }

    #[test]
    fn dff_and_dsf_produce_same_sacd_stream() {
        use crate::sacd;
        // DSF with 0xAB bit-reverses to 0xD5 on SACD; DFF with 0xD5 stays 0xD5.
        let dsf_data = crate::testutil::make_test_dsf(32768, false);
        let mut dsf_file = tempfile::NamedTempFile::new().unwrap();
        dsf_file.write_all(&dsf_data).unwrap();
        let dsf_info = crate::dsf::parse_bytes(&dsf_data, dsf_file.path()).unwrap();

        let dff_data = make_test_dff(0xD5, None);
        let mut dff_file = tempfile::NamedTempFile::new().unwrap();
        dff_file.write_all(&dff_data).unwrap();
        let dff_info = parse_bytes(&dff_data, dff_file.path()).unwrap();

        let mut a = Vec::new();
        let mut b = Vec::new();
        sacd::convert_track(&dsf_info, &mut a).unwrap();
        sacd::convert_track(&dff_info, &mut b).unwrap();
        assert_eq!(a, b);
    }
}
