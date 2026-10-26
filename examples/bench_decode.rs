// Benchmark the default decoder and RGBA8 conversion without file output.
#![allow(clippy::disallowed_methods)]

use crabby_avif::decoder::Decoder;
use crabby_avif::reformat::rgb;
use crabby_avif::utils::pixels::Pixels;
use std::collections::hash_map::DefaultHasher;
use std::error::Error;
use std::hash::{Hash, Hasher};
use std::time::Instant;

fn median(values: &mut [f64]) -> f64 {
    values.sort_by(f64::total_cmp);
    values[values.len() / 2]
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let threads: u32 = args
        .next()
        .ok_or("usage: bench_decode THREADS REPEATS FILE...")?
        .parse()?;
    let repeats: usize = args.next().ok_or("missing REPEATS")?.parse()?;
    if threads == 0 || repeats == 0 {
        return Err("THREADS and REPEATS must be positive".into());
    }
    let files: Vec<_> = args.collect();
    if files.is_empty() {
        return Err("at least one input FILE is required".into());
    }
    println!("file,threads,frames,width,height,depth,parse_ms,decode_ms,allocate_ms,rgba_ms,total_ms,first_frame_ms,checksum");
    for filename in files {
        let bytes = std::fs::read(&filename)?;
        let mut samples = Vec::new();
        let mut expected = None;
        for trial in 0..=repeats {
            let start = Instant::now();
            let mut decoder = Decoder::default();
            decoder.settings.max_threads = threads;
            decoder.set_io_vec(bytes.clone());
            decoder.parse()?;
            let parse = start.elapsed().as_secs_f64() * 1000.0;
            let mut decode = 0.0;
            let mut allocate = 0.0;
            let mut convert = 0.0;
            let mut first = 0.0;
            let mut hasher = DefaultHasher::new();
            let mut dimensions = (0, 0, 0);
            for index in 0..decoder.image_count() {
                let start = Instant::now();
                decoder.nth_image(index)?;
                decode += start.elapsed().as_secs_f64() * 1000.0;
                let image = decoder.image().ok_or("missing decoded image")?;
                dimensions = (image.width, image.height, image.depth);
                let start = Instant::now();
                let mut output = rgb::Image::create_from_yuv(image);
                output.depth = 8;
                output.format = rgb::Format::Rgba;
                output.allocate()?;
                allocate += start.elapsed().as_secs_f64() * 1000.0;
                let start = Instant::now();
                output.convert_from_yuv(image)?;
                convert += start.elapsed().as_secs_f64() * 1000.0;
                if index == 0 {
                    first = parse + decode + allocate + convert;
                }
                // Hash every output byte outside the measured stages to catch regressions.
                match output.pixels {
                    Some(Pixels::Buffer(pixels)) => pixels.hash(&mut hasher),
                    _ => return Err("expected RGBA8 output".into()),
                }
                decoder.image_timing().duration.to_bits().hash(&mut hasher);
            }
            let checksum = hasher.finish();
            if let Some(expected) = expected {
                assert_eq!(checksum, expected, "output changed between trials");
            } else {
                expected = Some(checksum);
            }
            if trial > 0 {
                samples.push([
                    parse,
                    decode,
                    allocate,
                    convert,
                    parse + decode + allocate + convert,
                    first,
                ]);
            }
            if trial == repeats {
                let medians: Vec<_> = (0..6)
                    .map(|i| median(&mut samples.iter().map(|s| s[i]).collect::<Vec<_>>()))
                    .collect();
                let csv_name = filename.replace('"', "\"\"");
                println!("\"{csv_name}\",{threads},{},{},{},{},{:.3},{:.3},{:.3},{:.3},{:.3},{:.3},{checksum:016x}",
                    decoder.image_count(), dimensions.0, dimensions.1, dimensions.2,
                    medians[0], medians[1], medians[2], medians[3], medians[4], medians[5]);
            }
        }
    }
    Ok(())
}
