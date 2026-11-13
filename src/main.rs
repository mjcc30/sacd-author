use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use sacd_author::{dff, dsf, dsf_write, image, iso_read};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "sacd-author",
    version,
    about = "Convert DSF files into a burnable SACD-R ISO image, and extract DSF tracks back from an ISO"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Legacy form: sacd-author <dsf files...> -o out.iso
    #[arg(value_name = "DSF_FILES")]
    inputs: Vec<PathBuf>,

    #[arg(short, long, default_value = "output.iso")]
    output: PathBuf,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Convert DSF files (stereo DSD64) into a SACD-R ISO image
    Convert {
        #[arg(required = true, value_name = "DSF_FILES")]
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
}

/// Parse a DSF or DFF input based on extension.
fn parse_input(path: &std::path::Path) -> sacd_author::Result<sacd_author::dsf::DsfInfo> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "dff" => dff::parse(path),
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

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Some(Commands::Convert { inputs, output }) => convert(&inputs, &output),
        Some(Commands::Extract {
            iso,
            output,
            format,
        }) => extract(&iso, &output, &format),
        None => {
            if cli.inputs.is_empty() {
                Cli::parse_from(["sacd-author", "--help"]);
            }
            convert(&cli.inputs, &cli.output)
        }
    }
}
