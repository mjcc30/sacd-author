use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SacdError {
    #[error("failed to read {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("{path}: not a DSF file (bad magic, expected 'DSD ')")]
    BadMagic { path: PathBuf },

    #[error("{path}: truncated DSF header (need {needed} bytes, got {got})")]
    TruncatedHeader {
        path: PathBuf,
        needed: usize,
        got: usize,
    },

    #[error("{path}: unsupported DSF format version {version} (only version 1 is supported)")]
    UnsupportedVersion { path: PathBuf, version: u32 },

    #[error("{path}: unsupported channel count {channels} (only stereo, 2 channels, is supported)")]
    UnsupportedChannels { path: PathBuf, channels: u32 },

    #[error("{path}: unsupported sample rate {rate} Hz (only DSD64 2822400 Hz is supported)")]
    UnsupportedSampleRate { path: PathBuf, rate: u32 },

    #[error("{path}: missing 'fmt ' chunk at offset {offset}")]
    MissingFmtChunk { path: PathBuf, offset: u64 },

    #[error("{path}: missing 'data' chunk")]
    MissingDataChunk { path: PathBuf },

    #[error("{path}: ID3 parse error: {msg}")]
    Id3 { path: PathBuf, msg: String },
}

pub type Result<T> = std::result::Result<T, SacdError>;
