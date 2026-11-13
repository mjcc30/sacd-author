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

fn be_chunk(id: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut c = id.to_vec();
    c.extend_from_slice(&(body.len() as u64).to_be_bytes());
    c.extend_from_slice(body);
    if body.len() % 2 == 1 {
        c.push(0);
    }
    c
}

/// Convert DSF-layout audio ([ch0 blk 4096][ch1 blk 4096]..., LSB-first bit
/// order) to DFF layout: channel bytes interleaved sample-wise ("clustered
/// frames", DSDIFF 1.5 §3.3), native MSB-first bit order.
pub fn dsf_layout_to_dff(data: &[u8], channels: usize) -> Vec<u8> {
    let block = BLOCK_SIZE as usize;
    assert!(data.len().is_multiple_of(block * channels));
    let mut out = Vec::with_capacity(data.len());
    let nblocks = data.len() / (block * channels);
    for b in 0..nblocks {
        for i in 0..block {
            for ch in 0..channels {
                out.push(data[(b * channels + ch) * block + i].reverse_bits());
            }
        }
    }
    out
}

/// Write a DSD64 DFF (DSDIFF) file with `channels` channels. `data` is the
/// DSD chunk payload in DFF layout: channel bytes interleaved sample-wise,
/// native bit order (e.g. produced by `dsf_layout_to_dff`, or the raw
/// payload of a multichannel SACD area).
pub fn write_dff(
    path: &Path,
    data: &[u8],
    channels: u32,
    title: Option<&str>,
    artist: Option<&str>,
    album: Option<&str>,
) -> std::io::Result<()> {
    let mut prop = b"SND ".to_vec();
    prop.extend_from_slice(&be_chunk(b"FS  ", &DSD64_SAMPLE_RATE.to_be_bytes()));
    prop.extend_from_slice(&be_chunk(b"CHNL", &(channels as u16).to_be_bytes()));
    let mut cmpr = b"DSD ".to_vec();
    cmpr.push(0);
    prop.extend_from_slice(&be_chunk(b"CMPR", &cmpr));

    let mut chunks = Vec::new();
    chunks.extend_from_slice(&be_chunk(b"FVER", &0x0105_0000u32.to_be_bytes()));
    chunks.extend_from_slice(&be_chunk(b"PROP", &prop));
    chunks.extend_from_slice(&be_chunk(b"DSD ", data));
    let id3 = id3v24(title, artist, album);
    if !id3.is_empty() {
        chunks.extend_from_slice(&be_chunk(b"ID3 ", &id3));
    }
    let mut out = b"FRM8".to_vec();
    out.extend_from_slice(&((chunks.len() + 4) as u64).to_be_bytes());
    out.extend_from_slice(b"DSD ");
    out.extend_from_slice(&chunks);
    let mut f = std::fs::File::create(path)?;
    f.write_all(&out)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsf;

    #[test]
    fn written_dff_parses_back() {
        // DFF layout: stereo clustered frames (L R L R ...), native order
        let data: Vec<u8> = (0..8192u32).map(|i| (i % 251) as u8).collect();
        let f = tempfile::NamedTempFile::new().unwrap();
        write_dff(f.path(), &data, 2, Some("T"), Some("A"), Some("AL")).unwrap();
        let bytes = std::fs::read(f.path()).unwrap();
        let info = crate::dff::parse_bytes(&bytes, f.path()).unwrap();
        assert_eq!(info.samples_per_channel, 32768);
        assert_eq!(info.data_size, 8192);
        assert_eq!(info.channels, 2);
        assert_eq!(
            &bytes[info.data_offset as usize..info.data_offset as usize + 8192],
            &data[..]
        );
        assert_eq!(info.tags.title.as_deref(), Some("T"));
    }

    #[test]
    fn dsf_layout_to_dff_interleaves_and_reverses() {
        // 1 block pair: L block = 0x01..., R block = 0x80...
        let mut dsf = vec![0x01u8; 4096];
        dsf.extend_from_slice(&[0x80u8; 4096]);
        let dff = dsf_layout_to_dff(&dsf, 2);
        assert_eq!(dff.len(), 8192);
        assert_eq!(dff[0], 0x01u8.reverse_bits());
        assert_eq!(dff[1], 0x80u8.reverse_bits());
        // and a 4-channel case round-trips through the DFF parser
        let mc: Vec<u8> = (0..4 * 4096u32).map(|i| (i % 253) as u8).collect();
        // build DSF-layout 4ch: [c0 blk][c1 blk][c2 blk][c3 blk]
        let mut dsf4 = vec![0u8; 4 * 4096];
        for i in 0..4096 {
            for ch in 0..4 {
                dsf4[ch * 4096 + i] = mc[i * 4 + ch];
            }
        }
        let dff4 = dsf_layout_to_dff(&dsf4, 4);
        let rev: Vec<u8> = dff4.iter().map(|b| b.reverse_bits()).collect();
        assert_eq!(rev, mc);
    }

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
