//! Manimax encoder — piped ffmpeg subprocess.
//!
//! `Encoder::start` spawns an ffmpeg process consuming rawvideo RGBA on stdin
//! and writing an mp4 (h264 + yuv420p) on disk. `push_frame` streams frames;
//! `finish` closes the pipe and waits for ffmpeg. `Drop` kills the child so a
//! panic mid-render cannot orphan the subprocess.
//!
//! See `docs/porting-notes/ffmpeg.md` for the delta vs. manimgl's
//! `scene_file_writer.py`.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};

/// Args ported from manimlib/scene/scene_file_writer.py:213-230
/// @ reference/manimgl submodule commit c5e23d93.
///
/// Deviations from the manimgl command:
///   - `-vf vflip` dropped: wgpu readback is top-down, matching what ffmpeg
///     expects. If Slice B's end-to-end test shows inverted motion, re-add.
///   - `-vf eq=saturation=...:gamma=...` dropped: not relevant to Slice B.
///   - Temp-file-then-rename dance dropped: not needed for a single encoder.
///   - `video_codec`/`pixel_format` hardcoded to `libx264` / `yuv420p` rather
///     than parameterized.
fn build_ffmpeg_command(path: &Path, width: u32, height: u32, fps: u32) -> Command {
    let mut cmd = Command::new("ffmpeg");
    cmd.arg("-y")
        .args(["-f", "rawvideo"])
        .args(["-s", &format!("{width}x{height}")])
        .args(["-pix_fmt", "rgba"])
        .args(["-r", &fps.to_string()])
        .args(["-i", "-"])
        .arg("-an")
        .args(["-loglevel", "error"])
        .args(["-vcodec", "libx264"])
        .args(["-pix_fmt", "yuv420p"])
        .arg(path);
    cmd
}

#[derive(Debug, thiserror::Error)]
pub enum EncodeError {
    #[error("ffmpeg not found on PATH (spawn failed): {0}")]
    FfmpegNotFound(#[source] std::io::Error),
    #[error("failed to spawn ffmpeg: {0}")]
    SpawnFailed(#[source] std::io::Error),
    #[error("output directory {0} does not exist")]
    OutputDirMissing(PathBuf),
    #[error("frame size mismatch: expected {expected} bytes, got {got}")]
    FrameSizeMismatch { expected: usize, got: usize },
    #[error("writing to ffmpeg stdin failed (pipe probably broken): {0}")]
    WriteFailed(#[source] std::io::Error),
    #[error("ffmpeg exited with status {code:?}:\n{stderr}")]
    NonZeroExit { code: Option<i32>, stderr: String },
    #[error("ffmpeg stdin pipe was not captured")]
    StdinMissing,
    #[error("waiting for ffmpeg failed: {0}")]
    WaitFailed(#[source] std::io::Error),
}

pub struct Encoder {
    child: Child,
    stdin: Option<ChildStdin>,
    width: u32,
    height: u32,
    fps: u32,
    output: PathBuf,
}

impl Encoder {
    /// Spawn ffmpeg and prepare to accept frames.
    pub fn start(output: &Path, width: u32, height: u32, fps: u32) -> Result<Self, EncodeError> {
        assert!(
            width > 0 && height > 0 && fps > 0,
            "non-zero dims/fps required"
        );

        if let Some(parent) = output.parent() {
            if !parent.as_os_str().is_empty() && !parent.exists() {
                return Err(EncodeError::OutputDirMissing(parent.to_path_buf()));
            }
        }

        let mut cmd = build_ffmpeg_command(output, width, height, fps);
        let mut child = cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| match e.kind() {
                std::io::ErrorKind::NotFound => EncodeError::FfmpegNotFound(e),
                _ => EncodeError::SpawnFailed(e),
            })?;

        let stdin = child.stdin.take().ok_or(EncodeError::StdinMissing)?;

        Ok(Self {
            child,
            stdin: Some(stdin),
            width,
            height,
            fps,
            output: output.to_path_buf(),
        })
    }

    pub fn width(&self) -> u32 {
        self.width
    }
    pub fn height(&self) -> u32 {
        self.height
    }
    pub fn fps(&self) -> u32 {
        self.fps
    }
    pub fn output(&self) -> &Path {
        &self.output
    }

    /// Push one frame of tight `width * height * 4` sRGB RGBA bytes.
    pub fn push_frame(&mut self, rgba: &[u8]) -> Result<(), EncodeError> {
        let expected = (self.width * self.height * 4) as usize;
        if rgba.len() != expected {
            return Err(EncodeError::FrameSizeMismatch {
                expected,
                got: rgba.len(),
            });
        }
        let stdin = self.stdin.as_mut().ok_or(EncodeError::StdinMissing)?;
        stdin.write_all(rgba).map_err(EncodeError::WriteFailed)
    }

    /// Close stdin, wait for ffmpeg, and surface nonzero exit codes.
    pub fn finish(mut self) -> Result<(), EncodeError> {
        // Dropping stdin closes the pipe → ffmpeg sees EOF → flushes and exits.
        drop(self.stdin.take());

        let status = self.child.wait().map_err(EncodeError::WaitFailed)?;
        if status.success() {
            return Ok(());
        }

        let mut stderr = String::new();
        if let Some(mut err) = self.child.stderr.take() {
            let _ = err.read_to_string(&mut stderr);
        }
        Err(EncodeError::NonZeroExit {
            code: status.code(),
            stderr,
        })
    }
}

impl Drop for Encoder {
    fn drop(&mut self) {
        // Only runs on abnormal shutdown — `finish` consumes `self`.
        // Drop the pipe first so ffmpeg can wind down cleanly if it's idle,
        // then hard-kill if it's still running. Swallow errors; this is
        // already a failure path.
        drop(self.stdin.take());
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_size_mismatch_reports_bytes() {
        // Smoke-test error formatting without spawning ffmpeg.
        let err = EncodeError::FrameSizeMismatch {
            expected: 100,
            got: 99,
        };
        assert!(err.to_string().contains("100"));
        assert!(err.to_string().contains("99"));
    }
}
