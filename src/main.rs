use anyhow::{Context, Result};
use clap::Parser;
use sacd_author::{dsf, image};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "sacd-author",
    version,
    about = "Convert DSF files into a burnable SACD-R ISO image"
)]
struct Cli {
    /// Input DSF files (stereo DSD64), in track order
    #[arg(required = true, value_name = "DSF_FILES")]
    inputs: Vec<PathBuf>,

    /// Output ISO image path
    #[arg(short, long, default_value = "output.iso")]
    output: PathBuf,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    for input in &cli.inputs {
        let info =
            dsf::parse(input).with_context(|| format!("failed to parse {}", input.display()))?;
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
    }
    let infos: Vec<_> = cli
        .inputs
        .iter()
        .map(|p| dsf::parse(p).with_context(|| format!("failed to parse {}", p.display())))
        .collect::<Result<_>>()?;
    println!("writing {} ...", cli.output.display());
    let summary = image::build_image(&infos, &cli.output)
        .with_context(|| format!("failed to write {}", cli.output.display()))?;
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
