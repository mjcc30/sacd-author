use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use sacd_author::{data_iso, dff, dop, dsd2pcm, dsf, dsf_write, image, iso_read};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "sacd-author",
    version,
    about = "Convert DSF/DFF/DoP-WAV files into a burnable SACD-R ISO image, and extract DSF/DFF tracks back from an ISO"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Legacy form: sacd-author <input files...> -o out.iso
    #[arg(value_name = "FILES")]
    inputs: Vec<PathBuf>,

    #[arg(short, long, default_value = "output.iso")]
    output: PathBuf,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Convert DSF/DFF/DoP-WAV files (stereo DSD64) into a SACD-R ISO image
    Convert {
        #[arg(required = true, value_name = "FILES")]
        inputs: Vec<PathBuf>,
        #[arg(short, long, default_value = "output.iso")]
        output: PathBuf,
    },
    /// Extract tracks from a SACD-R ISO image (DSF or DFF)
    Extract {
        #[arg(value_name = "ISO")]
        iso: PathBuf,
        /// Output directory
        #[arg(short, long, default_value = "extracted")]
        output: PathBuf,
        /// Output format: dsf (default) or dff
        #[arg(short = 'f', long, default_value = "dsf")]
        format: String,
    },
    /// Build a plain ISO 9660 + Joliet data image from files (FLAC/WAV/...),
    /// readable by any modern player from DVD-R (e.g. Sony BDP-S490 plays FLAC)
    DataIso {
        #[arg(required = true, value_name = "FILES")]
        inputs: Vec<PathBuf>,
        #[arg(short, long, default_value = "data.iso")]
        output: PathBuf,
    },
    /// Convert DSD (DSF/DFF/DoP-WAV file, or a SACD-R ISO) to PCM: WAV, FLAC
    /// or WavPack. With an .iso input, -o is a directory (one file per track).
    #[command(name = "dsd2pcm")]
    Dsd2Pcm {
        #[arg(value_name = "INPUT")]
        input: PathBuf,
        /// Output file (single input) or directory (ISO input)
        #[arg(short, long)]
        output: PathBuf,
        /// Target PCM sample rate
        #[arg(short = 'r', long, default_value = "96000")]
        rate: u32,
        /// Bit depth: 16 or 24
        #[arg(short = 'b', long, default_value = "24")]
        bits: u8,
        /// Low-pass cutoff in Hz (default: min(50000, 0.45*rate))
        #[arg(long)]
        lowpass: Option<u32>,
        /// Gain in dB applied after filtering (default +6 dB)
        #[arg(long, default_value = "6.0")]
        gain: f64,
        /// Output format when it can't be guessed from the output extension
        /// (flac, wav, wv); used as-is for ISO inputs
        #[arg(short = 'f', long, default_value = "flac")]
        format: String,
    },
}

/// Parse a DSF, DFF or DoP-WAV input based on extension.
fn parse_input(path: &std::path::Path) -> sacd_author::Result<sacd_author::dsf::DsfInfo> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "dff" => dff::parse(path),
        "wav" => dop::parse(path),
        _ => dsf::parse(path),
    }
}

fn convert(inputs: &[PathBuf], output: &std::path::Path) -> Result<()> {
    let mut infos = Vec::new();
    for input in inputs {
        let info =
            parse_input(input).with_context(|| format!("failed to parse {}", input.display()))?;
        println!(
            "{}: {} Hz, {} ch, {} samples, {:.2}s, {} SACD frames",
            input.display(),
            info.sample_rate,
            info.channels,
            info.samples_per_channel,
            info.duration_secs(),
            info.sacd_frames()
        );
        if let Some(title) = &info.tags.title {
            println!("  title:  {title}");
        }
        if let Some(artist) = &info.tags.artist {
            println!("  artist: {artist}");
        }
        if let Some(album) = &info.tags.album {
            println!("  album:  {album}");
        }
        infos.push(info);
    }
    println!("writing {} ...", output.display());
    let summary = image::build_image(&infos, output)
        .with_context(|| format!("failed to write {}", output.display()))?;
    println!(
        "done: {} sectors total, {} audio sectors, {} track(s)",
        summary.total_sectors,
        summary.audio_sectors,
        summary.layouts.len()
    );
    for (i, layout) in summary.layouts.iter().enumerate() {
        println!(
            "  track {}: LBA {} ({} sectors, {} frames)",
            i + 1,
            image::AUDIO_START_LBA + layout.start_sector,
            layout.sector_count,
            layout.frames
        );
    }
    Ok(())
}

fn extract(iso: &std::path::Path, output: &std::path::Path, format: &str) -> Result<()> {
    let img =
        iso_read::read_iso(iso).with_context(|| format!("failed to read {}", iso.display()))?;
    std::fs::create_dir_all(output)?;
    println!(
        "{}: {} track(s){}",
        iso.display(),
        img.tracks.len(),
        img.album_title
            .as_ref()
            .map(|t| format!(", album \"{t}\""))
            .unwrap_or_default()
    );
    for (i, track) in img.tracks.iter().enumerate() {
        let name = track
            .title
            .clone()
            .unwrap_or_else(|| format!("track{:02}", i + 1));
        let safe: String = name
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == ' ' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        let ext = if format == "dff" { "dff" } else { "dsf" };
        let path = output.join(format!("{:02} {safe}.{ext}", i + 1));
        let samples = track.data.len() as u64 / 2 * 8;
        match ext {
            "dff" => dsf_write::write_dff(
                &path,
                &track.data,
                samples,
                track.title.as_deref(),
                track.performer.as_deref(),
                img.album_title.as_deref(),
            )?,
            _ => dsf_write::write_dsf(
                &path,
                &track.data,
                samples,
                track.title.as_deref(),
                track.performer.as_deref(),
                img.album_title.as_deref(),
            )?,
        }
        println!(
            "  {} ({} bytes, {:.2}s)",
            path.display(),
            track.data.len(),
            samples as f64 / 2_822_400.0
        );
    }
    Ok(())
}

fn data_iso_cmd(inputs: &[PathBuf], output: &std::path::Path) -> Result<()> {
    let mut entries = Vec::new();
    for input in inputs {
        let name = input
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| anyhow::anyhow!("{}: invalid file name", input.display()))?
            .to_string();
        entries.push(data_iso::DataEntry {
            name,
            path: input.clone(),
        });
    }
    let volume_id = data_iso::volume_id_from(output);
    println!(
        "writing {} ({} file(s)) ...",
        output.display(),
        entries.len()
    );
    let sectors = data_iso::build_data_iso(&entries, output, &volume_id)
        .with_context(|| format!("failed to write {}", output.display()))?;
    println!(
        "done: {} sectors ({:.1} MiB), ISO 9660 + Joliet, files at root",
        sectors,
        sectors as f64 * 2048.0 / 1024.0 / 1024.0
    );
    for e in &entries {
        println!("  {}", e.name);
    }
    Ok(())
}

fn dsd2pcm_cmd(
    input: &std::path::Path,
    output: &std::path::Path,
    rate: u32,
    bits: u8,
    lowpass: Option<u32>,
    gain: f64,
    format: &str,
) -> Result<()> {
    if !dsd2pcm::supported_rate(rate) {
        anyhow::bail!(
            "unsupported rate {rate} (use 44100/48000/88200/96000/176400/192000/352800/384000)"
        );
    }
    if !matches!(bits, 16 | 24) {
        anyhow::bail!("unsupported bit depth {bits} (use 16 or 24)");
    }
    let fmt = dsd2pcm::PcmFormat::from_ext(format)
        .ok_or_else(|| anyhow::anyhow!("unknown format '{format}' (use flac, wav or wv)"))?;
    let params = dsd2pcm::PcmParams {
        rate,
        bits,
        lowpass: lowpass.map(|h| h as f64),
        gain_db: gain,
    };
    let is_iso = input
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("iso"))
        .unwrap_or(false);
    println!(
        "{}: DSD64 -> PCM {} Hz, {} bit, lowpass {} Hz, gain {:+.1} dB",
        input.display(),
        rate,
        bits,
        params
            .lowpass
            .map(|h| h as u32)
            .unwrap_or(50_000.min((0.45 * rate as f64) as u32)),
        gain
    );
    if is_iso {
        let frames = dsd2pcm::convert_iso(input, output, fmt, &params)?;
        println!(
            "done: {} frame(s) at {} Hz ({:.2}s) into {}",
            frames,
            rate,
            frames as f64 / rate as f64,
            output.display()
        );
    } else {
        let frames = dsd2pcm::convert_file(input, output, Some(fmt), &params)?;
        println!(
            "done: {} -> {} frame(s) at {} Hz ({:.2}s)",
            input.display(),
            frames,
            rate,
            frames as f64 / rate as f64
        );
    }
    Ok(())
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Some(Commands::Convert { inputs, output }) => convert(&inputs, &output),
        Some(Commands::Extract {
            iso,
            output,
            format,
        }) => extract(&iso, &output, &format),
        Some(Commands::DataIso { inputs, output }) => data_iso_cmd(&inputs, &output),
        Some(Commands::Dsd2Pcm {
            input,
            output,
            rate,
            bits,
            lowpass,
            gain,
            format,
        }) => dsd2pcm_cmd(&input, &output, rate, bits, lowpass, gain, &format),
        None => {
            if cli.inputs.is_empty() {
                Cli::parse_from(["sacd-author", "--help"]);
            }
            convert(&cli.inputs, &cli.output)
        }
    }
}
