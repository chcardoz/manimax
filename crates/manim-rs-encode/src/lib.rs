//! Manimax encoder — in-process libavcodec h264 + mp4 muxer with a worker
//! thread.
//!
//! `Encoder::start` opens an mp4 muxer and an h264 codec context, then spawns
//! a worker thread that owns the libav state. `push_frame` sends owned RGBA
//! buffers across an `mpsc::sync_channel(1)` to the worker; the worker does
//! swscale → h264 encode → mux. `finish` drops the sender, joins the worker,
//! and surfaces any error from the encode loop. This restores the implicit
//! parallelism the old `Command::spawn("ffmpeg")` subprocess gave us for
//! free, but without the 33 MB/frame stdin pipe or the multi-hundred-ms
//! subprocess teardown.
//!
//! Channel capacity is **1**: at most one frame in flight ahead of the
//! encoder. Larger queues would just buffer GBs of RGBA in RAM if the GPU is
//! ahead of the encoder, with no throughput benefit.
//!
//! Output is bit-equivalent to the previous `ffmpeg -vcodec libx264 -pix_fmt
//! yuv420p` subprocess for the same crf — both invocations call into the
//! same libx264 build that Homebrew/apt ship.
//!
//! Requires system ffmpeg dev libraries (libavcodec, libavformat, libavutil,
//! libswscale). On macOS: `brew install ffmpeg`. On Debian/Ubuntu:
//! `apt-get install libavcodec-dev libavformat-dev libavutil-dev libswscale-dev`.
//! LGPL via dynamic linking; do not enable static linking without confirming
//! the GPL implications for the broader project.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};
use std::thread::{self, JoinHandle};

use ffmpeg::Dictionary;
use ffmpeg::codec::{Context as CodecContext, Id as CodecId};
use ffmpeg::format::{self, Pixel};
use ffmpeg::frame::Video;
use ffmpeg::software::scaling::{Context as Scaler, Flags as ScalerFlags};
use ffmpeg_the_third as ffmpeg;

/// Optional tuning knobs for the encoder.
///
/// Default leaves all knobs unset, matching pre-O9 behavior (libx264 default
/// crf=23). Set `crf` to override quality: lower = higher quality, higher =
/// smaller file. Sensible range 18 (visually lossless) to 28 (preview-grade).
#[derive(Debug, Default, Clone)]
pub struct EncoderOptions {
    /// libx264 Constant Rate Factor. `None` lets libx264 use its built-in
    /// default. Values outside `0..=51` are passed through to libx264, which
    /// will reject them at codec-open time.
    pub crf: Option<u8>,
}

/// Errors surfaced by [`Encoder`] start, frame push, or finish.
#[derive(Debug, thiserror::Error)]
pub enum EncodeError {
    #[error("output directory {0} does not exist")]
    OutputDirMissing(PathBuf),
    #[error("frame size mismatch: expected {expected} bytes, got {got}")]
    FrameSizeMismatch { expected: usize, got: usize },
    #[error("ffmpeg/libav error: {0}")]
    Ffmpeg(#[from] ffmpeg::Error),
    #[error("h264 encoder unavailable in this libavcodec build")]
    EncoderUnavailable,
    #[error("encoder worker thread terminated unexpectedly")]
    WorkerGone,
    #[error("encoder worker thread panicked")]
    WorkerPanicked,
}

/// In-process h264 + mp4 encoder. Open with [`Encoder::start`] (or
/// [`Encoder::start_with_options`] for tuning), call [`push_frame`] per frame,
/// then [`finish`] to flush and write the mp4 trailer. Dropping without
/// `finish` leaves the file truncated (no trailer); the file may not be
/// playable. Always call `finish` on the success path.
pub struct Encoder {
    tx: Option<SyncSender<Vec<u8>>>,
    worker: Option<JoinHandle<Result<(), EncodeError>>>,
    width: u32,
    height: u32,
    fps: u32,
    output: PathBuf,
}

impl Encoder {
    /// Open the output mp4 with default options and prepare to accept frames.
    pub fn start(output: &Path, width: u32, height: u32, fps: u32) -> Result<Self, EncodeError> {
        Self::start_with_options(output, width, height, fps, &EncoderOptions::default())
    }

    /// Open the output mp4 with caller-supplied tuning options.
    ///
    /// All fallible setup (mux open, codec open, header write) happens
    /// synchronously here so the error surfaces before any frame work begins.
    /// Once this returns `Ok`, the worker thread owns the libav state and
    /// only further failure path is a frame-time encode error reported by
    /// `finish`.
    #[tracing::instrument(
        name = "encoder::start",
        skip_all,
        fields(width, height, fps, crf = ?options.crf),
    )]
    pub fn start_with_options(
        output: &Path,
        width: u32,
        height: u32,
        fps: u32,
        options: &EncoderOptions,
    ) -> Result<Self, EncodeError> {
        assert!(
            width > 0 && height > 0 && fps > 0,
            "non-zero dims/fps required"
        );

        if let Some(parent) = output.parent() {
            if !parent.as_os_str().is_empty() && !parent.exists() {
                return Err(EncodeError::OutputDirMissing(parent.to_path_buf()));
            }
        }

        // libav's global init — repeated calls are no-ops, so it's safe to call
        // from every Encoder construction. Cheaper than a OnceLock guard.
        ffmpeg::init()?;

        let mut octx = format::output(&output)?;
        let global_header = octx.format().flags().contains(format::Flags::GLOBAL_HEADER);

        let codec = ffmpeg::encoder::find(CodecId::H264).ok_or(EncodeError::EncoderUnavailable)?;

        let in_time_base = ffmpeg::Rational::new(1, fps as i32);

        let mut enc = CodecContext::new_with_codec(codec).encoder().video()?;
        enc.set_width(width);
        enc.set_height(height);
        enc.set_format(Pixel::YUV420P);
        enc.set_time_base(in_time_base);
        enc.set_frame_rate(Some(ffmpeg::Rational::new(fps as i32, 1)));

        // mp4 + h264 want a global header so the muxer can write the moov atom
        // without rescanning packets.
        if global_header {
            enc.set_flags(ffmpeg::codec::Flags::GLOBAL_HEADER);
        }

        let mut opts = Dictionary::new();
        if let Some(crf) = options.crf {
            opts.set("crf", &crf.to_string());
        }

        let opened = enc.open_with(opts)?;

        {
            let mut stream = octx.add_stream(codec)?;
            stream.set_time_base(in_time_base);
            stream.copy_parameters_from_context(&opened);
        }

        octx.write_header()?;

        // mp4 muxer typically rewrites time_base during `write_header`
        // (e.g. 1/30 → 1/15360), so re-read it for correct packet rescaling.
        let out_time_base = octx.stream(0).expect("stream 0 was just added").time_base();

        // `Scaler` and `Video` wrap raw libav pointers that are not `Send`,
        // so they're constructed *on* the worker thread (after `move`) rather
        // than constructed here and shipped across the spawn boundary. The
        // `octx` and `encoder` move fine because libav allows their state to
        // be touched from a single thread, just not migrated mid-operation;
        // we only ever use them on the worker thread after spawn.
        //
        // Bounded depth-1 channel: at most one frame in flight ahead of the
        // encoder. Anything deeper just buffers GBs of RGBA in RAM with no
        // throughput benefit (the encoder is the bottleneck either way).
        let (tx, rx) = sync_channel::<Vec<u8>>(1);

        let worker = thread::Builder::new()
            .name("manim-rs-encode".to_string())
            .spawn(move || -> Result<(), EncodeError> {
                let scaler = Scaler::get(
                    Pixel::RGBA,
                    width,
                    height,
                    Pixel::YUV420P,
                    width,
                    height,
                    ScalerFlags::BILINEAR,
                )?;
                let src_frame = Video::new(Pixel::RGBA, width, height);
                let s = WorkerState {
                    octx,
                    encoder: opened,
                    scaler,
                    src_frame,
                    width,
                    height,
                    in_time_base,
                    out_time_base,
                    next_pts: 0,
                };
                run_worker(s, rx)
            })
            .expect("spawning encoder worker thread");

        Ok(Self {
            tx: Some(tx),
            worker: Some(worker),
            width,
            height,
            fps,
            output: output.to_path_buf(),
        })
    }

    /// Frame width in pixels. Each [`push_frame`](Self::push_frame) must match.
    pub fn width(&self) -> u32 {
        self.width
    }
    /// Frame height in pixels. Each [`push_frame`](Self::push_frame) must match.
    pub fn height(&self) -> u32 {
        self.height
    }
    /// Frames-per-second baked into the codec context.
    pub fn fps(&self) -> u32 {
        self.fps
    }
    /// Destination mp4 path passed at [`start`](Self::start).
    pub fn output(&self) -> &Path {
        &self.output
    }

    /// Push one frame of tight `width * height * 4` sRGB RGBA bytes.
    ///
    /// Hands ownership of `rgba` to the encoder worker via a depth-1 channel.
    /// Returns immediately if there's slot available; blocks (briefly) if the
    /// worker is still encoding the previous frame. No swscale, no encode,
    /// no mux work happens on the caller's thread.
    #[tracing::instrument(name = "encoder::push_frame", skip_all, fields(bytes = rgba.len()))]
    pub fn push_frame(&mut self, rgba: Vec<u8>) -> Result<(), EncodeError> {
        let expected = (self.width * self.height * 4) as usize;
        if rgba.len() != expected {
            return Err(EncodeError::FrameSizeMismatch {
                expected,
                got: rgba.len(),
            });
        }
        let tx = self.tx.as_ref().ok_or(EncodeError::WorkerGone)?;
        // SendError means the worker dropped its receiver — i.e. it died with
        // an encode error. The actual error will be reported by `finish` when
        // the worker is joined; here we just signal the channel is dead.
        tx.send(rgba).map_err(|_| EncodeError::WorkerGone)
    }

    /// Drop the sender (signaling end-of-stream to the worker), join the
    /// worker, and surface any encode/mux error it hit. Must be called for
    /// the file to be playable; `Drop` does **not** finish the file (it
    /// can't, finalization can fail).
    #[tracing::instrument(name = "encoder::finish", skip_all)]
    pub fn finish(mut self) -> Result<(), EncodeError> {
        // Closing the channel signals end-of-stream. The worker drains
        // remaining frames, sends EOF to libav, writes the trailer, returns.
        drop(self.tx.take());
        let worker = self.worker.take().ok_or(EncodeError::WorkerGone)?;
        match worker.join() {
            Ok(result) => result,
            Err(_) => Err(EncodeError::WorkerPanicked),
        }
    }
}

impl Drop for Encoder {
    fn drop(&mut self) {
        // Only runs on abnormal shutdown — `finish` consumes `self`. Drop the
        // sender so the worker exits its receive loop, then join silently. The
        // file may be missing its mp4 trailer in this path; that's fine — it's
        // already a failure path.
        drop(self.tx.take());
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

struct WorkerState {
    octx: format::context::Output,
    encoder: ffmpeg::encoder::Video,
    scaler: Scaler,
    src_frame: Video,
    width: u32,
    height: u32,
    in_time_base: ffmpeg::Rational,
    out_time_base: ffmpeg::Rational,
    next_pts: i64,
}

/// Worker-thread main: receive RGBA buffers, swscale → encode → mux until the
/// channel closes, then flush the encoder and write the mp4 trailer. All
/// libav state lives on this thread; the public `Encoder` only owns a
/// channel sender and a join handle.
fn run_worker(mut s: WorkerState, rx: Receiver<Vec<u8>>) -> Result<(), EncodeError> {
    while let Ok(rgba) = rx.recv() {
        encode_one_frame(&mut s, &rgba)?;
    }
    flush_encoder(&mut s)?;
    s.octx.write_trailer()?;
    Ok(())
}

fn encode_one_frame(s: &mut WorkerState, rgba: &[u8]) -> Result<(), EncodeError> {
    // Copy RGBA into the source frame, respecting libav's plane stride
    // (which may be wider than `width * 4` for SIMD alignment).
    let stride = s.src_frame.stride(0);
    let row_bytes = (s.width * 4) as usize;
    let plane = s.src_frame.data_mut(0);
    if stride == row_bytes {
        plane[..rgba.len()].copy_from_slice(rgba);
    } else {
        for y in 0..s.height as usize {
            let src = &rgba[y * row_bytes..(y + 1) * row_bytes];
            let dst = &mut plane[y * stride..y * stride + row_bytes];
            dst.copy_from_slice(src);
        }
    }

    let mut yuv = Video::empty();
    s.scaler.run(&s.src_frame, &mut yuv)?;
    yuv.set_pts(Some(s.next_pts));
    s.next_pts += 1;

    s.encoder.send_frame(&yuv)?;
    drain_packets(s)
}

fn flush_encoder(s: &mut WorkerState) -> Result<(), EncodeError> {
    s.encoder.send_eof()?;
    drain_packets(s)
}

/// Pull any packets the encoder has ready and write them to the muxer.
///
/// Sets `duration = 1` (encoder time_base) on each packet — without this the
/// mp4 muxer's reported `avg_frame_rate` is off by one frame
/// (`N / (N-1)` instead of `N/1`) because PTS marks frame *starts* and the
/// muxer can't infer the last frame's display window from PTS alone.
fn drain_packets(s: &mut WorkerState) -> Result<(), EncodeError> {
    let mut packet = ffmpeg::Packet::empty();
    while s.encoder.receive_packet(&mut packet).is_ok() {
        packet.set_stream(0);
        packet.set_duration(1);
        packet.rescale_ts(s.in_time_base, s.out_time_base);
        packet.write_interleaved(&mut s.octx)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_size_mismatch_reports_bytes() {
        // Smoke-test error formatting without spinning up libav.
        let err = EncodeError::FrameSizeMismatch {
            expected: 100,
            got: 99,
        };
        assert!(err.to_string().contains("100"));
        assert!(err.to_string().contains("99"));
    }
}
