//! Slice B step 6 integration test — encode 30 solid-color frames to mp4 and
//! verify the output with ffprobe.

use std::path::PathBuf;
use std::process::Command;

use manim_rs_encode::Encoder;

const WIDTH: u32 = 480;
const HEIGHT: u32 = 270;
const FPS: u32 = 30;
const FRAMES: u32 = 30;

fn output_path() -> PathBuf {
    let mut p = std::env::temp_dir();
    p.push("manim_rs_encode_solid.mp4");
    p
}

#[test]
fn encode_30_solid_frames() {
    let path = output_path();
    let _ = std::fs::remove_file(&path);

    let mut enc = Encoder::start(&path, WIDTH, HEIGHT, FPS).expect("start ffmpeg");

    // Recognizable solid color; repeat one RGBA pixel across the frame.
    let frame: Vec<u8> = [0x40u8, 0x80, 0xc0, 0xff]
        .into_iter()
        .cycle()
        .take((WIDTH * HEIGHT * 4) as usize)
        .collect();

    for _ in 0..FRAMES {
        enc.push_frame(&frame).expect("push_frame");
    }
    enc.finish().expect("finish ffmpeg");

    assert!(path.exists(), "mp4 not written");

    let probe = Command::new("ffprobe")
        .args(["-v", "error"])
        .args(["-select_streams", "v:0"])
        .args([
            "-show_entries",
            "stream=width,height,avg_frame_rate,codec_name,nb_read_frames",
        ])
        .args(["-count_frames", "-of", "default=noprint_wrappers=1"])
        .arg(&path)
        .output()
        .expect("run ffprobe");
    assert!(probe.status.success(), "ffprobe failed: {:?}", probe);

    let out = String::from_utf8_lossy(&probe.stdout);
    assert!(out.contains(&format!("width={WIDTH}")), "{out}");
    assert!(out.contains(&format!("height={HEIGHT}")), "{out}");
    assert!(out.contains("codec_name=h264"), "{out}");
    assert!(out.contains(&format!("avg_frame_rate={FPS}/1")), "{out}");
    assert!(out.contains(&format!("nb_read_frames={FRAMES}")), "{out}");
}

#[test]
fn frame_size_mismatch_is_caught() {
    let path = output_path();
    let mut enc = Encoder::start(&path, WIDTH, HEIGHT, FPS).expect("start");
    let bad = vec![0u8; 100];
    let err = enc.push_frame(&bad).expect_err("should have rejected");
    assert!(err.to_string().contains("mismatch"));
    // Drop kills the child; test passes whether or not finish is called.
}

/// `Encoder::Drop` must reap the ffmpeg child. If it didn't, a dropped
/// encoder would leave the output file locked (on some platforms) and the
/// subprocess zombied. We prove it by dropping mid-encode and immediately
/// spawning a fresh encoder to the *same* path — the second encoder must
/// start and finish cleanly.
#[test]
fn dropped_encoder_releases_resources() {
    let mut p = std::env::temp_dir();
    p.push("manim_rs_encode_drop_zombie.mp4");
    let _ = std::fs::remove_file(&p);

    {
        let mut enc = Encoder::start(&p, WIDTH, HEIGHT, FPS).expect("start #1");
        let frame: Vec<u8> = [0xffu8, 0, 0, 0xff]
            .into_iter()
            .cycle()
            .take((WIDTH * HEIGHT * 4) as usize)
            .collect();
        enc.push_frame(&frame).expect("push #1");
        // Drop without calling finish().
    }

    // Immediately reuse the path. If Drop orphaned the first ffmpeg, this
    // race against the lingering writer — or a leftover partial file — is
    // exactly what would surface the bug.
    let mut enc2 = Encoder::start(&p, WIDTH, HEIGHT, FPS).expect("start #2");
    let frame: Vec<u8> = [0u8, 0xff, 0, 0xff]
        .into_iter()
        .cycle()
        .take((WIDTH * HEIGHT * 4) as usize)
        .collect();
    for _ in 0..3 {
        enc2.push_frame(&frame).expect("push #2");
    }
    enc2.finish().expect("finish #2");
    assert!(p.exists(), "mp4 not written after reuse");
}

/// Pixel color survives the rawvideo → libx264/yuv420p → decode roundtrip.
/// Libx264 with yuv420p subsamples chroma 2×2 and crushes sRGB to a limited
/// range, so we budget ±6/255 per channel — tight enough to catch a swapped
/// R/B pair, a stripped alpha, or an accidental grayscale path.
#[test]
fn solid_color_survives_yuv420p_roundtrip() {
    let mut p = std::env::temp_dir();
    p.push("manim_rs_encode_color_roundtrip.mp4");
    let _ = std::fs::remove_file(&p);

    let mut enc = Encoder::start(&p, WIDTH, HEIGHT, FPS).expect("start");
    // Same signature color used in encode_30_solid_frames.
    let rgba: [u8; 4] = [0x40, 0x80, 0xC0, 0xFF];
    let frame: Vec<u8> = rgba
        .into_iter()
        .cycle()
        .take((WIDTH * HEIGHT * 4) as usize)
        .collect();
    for _ in 0..5 {
        enc.push_frame(&frame).expect("push");
    }
    enc.finish().expect("finish");

    // Decode the middle frame back to raw RGBA so we can inspect bytes.
    let decoded = Command::new("ffmpeg")
        .args(["-v", "error"])
        .args(["-i"])
        .arg(&p)
        .args(["-vf", "select=eq(n\\,2)"])
        .args(["-vframes", "1"])
        .args(["-f", "rawvideo", "-pix_fmt", "rgba"])
        .arg("-")
        .output()
        .expect("run ffmpeg decode");
    assert!(
        decoded.status.success(),
        "ffmpeg decode failed: {}",
        String::from_utf8_lossy(&decoded.stderr),
    );
    let out = decoded.stdout;
    assert_eq!(
        out.len(),
        (WIDTH * HEIGHT * 4) as usize,
        "decoded frame has wrong byte count",
    );

    // Sample the center pixel — well away from any boundary chroma artifacts.
    let cx = WIDTH / 2;
    let cy = HEIGHT / 2;
    let i = ((cy * WIDTH + cx) * 4) as usize;
    let got = [out[i], out[i + 1], out[i + 2], out[i + 3]];

    let tolerance = 6i32;
    for ch in 0..3 {
        let d = (got[ch] as i32 - rgba[ch] as i32).abs();
        assert!(
            d <= tolerance,
            "channel {ch} drifted: got {got:?}, expected {rgba:?} (d={d} > {tolerance})",
        );
    }
    // yuv420p has no alpha channel — ffmpeg reconstructs it as 0xFF. Pin it
    // so a future pixel-format change doesn't silently degrade this test.
    assert_eq!(got[3], 0xFF, "alpha channel not opaque after roundtrip");
}
