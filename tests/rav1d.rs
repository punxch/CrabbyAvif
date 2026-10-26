#![cfg(feature = "rav1d")]
#![allow(clippy::disallowed_methods, clippy::disallowed_macros)]

use crabby_avif::decoder::Decoder;
use crabby_avif::image::Image;
use crabby_avif::reformat::rgb;
use crabby_avif::utils::pixels::Pixels;
use crabby_avif::{AvifError, AvifResult, CodecChoice};
use test_case::test_matrix;

fn open(filename: &str, threads: u32) -> AvifResult<Decoder> {
    let mut decoder = Decoder::default();
    decoder.settings.codec_choice = CodecChoice::Rav1d;
    decoder.settings.max_threads = threads;
    decoder.set_io_file(&format!("tests/data/{filename}"))?;
    decoder.parse()?;
    Ok(decoder)
}

fn rgba(image: &Image) -> AvifResult<Vec<u8>> {
    let mut output = rgb::Image::create_from_yuv(image);
    output.depth = 8;
    output.format = rgb::Format::Rgba;
    output.allocate()?;
    output.convert_from_yuv(image)?;
    match output.pixels {
        Some(Pixels::Buffer(pixels)) => Ok(pixels),
        _ => Err(AvifError::ReformatFailed),
    }
}

#[test_matrix([
    "colors-animated-8bpc.avif",
    "colors-animated-8bpc-alpha-exif-xmp.avif",
    "colors-animated-12bpc-keyframes-0-2-3.avif",
], [1, 2])]
fn animation_pixels_survive_seeking_and_replay(filename: &str, threads: u32) -> AvifResult<()> {
    let mut decoder = open(filename, threads)?;
    assert_eq!(decoder.image_count(), 5);
    let mut frames = Vec::new();
    let mut timings = Vec::new();
    for index in 0..decoder.image_count() {
        let timing = decoder.nth_image_timing(index)?;
        assert!(timing.duration > 0.0);
        timings.push(timing.duration);
        decoder.next_image()?;
        frames.push(rgba(decoder.image().unwrap())?);
    }
    assert!(frames.windows(2).any(|pair| pair[0] != pair[1]));
    if filename.contains("alpha") {
        assert!(frames
            .iter()
            .any(|frame| frame.chunks_exact(4).any(|pixel| pixel[3] < 255)));
    }
    assert_eq!(decoder.next_image(), Err(AvifError::NoImagesRemaining));
    // Includes backward seeks, skipped predictive frames, and two full loop restarts.
    for index in [3, 0, 4, 1, 2, 0, 1, 2, 3, 4, 0, 1, 2, 3, 4] {
        decoder.nth_image(index)?;
        assert_eq!(rgba(decoder.image().unwrap())?, frames[index as usize]);
        assert_eq!(decoder.image_timing().duration, timings[index as usize]);
    }
    Ok(())
}

#[test]
fn auto_selects_rust_backend_and_native_choice_is_unavailable() -> AvifResult<()> {
    assert!(crabby_avif::codec_versions().contains("rav1d:"));
    let mut decoder = open("white_2x2.avif", 1)?;
    decoder.settings.codec_choice = CodecChoice::Auto;
    decoder.next_image()?;
    let mut decoder = open("white_2x2.avif", 1)?;
    decoder.settings.codec_choice = CodecChoice::Dav1d;
    assert_eq!(decoder.next_image(), Err(AvifError::NoCodecAvailable));
    Ok(())
}

#[test]
fn decoder_frame_limit_is_forwarded_to_rav1d() -> AvifResult<()> {
    let mut decoder = open("white_2x2.avif", 1)?;
    // Set after parsing to test the codec limit independently of BMFF validation.
    decoder.settings.image_size_limit = std::num::NonZero::new(1);
    assert!(decoder.next_image().is_err());
    Ok(())
}

#[test]
fn invalid_av1_payload_returns_error_and_can_be_replaced() -> AvifResult<()> {
    let decoder = open("white_2x2.avif", 1)?;
    let extent = decoder.nth_image_max_extent(0)?;
    let mut bytes = std::fs::read("tests/data/white_2x2.avif").unwrap();
    let start = extent.offset as usize;
    bytes[start..start + extent.size].fill(0xff);
    let mut decoder = Decoder::default();
    decoder.settings.codec_choice = CodecChoice::Rav1d;
    decoder.set_io_vec(bytes);
    decoder.parse()?;
    assert!(decoder.next_image().is_err());
    decoder.set_io_file(&"tests/data/white_2x2.avif".into())?;
    decoder.parse()?;
    decoder.next_image()?;
    assert_eq!(rgba(decoder.image().unwrap())?.len(), 2 * 2 * 4);
    Ok(())
}
