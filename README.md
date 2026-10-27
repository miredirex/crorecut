<div align="center">

# <img height="40px" align="top" src="./assets/crorecut.ico">&nbsp;Crorecut

A tiny, portable, FFmpeg-based video editing tool to cut, crop and export share-ready clips

[![Download](https://img.shields.io/github/v/release/miredirex/crorecut?label=download)](https://github.com/miredirex/crorecut/releases)
[![License](https://img.shields.io/github/license/miredirex/crorecut)](LICENSE)
![Executable Size](https://img.shields.io/badge/executable%20size-~10MB-green)

</div>

<div align="center">
  <img src="screenshot.webp" alt="Screenshot" width="800">
</div>

## Features

- **Crop** - select any region of the video to keep
- **Resize** - scale output to any resolution
- **Cut** - trim videos with frame-accurate start and end points
- **Compress** - reduce file size with quality presets
- **Audio** - adjust volume levels or mute
- **Portable** - single binary with only 1 runtime dependency - [FFmpeg](https://www.ffmpeg.org/download.html)

## Download

> [!NOTE]
> Crorecut requires FFmpeg to function. The app will not work without it.

1. Download the latest release from the [releases page](https://github.com/miredirex/crorecut/releases)
1. Get FFmpeg from [ffmpeg.org](https://www.ffmpeg.org/download.html) if you don't have it
1. Configure FFmpeg in one of two ways:
   - Add FFmpeg's `bin` directory to your system `PATH`, or
   - Set the path in the app's settings

## Contributing
Found a bug or have a feature request? Open an issue or submit a pull request.

### Development with Nix (Linux)

```bash
nix develop
cargo run
```

The development shell includes Rust, FFmpeg, and the audio and graphics libraries.

## License

[MIT licensed](LICENSE)
\
\
<a href="https://github.com/iced-rs/iced"><img src="https://iced.rs/badge.svg" width="140"></a>
