use std::path::{Path, PathBuf};

use super::Resolution;
use crate::video::error::CompressionError;

/// Some decoders declare a 96x96 minimum size, and certain video players reject anything smaller
const MIN_OUTPUT_DIMENSION: i32 = 96;

/// Video dimensions (original and preview)
pub struct VideoDimensions {
    pub video_width: u32,
    pub video_height: u32,
    pub preview_width: f32,
    pub preview_height: f32,
}

/// Structured FFmpeg command for display and execution
pub struct FfmpegCommand {
    /// The full command string (for display/copy)
    pub full: String,
    /// Command with placeholders for input/output (for display)
    pub masked: String,
    /// Raw arguments for direct execution (without shell quoting)
    pub args: Vec<String>,
    /// Output path for the rendered video
    pub output_path: PathBuf,
}

use crate::utils::{CropSelection, format_timestamp_precise};

fn is_trimming(trim_start: f64, trim_end: f64, duration: f64) -> bool {
    trim_start > 0.01 || (duration > 0.0 && trim_end < duration - 0.01)
}

struct TrimArgs {
    before_input: Vec<String>, // -ss args
    after_input: Vec<String>,  // -t args
}

fn build_trim_args(trim_start: f64, trim_end: f64, duration: f64) -> TrimArgs {
    let mut before_input = Vec::new();
    let mut after_input = Vec::new();

    if is_trimming(trim_start, trim_end, duration) {
        if trim_start > 0.01 {
            before_input.push("-ss".to_string());
            before_input.push(format_timestamp_precise(trim_start));
        }

        let trim_duration = trim_end - trim_start;
        after_input.push("-t".to_string());
        after_input.push(format_timestamp_precise(trim_duration));
    }

    TrimArgs {
        before_input,
        after_input,
    }
}

/// Build an FFmpeg command from the current editing state.
///
/// Returns a structured command with both full and masked versions.
/// Crop coordinates are automatically scaled from preview space to original video dimensions.
///
/// When no video filters are needed (no crop/resize), uses `-c copy` to avoid re-encoding.
#[allow(clippy::too_many_arguments)]
pub fn build_ffmpeg_command(
    input_path: &Path,
    crop: Option<&CropSelection>,
    resolution: Option<Resolution>,
    trim_start: f64,
    trim_end: f64,
    duration: f64,
    dimen: &VideoDimensions,
    mute_audio: bool,
) -> FfmpegCommand {
    let mut display_args = vec!["ffmpeg".to_string()];
    let mut masked_args = vec!["ffmpeg".to_string()];
    let mut exec_args: Vec<String> = Vec::new();

    let trim = build_trim_args(trim_start, trim_end, duration);

    // Add seek position before input
    display_args.extend(trim.before_input.iter().cloned());
    masked_args.extend(trim.before_input.iter().cloned());
    exec_args.extend(trim.before_input.clone());

    // Input file
    let input_str = input_path.to_string_lossy().to_string();
    display_args.extend(["-i".to_string(), format!("\"{}\"", input_str)]);
    masked_args.extend(["-i".to_string(), "{input}".to_string()]);
    exec_args.extend(["-i".to_string(), input_str]);

    // Add duration after input
    display_args.extend(trim.after_input.iter().cloned());
    masked_args.extend(trim.after_input.iter().cloned());
    exec_args.extend(trim.after_input);

    // Video filters
    let video_filter = build_video_filter(crop, resolution, dimen);
    if let Some(ref filter_str) = video_filter {
        display_args.push("-vf".to_string());
        display_args.push(format!("\"{}\"", filter_str));
        masked_args.push("-vf".to_string());
        masked_args.push(format!("\"{}\"", filter_str));
        exec_args.push("-vf".to_string());
        exec_args.push(filter_str.clone());
    }

    // Use stream copy when no video filters are applied (no re-encoding needed)
    if video_filter.is_none() {
        display_args.extend(["-c".to_string(), "copy".to_string()]);
        masked_args.extend(["-c".to_string(), "copy".to_string()]);
        exec_args.extend(["-c".to_string(), "copy".to_string()]);
    }

    // Audio handling: mute removes audio stream entirely
    if mute_audio {
        display_args.push("-an".to_string());
        masked_args.push("-an".to_string());
        exec_args.push("-an".to_string());
    }

    // Generate output filename
    let stem = input_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("output");
    let ext = input_path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("mp4");

    let output_name = format!("{}_edited.{}", stem, ext);

    // Output path (same directory as input)
    let output_path = input_path
        .parent()
        .map(|p| p.join(&output_name))
        .unwrap_or_else(|| output_name.clone().into());

    let output_str = output_path.to_string_lossy().to_string();
    display_args.push(format!("\"{}\"", output_str));
    masked_args.push("{output}".to_string());
    exec_args.push(output_str);

    FfmpegCommand {
        full: display_args.join(" "),
        masked: masked_args.join(" "),
        args: exec_args,
        output_path,
    }
}

/// Parameters for two-pass video compression
pub struct CompressionParams {
    pub target_size_mb: f64,
    pub duration_sec: f64,
    /// Audio bitrate in bits per second (None if no audio)
    pub audio_bitrate_bps: Option<u64>,
}

/// Two-pass compression command
pub struct TwoPassCommand {
    pub pass1_args: Vec<String>,
    pub pass2_args: Vec<String>,
    pub output_path: PathBuf,
    pub preview_full: String,
    pub preview_masked: String,
}

struct Pass2Builder {
    exec: Vec<String>,
    display: Vec<String>,
    masked: Vec<String>,
}

impl Pass2Builder {
    fn new() -> Self {
        Self {
            exec: vec!["-y".to_string()],
            display: vec!["ffmpeg".to_string(), "-y".to_string()],
            masked: vec!["ffmpeg".to_string(), "-y".to_string()],
        }
    }

    fn push(&mut self, arg: impl Into<String>) {
        let s = arg.into();
        self.exec.push(s.clone());
        self.display.push(s.clone());
        self.masked.push(s);
    }

    fn extend(&mut self, args: impl IntoIterator<Item = String>) {
        for arg in args {
            self.push(arg);
        }
    }

    fn input(&mut self, path: &str) {
        self.exec.extend(["-i".to_string(), path.to_string()]);
        self.display
            .extend(["-i".to_string(), format!("\"{}\"", path)]);
        self.masked
            .extend(["-i".to_string(), "{input}".to_string()]);
    }

    fn output(&mut self, path: &str) {
        self.exec.push(path.to_string());
        self.display.push(format!("\"{}\"", path));
        self.masked.push("{output}".to_string());
    }

    fn filter(&mut self, filter: &str) {
        self.exec.extend(["-vf".to_string(), filter.to_string()]);
        self.display
            .extend(["-vf".to_string(), format!("\"{}\"", filter)]);
        self.masked
            .extend(["-vf".to_string(), format!("\"{}\"", filter)]);
    }
}

/// Crop rectangle `(w, h, x, y)` in source video pixels
fn crop_rect(crop: &CropSelection, dimen: &VideoDimensions) -> Option<(i32, i32, i32, i32)> {
    let scale_x = if dimen.preview_width > 0.0 {
        dimen.video_width as f32 / dimen.preview_width
    } else {
        1.0
    };

    let scale_y = if dimen.preview_height > 0.0 {
        dimen.video_height as f32 / dimen.preview_height
    } else {
        1.0
    };

    let w = (crop.width * scale_x).round() as i32;
    let h = (crop.height * scale_y).round() as i32;
    let x = (crop.x * scale_x).round() as i32;
    let y = (crop.y * scale_y).round() as i32;

    (w > 0 && h > 0).then_some((w, h, x, y))
}

/// -2 for width to ensure divisibility by 2
fn build_scale_filter(resolution: Resolution) -> String {
    format!("scale=-2:{}", resolution.height())
}

/// Upscales the shorter side to [`MIN_OUTPUT_DIMENSION`] if it's below that
fn build_min_size_filter(width: i32, height: i32) -> Option<String> {
    if width.min(height) >= MIN_OUTPUT_DIMENSION {
        None
    } else if height < width {
        Some(format!("scale=-2:{MIN_OUTPUT_DIMENSION}"))
    } else {
        Some(format!("scale={MIN_OUTPUT_DIMENSION}:-2"))
    }
}

/// Combine crop and scale filters into a single filter: crop first, then scale
fn build_video_filter(
    crop: Option<&CropSelection>,
    resolution: Option<Resolution>,
    dimen: &VideoDimensions,
) -> Option<String> {
    let crop = crop.and_then(|c| crop_rect(c, dimen));

    let scale = resolution
        .map(build_scale_filter)
        .or_else(|| crop.and_then(|(w, h, _, _)| build_min_size_filter(w, h)));

    let filters: Vec<_> = [
        crop.map(|(w, h, x, y)| format!("crop={}:{}:{}:{}", w, h, x, y)),
        scale,
    ]
    .into_iter()
    .flatten()
    .collect();

    (!filters.is_empty()).then(|| filters.join(","))
}

/// Build a two-pass compression command for a target file size.
/// Supports optional crop and trim operations integrated into the compression.
///
/// The formula for video bitrate:
/// `video_bitrate_kbps = (target_size_mb * 8192) / (1.048576 * duration) - audio_bitrate_kbps`
#[allow(clippy::too_many_arguments)]
pub fn build_compression_command(
    input_path: &Path,
    params: &CompressionParams,
    crop: Option<&CropSelection>,
    resolution: Option<Resolution>,
    trim_start: f64,
    trim_end: f64,
    dimen: &VideoDimensions,
    mute_audio: bool,
) -> Result<TwoPassCommand, CompressionError> {
    // Convert audio bitrate from bps to kbps (or 0 if no audio or muted)
    let audio_bitrate_kbps = if mute_audio {
        0.0
    } else {
        params.audio_bitrate_bps.map(|b| b / 1024).unwrap_or(0) as f64
    };

    // Use trimmed duration for bitrate calculation
    let effective_duration = trim_end - trim_start;

    // Calculate minimum size (audio-only size)
    let min_size_mb = (audio_bitrate_kbps * effective_duration) / 8192.0;

    if params.target_size_mb <= min_size_mb {
        return Err(CompressionError::TargetSizeTooSmall {
            min_size_mb: min_size_mb + 0.1,
        });
    }

    // Calculate target video bitrate
    let video_bitrate_kbps =
        (params.target_size_mb * 8192.0) / (1.048576 * effective_duration) - audio_bitrate_kbps;

    if video_bitrate_kbps <= 0.0 {
        return Err(CompressionError::InvalidTargetSize);
    }

    // Generate output filename: {original}-{size}MB.mp4
    let stem = input_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("output");

    let output_name = format!("{}-{}MB.mp4", stem, params.target_size_mb as u32);

    let output_path = input_path
        .parent()
        .map(|p| p.join(&output_name))
        .unwrap_or_else(|| output_name.clone().into());

    let input_str = input_path.to_string_lossy().to_string();
    let output_str = output_path.to_string_lossy().to_string();
    let video_bitrate_str = format!("{}k", video_bitrate_kbps as u64);

    let trim = build_trim_args(trim_start, trim_end, params.duration_sec);
    let video_filter = build_video_filter(crop, resolution, dimen);

    #[cfg(windows)]
    let null_output = "NUL";
    #[cfg(not(windows))]
    let null_output = "/dev/null";

    // Pass 1: Analysis
    let mut pass1_args = vec!["-y".to_string()];
    pass1_args.extend(trim.before_input.iter().cloned());
    pass1_args.extend(["-i".to_string(), input_str.clone()]);
    pass1_args.extend(trim.after_input.iter().cloned());

    if let Some(ref filter) = video_filter {
        pass1_args.extend(["-vf".to_string(), filter.clone()]);
    }

    pass1_args.extend([
        "-c:v".to_string(),
        "libx264".to_string(),
        "-b:v".to_string(),
        video_bitrate_str.clone(),
        "-pass".to_string(),
        "1".to_string(),
        "-an".to_string(),
        "-f".to_string(),
        "mp4".to_string(),
        null_output.to_string(),
    ]);

    // Pass 2: Actual encoding
    let audio_bitrate_str = format!("{}k", audio_bitrate_kbps as u64);
    let mut pass2 = Pass2Builder::new();

    pass2.extend(trim.before_input.iter().cloned());
    pass2.input(&input_str);
    pass2.extend(trim.after_input);

    if let Some(ref filter) = video_filter {
        pass2.filter(filter);
    }

    pass2.extend([
        "-c:v".to_string(),
        "libx264".to_string(),
        "-b:v".to_string(),
        video_bitrate_str,
        "-pass".to_string(),
        "2".to_string(),
    ]);

    if mute_audio || params.audio_bitrate_bps.is_none() {
        pass2.push("-an");
    } else {
        pass2.extend([
            "-c:a".to_string(),
            "aac".to_string(),
            "-b:a".to_string(),
            audio_bitrate_str,
        ]);
    }

    pass2.output(&output_str);

    Ok(TwoPassCommand {
        pass1_args,
        pass2_args: pass2.exec,
        output_path,
        preview_full: pass2.display.join(" "),
        preview_masked: pass2.masked.join(" "),
    })
}
