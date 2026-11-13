//! DFF/DSDIFF parser (big-endian chunks). Produces the same DsfInfo as the
//! DSF parser, but with bit_reverse_needed = false: DFF stores DSD bytes in
//! the native SACD bit order, while DSF stores them bit-reversed
//! (see sacd-ripper's bit_reverse_table in dsf.c).
//!
//! Layout: "FRM8" + u64 BE size + "DSD " form, then chunks (id[4] + u64 BE
//! size + data, padded to even): "FVER", "PROP" ("SND " + "FS  "/"CHNL"/
//! "CMPR"/"LSCO"), "DSD " (audio: channel bytes interleaved sample-wise
//! = "clustered frames" of numChannels bytes, per DSDIFF 1.5 spec 3.3,
//! native SACD bit order), optional "ID3 " chunk with an ID3v2 tag.

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
    // DSF is stereo-only by spec; DFF also carries the SACD multichannel
    // area (up to 6 channels), which we accept for MC extraction/conversion.
    if !(2..=6).contains(&channels) {
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
        packing: crate::dsf::AudioPacking::Interleaved,
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

    #[test]
    fn accepts_multichannel_and_rejects_bad_counts() {
        let path = Path::new("mc.dff");
        // 4 channels, byte-interleaved clustered frames (DSDIFF 1.5)
        let audio = vec![0x11; 4 * 4096];
        let d = crate::testutil::make_test_dff(&audio, 4, None);
        let info = parse_bytes(&d, path).unwrap();
        assert_eq!(info.channels, 4);
        assert_eq!(info.samples_per_channel, 4096 * 8);
        assert_eq!(info.packing, crate::dsf::AudioPacking::Interleaved);
        let d7 = crate::testutil::make_test_dff(&vec![0; 7 * 4096], 7, None);
        assert!(matches!(
            parse_bytes(&d7, path),
            Err(SacdError::UnsupportedChannels { channels: 7, .. })
        ));
    }

    #[test]
    fn parses_dff_header_and_audio() {
        let path = Path::new("t.dff");
        let d = crate::testutil::make_test_dff(&vec![0xD5; 8192], 2, Some("DFF Song"));
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

        let dff_data = crate::testutil::make_test_dff(&vec![0xD5; 8192], 2, None);
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
