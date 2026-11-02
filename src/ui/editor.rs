use iced::event::{self, Event};
use iced::{Point, Subscription, Task, mouse, window};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

mod selection_canvas;
mod trim_timeline;
mod view;

use selection_canvas::SelectionCanvas;
use trim_timeline::trim_timeline;

use crate::audio::AudioClock;
use crate::utils::CropSelection;
use crate::utils::ImageTransform;
use crate::video::{
    CompressionParams, RenderEvent, RenderProgress, Resolution, VideoDimensions, VideoStreamInfo,
    build_compression_command, build_ffmpeg_command, run_ffmpeg_cmd, run_two_pass_ffmpeg_cmd,
};
use iced::widget::image::{Allocation, allocate};

#[derive(Debug, Clone, Default, PartialEq)]
enum RenderStatus {
    #[default]
    Idle,
    Rendering,
    Completed(PathBuf),
    Failed(String),
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum ResizeHandle {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

#[derive(Debug, Clone, PartialEq)]
enum SelectionState {
    Idle,
    Creating {
        start_x: f32,
        start_y: f32,
    },
    Moving {
        offset_x: f32,
        offset_y: f32,
    },
    Resizing {
        handle: ResizeHandle,
        original: CropSelection,
    },
}

#[derive(Debug, Clone)]
struct CopyToast {
    position: Point,
    started_at: Instant,
}

impl CopyToast {
    const DURATION: Duration = Duration::from_millis(900);

    fn new(position: Point) -> Self {
        Self {
            position,
            started_at: Instant::now(),
        }
    }

    pub fn progress(&self) -> f32 {
        (self.started_at.elapsed().as_secs_f32() / Self::DURATION.as_secs_f32()).min(1.0)
    }

    fn is_finished(&self) -> bool {
        self.started_at.elapsed() >= Self::DURATION
    }
}

#[derive(Debug, Clone)]
pub struct Editor {
    // Video metadata (immutable after loading)
    video_path: PathBuf,
    duration: f64,
    video_width: u32,
    video_height: u32,
    video_fps: f64,
    preview_width: f32,
    preview_height: f32,
    video_file_size_mb: f64,
    has_audio_stream: bool,
    audio_bitrate_bps: Option<u64>,

    // Crop selection UI state
    selection_state: SelectionState,
    mouse_position: (f32, f32),
    canvas_bounds: (f32, f32),

    // Frame and audio streaming
    frame_allocation: Option<Allocation>,
    pending_allocation_handle: Option<iced::task::Handle>,
    timestamp: f64,
    is_streaming: bool,
    playback_ended: bool,
    seek_generation: u64,
    audio_position: f64,
    audio_clock: Arc<AudioClock>,

    // Extraction state
    is_extracting: bool,
    last_extraction_request_id: u64,
    pending_timestamp: Option<f64>,

    // Command preview and rendering
    render_status: RenderStatus,
    render_progress: Option<RenderProgress>,
    is_command_hovered: bool,
    command_preview_full: String,
    command_preview_masked: String,
    copy_toast: Option<CopyToast>,
    copy_feedback_anchor: Option<Point>,

    // Editing
    crop_selection: Option<CropSelection>,
    trim_start: f64,
    trim_end: f64,
    target_resolution: Option<Resolution>,
    target_size_mb: String,
    mute_audio: bool,
}

#[derive(Debug, Clone)]
pub enum Message {
    // Initialization
    PrepareEditor,
    VideoStreamInfoReceived(Result<VideoStreamInfo, String>),

    // Frame extraction
    FrameExtracted {
        request_id: u64,
        result: Result<Vec<u8>, String>,
    },
    FrameAllocated(Result<Allocation, iced::widget::image::Error>),
    SliderChanged(f64),

    // Crop selection
    CropMousePressed {
        x: f32,
        y: f32,
        canvas_width: f32,
        canvas_height: f32,
    },
    CropMouseMoved {
        x: f32,
        y: f32,
        canvas_width: f32,
        canvas_height: f32,
    },
    CropMouseReleased,

    // Trim selection
    TrimStartChanged(f64),
    TrimEndChanged(f64),

    // Command
    CopyCommand,
    CopyFeedbackAnchorChanged(Point),
    CommandCopied,
    ToastFrame,
    RunCommand,
    RenderProgressUpdate(RenderProgress),
    CommandCompleted(Result<PathBuf, String>),
    OpenFile(PathBuf),
    DeleteFile(PathBuf),
    OpenParentDir(PathBuf),
    CommandPreviewHovered(bool),
    ResolutionChanged(Option<Resolution>),
    TargetSizeChanged(String),
    MuteAudioToggled(bool),

    // Streaming
    StartPlayback,
    StopPlayback,
    StreamedFrame {
        frame_bytes: Vec<u8>,
        timestamp: f64,
    },

    // Audio
    AudioPositionUpdate(f64),
    AudioStreamEnded,

    // Navigation
    GoBack,
}

impl Editor {
    pub fn new(video_path: PathBuf) -> Self {
        let file_size_mb = std::fs::metadata(&video_path)
            .map(|m| m.len() as f64 / (1024.0 * 1024.0))
            .unwrap_or(0.0);

        Self {
            // Video metadata
            video_path,
            duration: 0.0,
            video_width: 0,
            video_height: 0,
            video_fps: 30.0,
            preview_width: 0.0,
            preview_height: 720.0,
            video_file_size_mb: file_size_mb,
            has_audio_stream: false,
            audio_bitrate_bps: None,
            // Crop selection UI state
            selection_state: SelectionState::Idle,
            mouse_position: (0.0, 0.0),
            canvas_bounds: (1.0, 1.0),
            // Frame and audio streaming
            frame_allocation: None,
            pending_allocation_handle: None,
            timestamp: 0.0,
            is_streaming: false,
            playback_ended: false,
            seek_generation: 0,
            audio_position: 0.0,
            audio_clock: Arc::new(AudioClock::new()),
            // Extraction state
            is_extracting: false,
            last_extraction_request_id: 0,
            pending_timestamp: None,
            // Command preview and rendering
            render_status: RenderStatus::Idle,
            render_progress: None,
            is_command_hovered: false,
            command_preview_full: String::new(),
            command_preview_masked: String::new(),
            copy_toast: None,
            copy_feedback_anchor: None,
            // Editing
            crop_selection: None,
            trim_start: 0.0,
            trim_end: 0.0,
            target_resolution: None,
            target_size_mb: String::new(),
            mute_audio: false,
        }
    }

    fn get_image_transform(&self) -> ImageTransform {
        ImageTransform::from_contain(
            self.canvas_bounds.0,
            self.canvas_bounds.1,
            self.preview_width,
            self.preview_height,
        )
    }

    fn is_crop_interaction_active(&self) -> bool {
        !matches!(self.selection_state, SelectionState::Idle)
    }

    /// Aborts any pending frame allocation and starts allocating a new frame.
    fn start_frame_allocation(&mut self, frame_bytes: Vec<u8>) -> Task<Message> {
        if let Some(handle) = self.pending_allocation_handle.take() {
            handle.abort();
        }

        let handle = iced::advanced::image::Handle::from_rgba(
            self.preview_width as u32,
            self.preview_height as u32,
            frame_bytes,
        );

        let (task, abort_handle) = allocate(handle).map(Message::FrameAllocated).abortable();
        self.pending_allocation_handle = Some(abort_handle);
        task
    }

    pub fn subscription(&self) -> Subscription<Message> {
        let media_subscription = if self.is_streaming {
            let video_sub = crate::video::frame_streaming::frame_stream_subscription(
                self.video_path.clone(),
                self.timestamp,
                (self.preview_width as u32, self.preview_height as u32),
                self.video_fps,
                self.audio_clock.clone(),
                self.has_audio_stream,
                self.seek_generation,
            )
            .map(|event| match event {
                crate::video::frame_streaming::FrameStreamEvent::FrameReady {
                    frame_bytes,
                    timestamp,
                } => Message::StreamedFrame {
                    frame_bytes,
                    timestamp,
                },
            });

            if self.has_audio_stream {
                let audio_sub = crate::audio::audio_stream_subscription(
                    self.video_path.clone(),
                    self.timestamp,
                    self.audio_clock.clone(),
                    self.seek_generation,
                )
                .map(|event| match event {
                    crate::audio::AudioStreamEvent::PositionUpdate(pos) => {
                        Message::AudioPositionUpdate(pos)
                    }
                    crate::audio::AudioStreamEvent::StreamEnded => Message::AudioStreamEnded,
                });
                Subscription::batch([video_sub, audio_sub])
            } else {
                video_sub
            }
        } else {
            Subscription::none()
        };

        let crop_subscription = if self.is_crop_interaction_active() {
            event::listen_with(|event, _status, _window| match event {
                Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                    Some(Message::CropMouseReleased)
                }
                _ => None,
            })
        } else {
            Subscription::none()
        };

        let toast_subscription = if self.copy_toast.is_some() {
            window::frames().map(|_| Message::ToastFrame)
        } else {
            Subscription::none()
        };

        Subscription::batch([media_subscription, crop_subscription, toast_subscription])
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::PrepareEditor => {
                self.regenerate_command_preview();
                Task::none()
            }

            Message::VideoStreamInfoReceived(result) => match result {
                Ok(info) => self.stream_info_received(info),
                Err(error_msg) => {
                    log::error!("Failed to get video info: {error_msg}");
                    Task::none()
                }
            },

            Message::FrameExtracted { request_id, result } => {
                self.on_frame_extracted(request_id, result)
            }

            Message::SliderChanged(new_timestamp) => self.seek_to(new_timestamp),

            Message::CropMousePressed {
                x,
                y,
                canvas_width,
                canvas_height,
            } => {
                self.handle_crop_mouse_press(x, y, canvas_width, canvas_height);
                Task::none()
            }

            Message::CropMouseMoved {
                x,
                y,
                canvas_width,
                canvas_height,
            } => {
                self.handle_crop_mouse_move(x, y, canvas_width, canvas_height);
                Task::none()
            }

            Message::CropMouseReleased => {
                if self.is_crop_interaction_active() {
                    self.selection_state = SelectionState::Idle;
                    self.regenerate_command_preview();
                }
                Task::none()
            }

            // Trimming is locked while rendering (also covers a drag that started before the render)
            Message::TrimStartChanged(_) | Message::TrimEndChanged(_)
                if matches!(self.render_status, RenderStatus::Rendering) =>
            {
                Task::none()
            }

            Message::TrimStartChanged(value) => {
                self.trim_start = value.min(self.trim_end - 0.1).max(0.0);
                self.regenerate_command_preview();
                Task::done(Message::SliderChanged(self.trim_start))
            }

            Message::TrimEndChanged(value) => {
                self.trim_end = value.max(self.trim_start + 0.1).min(self.duration);
                self.regenerate_command_preview();
                Task::done(Message::SliderChanged(self.trim_end))
            }

            Message::CopyCommand => iced::clipboard::write(self.command_preview_full.clone())
                .map(|_| Message::CommandCopied),

            Message::CopyFeedbackAnchorChanged(position) => {
                self.copy_feedback_anchor = Some(position);
                Task::none()
            }

            Message::CommandCopied => {
                let position = self.copy_feedback_anchor.unwrap_or(Point::ORIGIN);
                self.copy_toast = Some(CopyToast::new(position));
                Task::none()
            }

            Message::ToastFrame => {
                if self.copy_toast.as_ref().is_some_and(CopyToast::is_finished) {
                    self.copy_toast = None;
                }
                Task::none()
            }

            Message::RunCommand => self.run_command(),

            Message::RenderProgressUpdate(progress) => {
                self.render_progress = Some(progress);
                Task::none()
            }

            Message::CommandCompleted(result) => {
                self.render_progress = None;
                match result {
                    Ok(output_path) => {
                        log::info!("Command completed successfully");
                        self.render_status = RenderStatus::Completed(output_path);
                    }
                    Err(ref error_msg) => {
                        log::error!("Command failed: {error_msg}");
                        self.render_status = RenderStatus::Failed(error_msg.clone());
                    }
                }
                Task::none()
            }

            Message::OpenFile(path) => {
                if let Err(e) = open::that_detached(&path) {
                    log::error!("Failed to open file: {e}");
                }
                Task::none()
            }

            Message::DeleteFile(path) => {
                match std::fs::remove_file(&path) {
                    Ok(()) => {
                        log::info!("Deleted file: {}", path.display());
                        self.render_status = RenderStatus::Idle;
                    }
                    Err(e) => {
                        log::error!("Failed to delete file: {e}");
                    }
                }
                Task::none()
            }

            Message::OpenParentDir(filepath) => {
                #[cfg(windows)]
                {
                    if let Err(e) = std::process::Command::new("explorer.exe")
                        .arg("/select,")
                        .arg(&filepath)
                        .spawn()
                    {
                        log::error!("Failed to open Explorer with file selected: {e}");
                    }
                }
                #[cfg(not(windows))]
                {
                    if let Some(parent) = filepath.parent() {
                        if let Err(e) = open::that_detached(parent) {
                            log::error!("Failed to open parent directory: {e}");
                        }
                    } else {
                        log::warn!("Path has no parent directory: {}", filepath.display());
                    }
                }
                Task::none()
            }

            Message::CommandPreviewHovered(hovered) => {
                self.is_command_hovered = hovered;
                Task::none()
            }

            Message::ResolutionChanged(resolution) => {
                self.target_resolution = resolution;
                self.regenerate_command_preview();
                Task::none()
            }

            Message::TargetSizeChanged(value) => {
                self.target_size_mb = value;
                self.regenerate_command_preview();
                Task::none()
            }

            Message::MuteAudioToggled(mute) => {
                self.mute_audio = mute;
                self.regenerate_command_preview();
                Task::none()
            }

            Message::StartPlayback => {
                if !self.is_streaming {
                    if self.playback_ended {
                        self.timestamp = 0.0;
                        self.seek_generation += 1;
                        self.playback_ended = false;
                    }
                    log::info!("Starting frame streaming at timestamp {}", self.timestamp);
                    self.is_streaming = true;
                }
                Task::none()
            }

            Message::StopPlayback => {
                if self.is_streaming {
                    log::info!("Stopping frame streaming");
                    self.is_streaming = false;
                }
                Task::none()
            }

            Message::StreamedFrame {
                frame_bytes,
                timestamp,
            } => {
                if self.is_streaming {
                    self.timestamp = timestamp;
                    return self.start_frame_allocation(frame_bytes);
                }
                Task::none()
            }

            Message::FrameAllocated(result) => {
                self.pending_allocation_handle = None;
                if let Ok(allocation) = result {
                    self.frame_allocation = Some(allocation);
                }
                Task::none()
            }

            Message::AudioPositionUpdate(position) => {
                if self.is_streaming {
                    self.audio_position = position;
                }
                Task::none()
            }

            Message::AudioStreamEnded => {
                log::info!("Audio stream ended, stopping playback");
                self.is_streaming = false;
                self.playback_ended = true;
                self.timestamp = self.duration;
                Task::none()
            }

            Message::GoBack => {
                // Navigation is handled at app level
                Task::none()
            }
        }
    }

    fn copy_toast(&self) -> Option<&CopyToast> {
        self.copy_toast.as_ref()
    }

    fn stream_info_received(&mut self, info: VideoStreamInfo) -> Task<Message> {
        self.video_width = info.width;
        self.video_height = info.height;
        self.video_fps = info.fps;
        self.duration = info.duration;
        self.trim_end = info.duration;
        self.has_audio_stream = info.has_audio;
        self.audio_bitrate_bps = info.audio_bitrate_bps;

        // Calculate preview dimensions (720p with maintained aspect ratio)
        if info.height > 0 {
            self.preview_width = (info.width as f32 * 720.0) / info.height as f32;
            self.preview_height = 720.0;
        }

        log::info!(
            "Video: {}x{} @ {:.2}fps, {:.2}s, audio: {}",
            info.width,
            info.height,
            info.fps,
            info.duration,
            if info.has_audio { "yes" } else { "no" }
        );

        // Now that dimensions are set, extract initial frame
        let video_path = self.video_path.clone();
        let width = self.preview_width as u32;
        let height = self.preview_height as u32;
        Task::perform(
            async move {
                crate::video::extract_frame(video_path, 0.0, width, height)
                    .await
                    .map_err(|e| e.to_string())
            },
            |result| Message::FrameExtracted {
                request_id: 0,
                result,
            },
        )
    }

    fn on_frame_extracted(
        &mut self,
        request_id: u64,
        result: Result<Vec<u8>, String>,
    ) -> Task<Message> {
        // Only accept frame from most recent extraction
        if request_id != self.last_extraction_request_id {
            log::debug!(
                "Discarding stale frame (request_id {} != current {})",
                request_id,
                self.last_extraction_request_id
            );
            return Task::none();
        }

        let allocation_task = match result {
            Ok(frame_bytes) => self.start_frame_allocation(frame_bytes),
            Err(error_msg) => {
                log::error!("Frame extraction failed: {error_msg}");
                Task::none()
            }
        };

        self.is_extracting = false;

        if let Some(pending_ts) = self.pending_timestamp.take() {
            Task::batch([allocation_task, self.seek_to(pending_ts)])
        } else {
            allocation_task
        }
    }

    fn seek_to(&mut self, new_timestamp: f64) -> Task<Message> {
        // Update timestamp immediately for responsive UI
        self.timestamp = new_timestamp;

        // If streaming, increment seek_generation to restart subscriptions from new position
        if self.is_streaming {
            self.seek_generation += 1;
            return Task::none();
        }

        if self.is_extracting {
            // Extraction in progress - store as pending
            self.pending_timestamp = Some(new_timestamp);
            Task::none()
        } else {
            // No extraction in progress - spawn FFmpeg immediately
            self.is_extracting = true;
            self.last_extraction_request_id += 1;
            self.pending_timestamp = None;

            let video_path = self.video_path.clone();
            let request_id = self.last_extraction_request_id;
            let width = self.preview_width as u32;
            let height = self.preview_height as u32;

            Task::perform(
                async move {
                    crate::video::extract_frame(video_path, new_timestamp, width, height)
                        .await
                        .map_err(|e| e.to_string())
                },
                move |result| Message::FrameExtracted { request_id, result },
            )
        }
    }

    fn handle_crop_mouse_press(
        &mut self,
        canvas_x: f32,
        canvas_y: f32,
        canvas_width: f32,
        canvas_height: f32,
    ) {
        self.canvas_bounds = (canvas_width, canvas_height);
        self.mouse_position = (canvas_x, canvas_y);

        // Transform from canvas space to image space
        let transform = self.get_image_transform();
        let (img_x, img_y) = transform.canvas_to_image(canvas_x, canvas_y);

        // Check if clicking on an existing selection
        if let Some(selection) = self.crop_selection {
            if let Some(handle) = get_resize_handle(img_x, img_y, &selection) {
                self.selection_state = SelectionState::Resizing {
                    handle,
                    original: selection,
                };
            } else if point_in_selection(img_x, img_y, &selection) {
                self.selection_state = SelectionState::Moving {
                    offset_x: img_x - selection.x,
                    offset_y: img_y - selection.y,
                };
            } else {
                self.crop_selection = None;
                self.selection_state = SelectionState::Creating {
                    start_x: canvas_x,
                    start_y: canvas_y,
                };
            }
        } else {
            self.selection_state = SelectionState::Creating {
                start_x: canvas_x,
                start_y: canvas_y,
            };
        }
        self.regenerate_command_preview();
    }

    fn handle_crop_mouse_move(&mut self, x: f32, y: f32, canvas_width: f32, canvas_height: f32) {
        // Update canvas bounds with actual values from canvas
        self.canvas_bounds = (canvas_width, canvas_height);

        // Transform from canvas space to image space
        let transform = self.get_image_transform();
        let (img_x, img_y) = transform.canvas_to_image(x, y);

        // Store mouse position in canvas space
        self.mouse_position = (x, y);

        match self.selection_state {
            SelectionState::Creating { start_x, start_y } => {
                // Transform start position from canvas to image space
                let (start_img_x, start_img_y) = transform.canvas_to_image(start_x, start_y);

                // Create selection in image space with clamped bounds
                let min_x = start_img_x.min(img_x).clamp(0.0, self.preview_width);
                let min_y = start_img_y.min(img_y).clamp(0.0, self.preview_height);
                let max_x = start_img_x.max(img_x).clamp(0.0, self.preview_width);
                let max_y = start_img_y.max(img_y).clamp(0.0, self.preview_height);

                self.crop_selection = Some(CropSelection {
                    x: min_x,
                    y: min_y,
                    width: max_x - min_x,
                    height: max_y - min_y,
                });
            }
            SelectionState::Moving { offset_x, offset_y } => {
                if let Some(ref mut selection) = self.crop_selection {
                    let new_x = img_x - offset_x;
                    let new_y = img_y - offset_y;

                    // Clamp to keep selection within image bounds
                    selection.x = new_x.clamp(0.0, self.preview_width - selection.width);
                    selection.y = new_y.clamp(0.0, self.preview_height - selection.height);
                }
            }
            SelectionState::Resizing { handle, original } => {
                if let Some(ref mut selection) = self.crop_selection {
                    resize_selection(
                        selection,
                        handle,
                        img_x,
                        img_y,
                        &original,
                        self.preview_width,
                        self.preview_height,
                    );
                }
            }
            SelectionState::Idle => {}
        }
        self.regenerate_command_preview();
    }

    fn run_command(&mut self) -> Task<Message> {
        use iced::futures::StreamExt;

        self.render_status = RenderStatus::Rendering;

        let dimen = VideoDimensions {
            video_width: self.video_width,
            video_height: self.video_height,
            preview_width: self.preview_width,
            preview_height: self.preview_height,
        };

        // Check if we should do compression
        let target_size: Option<f64> = self
            .target_size_mb
            .trim()
            .parse()
            .ok()
            .filter(|&s: &f64| s > 0.0);

        if let Some(target_size_mb) = target_size {
            // Two-pass mode with compression
            let params = CompressionParams {
                target_size_mb,
                duration_sec: self.duration,
                audio_bitrate_bps: self.audio_bitrate_bps,
            };

            let cmd = match build_compression_command(
                &self.video_path,
                &params,
                self.crop_selection.as_ref(),
                self.target_resolution,
                self.trim_start,
                self.trim_end,
                &dimen,
                self.mute_audio,
            ) {
                Ok(cmd) => cmd,
                Err(e) => {
                    return Task::done(Message::CommandCompleted(Err(e.to_string())));
                }
            };

            let working_dir = self
                .video_path
                .parent()
                .map(|p| p.to_path_buf())
                .unwrap_or_else(|| PathBuf::from("."));

            let output_path = cmd.output_path.clone();
            let stream = run_two_pass_ffmpeg_cmd(cmd.pass1_args, cmd.pass2_args, working_dir);

            Task::stream(stream.map(move |event| match event {
                RenderEvent::Progress(p) => Message::RenderProgressUpdate(p),
                RenderEvent::Completed => Message::CommandCompleted(Ok(output_path.clone())),
                RenderEvent::Failed(e) => Message::CommandCompleted(Err(e)),
            }))
        } else {
            // Single-pass mode (no compression)
            let ffmpeg_cmd = build_ffmpeg_command(
                &self.video_path,
                self.crop_selection.as_ref(),
                self.target_resolution,
                self.trim_start,
                self.trim_end,
                self.duration,
                &dimen,
                self.mute_audio,
            );

            let output_path = ffmpeg_cmd.output_path.clone();
            let stream = run_ffmpeg_cmd(ffmpeg_cmd.args);

            Task::stream(stream.map(move |event| match event {
                RenderEvent::Progress(p) => Message::RenderProgressUpdate(p),
                RenderEvent::Completed => Message::CommandCompleted(Ok(output_path.clone())),
                RenderEvent::Failed(e) => Message::CommandCompleted(Err(e)),
            }))
        }
    }

    fn regenerate_command_preview(&mut self) {
        let dimen = VideoDimensions {
            video_width: self.video_width,
            video_height: self.video_height,
            preview_width: self.preview_width,
            preview_height: self.preview_height,
        };

        let target_size: Option<f64> = self
            .target_size_mb
            .trim()
            .parse()
            .ok()
            .filter(|&s: &f64| s > 0.0);

        if let Some(target_size_mb) = target_size {
            let params = CompressionParams {
                target_size_mb,
                duration_sec: self.duration,
                audio_bitrate_bps: self.audio_bitrate_bps,
            };
            match build_compression_command(
                &self.video_path,
                &params,
                self.crop_selection.as_ref(),
                self.target_resolution,
                self.trim_start,
                self.trim_end,
                &dimen,
                self.mute_audio,
            ) {
                Ok(cmd) => {
                    self.command_preview_full = cmd.preview_full;
                    self.command_preview_masked = cmd.preview_masked;
                }
                Err(e) => {
                    let err_msg = format!("Error: {}", e);
                    self.command_preview_full = err_msg.clone();
                    self.command_preview_masked = err_msg;
                }
            }
        } else {
            let cmd = build_ffmpeg_command(
                &self.video_path,
                self.crop_selection.as_ref(),
                self.target_resolution,
                self.trim_start,
                self.trim_end,
                self.duration,
                &dimen,
                self.mute_audio,
            );
            self.command_preview_full = cmd.full;
            self.command_preview_masked = cmd.masked;
        }
    }
}

fn get_resize_handle(x: f32, y: f32, selection: &CropSelection) -> Option<ResizeHandle> {
    const HANDLE_SIZE: f32 = 10.0;

    let left = selection.x;
    let right = selection.x + selection.width;
    let top = selection.y;
    let bottom = selection.y + selection.height;

    // Check corner handles
    if (x - left).abs() < HANDLE_SIZE && (y - top).abs() < HANDLE_SIZE {
        return Some(ResizeHandle::TopLeft);
    }
    if (x - right).abs() < HANDLE_SIZE && (y - top).abs() < HANDLE_SIZE {
        return Some(ResizeHandle::TopRight);
    }
    if (x - left).abs() < HANDLE_SIZE && (y - bottom).abs() < HANDLE_SIZE {
        return Some(ResizeHandle::BottomLeft);
    }
    if (x - right).abs() < HANDLE_SIZE && (y - bottom).abs() < HANDLE_SIZE {
        return Some(ResizeHandle::BottomRight);
    }

    None
}

/// Check if a point is inside the selection rectangle
fn point_in_selection(x: f32, y: f32, selection: &CropSelection) -> bool {
    x >= selection.x
        && x <= selection.x + selection.width
        && y >= selection.y
        && y <= selection.y + selection.height
}

/// Resize a selection based on the handle being dragged
fn resize_selection(
    selection: &mut CropSelection,
    handle: ResizeHandle,
    x: f32,
    y: f32,
    original: &CropSelection,
    image_width: f32,
    image_height: f32,
) {
    let image_width = image_width.max(0.0);
    let image_height = image_height.max(0.0);

    let original_left = original.x.clamp(0.0, image_width);
    let original_top = original.y.clamp(0.0, image_height);
    let original_right = (original.x + original.width).clamp(0.0, image_width);
    let original_bottom = (original.y + original.height).clamp(0.0, image_height);

    let (anchor_x, anchor_y) = match handle {
        ResizeHandle::TopLeft => (original_right, original_bottom),
        ResizeHandle::TopRight => (original_left, original_bottom),
        ResizeHandle::BottomLeft => (original_right, original_top),
        ResizeHandle::BottomRight => (original_left, original_top),
    };

    let drag_x = x.clamp(0.0, image_width);
    let drag_y = y.clamp(0.0, image_height);

    let left = anchor_x.min(drag_x);
    let right = anchor_x.max(drag_x);
    let top = anchor_y.min(drag_y);
    let bottom = anchor_y.max(drag_y);

    selection.x = left;
    selection.y = top;
    selection.width = right - left;
    selection.height = bottom - top;
}
