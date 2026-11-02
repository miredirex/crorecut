use std::path::Path;
use std::path::PathBuf;
use tokio::io::AsyncReadExt;
use tokio::process::Command;

#[cfg(windows)]
#[allow(unused_imports)] // Trait must be in scope for .creation_flags() method
use std::os::windows::process::CommandExt;

use super::error::{FfmpegCommandError, FrameExtractionError};
use crate::utils::format_timestamp_precise;

/// Progress info from FFmpeg encoding
#[derive(Debug, Clone, Default)]
pub struct RenderProgress {
    pub frame: u64,
    pub out_time: f64,
    pub speed: f64,
    /// Optional status text that overrides the default progress display
    pub status: Option<String>,
}

impl RenderProgress {
    pub fn with_status(status: impl Into<String>) -> Self {
        Self {
            status: Some(status.into()),
            ..Default::default()
        }
    }
}

/// Events emitted during FFmpeg rendering
#[derive(Debug, Clone)]
pub enum RenderEvent {
    Progress(RenderProgress),
    Completed,
    Failed(String),
}

/// Parser for FFmpeg's `-progress pipe:1` output format.
#[derive(Default)]
pub struct ProgressParser {
    frame: u64,
    out_time: f64,
    speed: f64,
}

impl ProgressParser {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn feed(&mut self, line: &str) -> Option<RenderProgress> {
        let line = line.trim();
        if let Some((key, value)) = line.split_once('=') {
            match key {
                "frame" => self.frame = value.parse().unwrap_or(0),
                "out_time_ms" => {
                    let micros: u64 = value.parse().unwrap_or(0);
                    self.out_time = micros as f64 / 1_000_000.0;
                }
                "speed" => {
                    self.speed = value.trim_end_matches('x').parse().unwrap_or(0.0);
                }
                "progress" => {
                    return Some(RenderProgress {
                        frame: self.frame,
                        out_time: self.out_time,
                        speed: self.speed,
                        status: None,
                    });
                }
                _ => {}
            }
        }
        None
    }
}

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

/// Video stream properties from ffprobe
#[derive(Debug, Clone)]
pub struct VideoStreamInfo {
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub duration: f64,
    pub has_audio: bool,
    pub audio_bitrate_bps: Option<u64>,
}

/// Probe video stream info (dimensions, fps, duration, audio)
pub async fn get_video_stream_info(
    video_path: &Path,
) -> Result<VideoStreamInfo, FfmpegCommandError> {
    let mut command = Command::new("ffprobe");
    command
        .args(["-v", "error"])
        .args(["-select_streams", "v:0"])
        .args([
            "-show_entries",
            "stream=width,height,r_frame_rate:format=duration",
        ])
        .args(["-of", "default=noprint_wrappers=1"])
        .arg(video_path);

    #[cfg(windows)]
    command.creation_flags(CREATE_NO_WINDOW);

    let output = command
        .output()
        .await
        .map_err(|_| FfmpegCommandError::FFmpegNotFound)?;

    if !output.status.success() {
        return Err(FfmpegCommandError::CommandFailed(
            String::from_utf8_lossy(&output.stderr).into(),
        ));
    }

    let mut width = None;
    let mut height = None;
    let mut fps = None;
    let mut duration = None;

    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        match key {
            "width" => width = value.parse().ok(),
            "height" => height = value.parse().ok(),
            "r_frame_rate" => fps = parse_frame_rate(value),
            "duration" => duration = value.parse().ok(),
            _ => {}
        }
    }

    let has_audio = has_audio_stream(video_path).await;
    let audio_bitrate_bps = if has_audio {
        get_audio_bitrate(video_path.to_path_buf())
            .await
            .ok()
            .flatten()
    } else {
        None
    };

    Ok(VideoStreamInfo {
        width: width.ok_or_else(|| FfmpegCommandError::CommandFailed("No video stream".into()))?,
        height: height.ok_or_else(|| FfmpegCommandError::CommandFailed("Missing height".into()))?,
        fps: fps.unwrap_or(30.0),
        duration: duration
            .ok_or_else(|| FfmpegCommandError::CommandFailed("Missing duration".into()))?,
        has_audio,
        audio_bitrate_bps,
    })
}

/// Extract a single frame from a video at the specified timestamp. Returns RGBA frame bytes
///
/// Both dimensions are explicit because `scale=-1:h` can round differently and shear the image.
pub async fn extract_frame(
    video_path: PathBuf,
    timestamp_seconds: f64,
    width: u32,
    height: u32,
) -> Result<Vec<u8>, FrameExtractionError> {
    if !tokio::fs::try_exists(&video_path).await.unwrap_or(false) {
        return Err(FrameExtractionError::VideoNotFound(video_path));
    }

    let timestamp_str = format_timestamp_precise(timestamp_seconds);

    // -ss before -i enables faster keyframe seeking
    let mut command = Command::new("ffmpeg");
    command
        .args(["-ss", &timestamp_str])
        .arg("-i")
        .arg(video_path)
        .args(["-vf", &format!("scale={}:{}", width, height)])
        .args(["-frames:v", "1"])
        .args(["-f", "rawvideo"])
        .args(["-pix_fmt", "rgba"])
        .arg("-")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    #[cfg(windows)]
    command.creation_flags(CREATE_NO_WINDOW);

    let mut child = command
        .spawn()
        .map_err(|_| FrameExtractionError::FFmpegNotFound)?;

    let mut stdout = child
        .stdout
        .take()
        .ok_or(FrameExtractionError::InvalidOutput)?;
    let mut frame_bytes = Vec::new();
    stdout
        .read_to_end(&mut frame_bytes)
        .await
        .map_err(|e| FrameExtractionError::ExtractionFailed(e.to_string()))?;

    let status = child
        .wait()
        .await
        .map_err(|e| FrameExtractionError::ExtractionFailed(e.to_string()))?;

    if !status.success() {
        if let Some(mut stderr) = child.stderr.take() {
            let mut error_msg = String::new();
            let _ = stderr.read_to_string(&mut error_msg).await;
            return Err(FrameExtractionError::ExtractionFailed(error_msg));
        }
        return Err(FrameExtractionError::ExtractionFailed(
            "Unknown error".to_string(),
        ));
    }

    if frame_bytes.is_empty() {
        return Err(FrameExtractionError::InvalidOutput);
    }

    Ok(frame_bytes)
}

/// Thumbnail size for batch extraction (16:9, letterbox)
pub const THUMBNAIL_WIDTH: u32 = 640;
pub const THUMBNAIL_HEIGHT: u32 = 360;
pub const THUMBNAIL_FRAME_SIZE: usize = (THUMBNAIL_WIDTH * THUMBNAIL_HEIGHT * 4) as usize;

/// Batch-extract first frames using filter_complex; preserves input order.
pub async fn extract_first_frames_batch(
    video_paths: Vec<PathBuf>,
) -> Vec<Result<Vec<u8>, FrameExtractionError>> {
    if video_paths.is_empty() {
        return Vec::new();
    }

    let mut filter_parts = Vec::new();
    let mut concat_inputs = Vec::new();

    for (i, _) in video_paths.iter().enumerate() {
        // Each input: take first frame, scale with letterboxing, normalize SAR
        filter_parts.push(format!(
            "[{i}:v]trim=end_frame=1,setpts=PTS-STARTPTS,\
             scale={w}:{h}:force_original_aspect_ratio=decrease,\
             pad={w}:{h}:(ow-iw)/2:(oh-ih)/2:black,setsar=1,format=rgba[v{i}]",
            i = i,
            w = THUMBNAIL_WIDTH,
            h = THUMBNAIL_HEIGHT
        ));
        concat_inputs.push(format!("[v{i}]"));
    }

    let n = video_paths.len();
    let filter_complex = format!(
        "{};{}concat=n={}:v=1:a=0[out]",
        filter_parts.join(";"),
        concat_inputs.join(""),
        n
    );

    let mut command = Command::new("ffmpeg");
    command.args(["-hide_banner", "-nostdin"]);

    for path in &video_paths {
        command.arg("-i").arg(path);
    }

    command
        .args(["-filter_complex", &filter_complex])
        .args(["-map", "[out]"])
        .args(["-fps_mode", "passthrough"]) // Prevent dropping frames with duplicate timestamps
        .args(["-f", "rawvideo"])
        .args(["-pix_fmt", "rgba"])
        .arg("-")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    #[cfg(windows)]
    command.creation_flags(CREATE_NO_WINDOW);

    let child = match command.spawn() {
        Ok(child) => child,
        Err(_) => {
            return video_paths
                .iter()
                .map(|_| Err(FrameExtractionError::FFmpegNotFound))
                .collect();
        }
    };

    let output = match child.wait_with_output().await {
        Ok(output) => output,
        Err(e) => {
            return video_paths
                .iter()
                .map(|_| Err(FrameExtractionError::ExtractionFailed(e.to_string())))
                .collect();
        }
    };

    if !output.status.success() {
        log::warn!(
            "Batch extraction failed, falling back to individual extraction for {} videos",
            video_paths.len()
        );
        return extract_frames_individually(video_paths).await;
    }

    let frame_bytes = output.stdout;
    let expected_bytes = n * THUMBNAIL_FRAME_SIZE;

    // If we got fewer frames than expected, we can't know which video failed.
    // Fall back to individual extraction to avoid misaligned thumbnails.
    if frame_bytes.len() != expected_bytes {
        log::warn!(
            "Batch extraction returned {} bytes, expected {}. Falling back to individual extraction.",
            frame_bytes.len(),
            expected_bytes
        );
        return extract_frames_individually(video_paths).await;
    }

    frame_bytes
        .chunks_exact(THUMBNAIL_FRAME_SIZE)
        .map(|chunk| Ok(chunk.to_vec()))
        .collect()
}

/// Fallback: extract frames individually. Used when batch extraction fails (e.g. corrupt file)
async fn extract_frames_individually(
    video_paths: Vec<PathBuf>,
) -> Vec<Result<Vec<u8>, FrameExtractionError>> {
    let futures: Vec<_> = video_paths
        .into_iter()
        .map(extract_single_thumbnail)
        .collect();
    iced::futures::future::join_all(futures).await
}

/// Extract a single thumbnail with the same parameters as batch extraction.
async fn extract_single_thumbnail(video_path: PathBuf) -> Result<Vec<u8>, FrameExtractionError> {
    let filter = format!(
        "trim=end_frame=1,setpts=PTS-STARTPTS,\
         scale={w}:{h}:force_original_aspect_ratio=decrease,\
         pad={w}:{h}:(ow-iw)/2:(oh-ih)/2:black,setsar=1,format=rgba",
        w = THUMBNAIL_WIDTH,
        h = THUMBNAIL_HEIGHT
    );

    let mut command = Command::new("ffmpeg");
    command
        .args(["-hide_banner", "-nostdin"])
        .arg("-i")
        .arg(&video_path)
        .args(["-vf", &filter])
        .args(["-frames:v", "1"])
        .args(["-f", "rawvideo"])
        .args(["-pix_fmt", "rgba"])
        .arg("-")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    #[cfg(windows)]
    command.creation_flags(CREATE_NO_WINDOW);

    let child = command
        .spawn()
        .map_err(|_| FrameExtractionError::FFmpegNotFound)?;

    let output = child
        .wait_with_output()
        .await
        .map_err(|e| FrameExtractionError::ExtractionFailed(e.to_string()))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(FrameExtractionError::ExtractionFailed(
            stderr.lines().last().unwrap_or("Unknown error").to_string(),
        ));
    }

    if output.stdout.len() != THUMBNAIL_FRAME_SIZE {
        return Err(FrameExtractionError::ExtractionFailed(format!(
            "Unexpected frame size: {} bytes (expected {})",
            output.stdout.len(),
            THUMBNAIL_FRAME_SIZE
        )));
    }

    Ok(output.stdout)
}

pub async fn get_audio_bitrate(video_path: PathBuf) -> Result<Option<u64>, FrameExtractionError> {
    if !tokio::fs::try_exists(&video_path).await.unwrap_or(false) {
        return Err(FrameExtractionError::VideoNotFound(video_path));
    }

    let mut command = Command::new("ffprobe");
    command
        .arg("-v")
        .arg("error")
        .arg("-select_streams")
        .arg("a:0")
        .arg("-show_entries")
        .arg("stream=bit_rate")
        .arg("-of")
        .arg("csv=p=0")
        .arg(&video_path);

    #[cfg(windows)]
    command.creation_flags(CREATE_NO_WINDOW);

    let output = command
        .output()
        .await
        .map_err(|_| FrameExtractionError::FFmpegNotFound)?;

    if !output.status.success() {
        // No audio stream or other error - return None
        return Ok(None);
    }

    let bitrate_str = String::from_utf8_lossy(&output.stdout);
    let trimmed = bitrate_str.trim();

    if trimmed.is_empty() || trimmed == "N/A" {
        return Ok(None);
    }

    match trimmed.parse::<u64>() {
        Ok(bitrate) => Ok(Some(bitrate)),
        Err(_) => Ok(None), // Couldn't parse - treat as no audio
    }
}

/// Check if a video file has an audio stream.
pub async fn has_audio_stream(video_path: &Path) -> bool {
    let mut command = Command::new("ffprobe");
    command
        .args(["-v", "error"])
        .args(["-select_streams", "a:0"])
        .args(["-show_entries", "stream=codec_type"])
        .args(["-of", "csv=p=0"])
        .arg(video_path);

    #[cfg(windows)]
    command.creation_flags(CREATE_NO_WINDOW);

    let Ok(output) = command.output().await else {
        return false;
    };

    output.status.success() && String::from_utf8_lossy(&output.stdout).trim() == "audio"
}

/// Reads `-progress pipe:1` output, parses it, and sends progress events.
async fn stream_ffmpeg_progress(
    mut child: tokio::process::Child,
    sender: &mut iced::futures::channel::mpsc::Sender<RenderEvent>,
) -> Result<std::process::Output, String> {
    use iced::futures::SinkExt;
    use tokio::io::{AsyncBufReadExt, BufReader};

    let stdout = child.stdout.take().ok_or("Failed to capture stdout")?;
    let mut reader = BufReader::new(stdout);
    let mut line = String::new();
    let mut parser = ProgressParser::new();

    while reader.read_line(&mut line).await.unwrap_or(0) > 0 {
        if let Some(progress) = parser.feed(&line) {
            let _ = sender.send(RenderEvent::Progress(progress)).await;
        }
        line.clear();
    }

    child.wait_with_output().await.map_err(|e| e.to_string())
}

/// Creates a stream that runs FFmpeg and yields progress events.
/// Use with `Task::stream(render_stream(...).map(...))` in iced.
pub fn run_ffmpeg_cmd(args: Vec<String>) -> iced::futures::stream::BoxStream<'static, RenderEvent> {
    Box::pin(iced::stream::channel(
        100,
        move |mut sender: iced::futures::channel::mpsc::Sender<RenderEvent>| async move {
            let run = async {
                let mut cmd = Command::new("ffmpeg");
                cmd.args(["-progress", "pipe:1", "-nostats", "-hide_banner"]);
                cmd.args(&args);

                #[cfg(windows)]
                cmd.creation_flags(CREATE_NO_WINDOW);

                cmd.stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::piped());

                let child = cmd.spawn().map_err(|_| "FFmpeg not found".to_string())?;
                let output = stream_ffmpeg_progress(child, &mut sender).await?;

                if !output.status.success() {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    return Err(stderr.to_string());
                }

                Ok(())
            };

            use iced::futures::SinkExt;
            match run.await {
                Ok(()) => {
                    let _ = sender.send(RenderEvent::Completed).await;
                }
                Err(e) => {
                    let _ = sender.send(RenderEvent::Failed(e)).await;
                }
            }
        },
    ))
}

/// Creates a stream that runs two-pass FFmpeg compression and yields progress events.
pub fn run_two_pass_ffmpeg_cmd(
    pass1_args: Vec<String>,
    pass2_args: Vec<String>,
    working_dir: PathBuf,
) -> iced::futures::stream::BoxStream<'static, RenderEvent> {
    Box::pin(iced::stream::channel(
        100,
        move |mut sender: iced::futures::channel::mpsc::Sender<RenderEvent>| async move {
            use iced::futures::SinkExt;

            let run = async {
                let _ = sender
                    .send(RenderEvent::Progress(RenderProgress::with_status(
                        "Pass 1 of 2…",
                    )))
                    .await;

                // Pass 1: Analysis pass
                let mut cmd1 = Command::new("ffmpeg");
                cmd1.arg("-hide_banner");
                cmd1.args(&pass1_args);
                cmd1.current_dir(&working_dir);

                #[cfg(windows)]
                cmd1.creation_flags(CREATE_NO_WINDOW);

                cmd1.stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::piped());

                let output1 = cmd1.output().await.map_err(|_| "FFmpeg not found")?;

                if !output1.status.success() {
                    let stderr = String::from_utf8_lossy(&output1.stderr);
                    return Err(format!("Pass 1 failed: {}", stderr));
                }

                // Pass 2: Actual encoding with progress
                let mut cmd2 = Command::new("ffmpeg");
                cmd2.args(["-progress", "pipe:1", "-nostats", "-hide_banner"]);
                cmd2.args(&pass2_args);
                cmd2.current_dir(&working_dir);

                #[cfg(windows)]
                cmd2.creation_flags(CREATE_NO_WINDOW);

                cmd2.stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::piped());

                let child = cmd2.spawn().map_err(|_| "FFmpeg not found")?;
                let output = stream_ffmpeg_progress(child, &mut sender).await?;

                let log_files = ["ffmpeg2pass-0.log", "ffmpeg2pass-0.log.mbtree"];
                for log_file in &log_files {
                    let log_path = working_dir.join(log_file);
                    let _ = tokio::fs::remove_file(log_path).await;
                }

                if !output.status.success() {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    return Err(format!("Pass 2 failed: {}", stderr));
                }

                Ok(())
            };

            match run.await {
                Ok(()) => {
                    let _ = sender.send(RenderEvent::Completed).await;
                }
                Err(e) => {
                    let _ = sender.send(RenderEvent::Failed(e)).await;
                }
            }
        },
    ))
}

fn parse_frame_rate(rate: &str) -> Option<f64> {
    let (num, den) = rate.split_once('/')?;
    let num: f64 = num.parse().ok()?;
    let den: f64 = den.parse().ok()?;
    (den != 0.0).then_some(num / den)
}
