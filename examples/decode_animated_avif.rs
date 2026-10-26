use crabby_avif::decoder::Decoder;
use crabby_avif::reformat::rgb;
use crabby_avif::utils::pixels::Pixels;
use image::ImageReader;
use std::env;
use std::fs::File;
use std::io::Read;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();
    let path = if args.len() > 1 {
        &args[1]
    } else {
        "bar.avif"
    };
    
    println!("Decoding file: {}", path);
    
    // First try to decode with the image crate
    match ImageReader::open(path)?.with_guessed_format()?.decode() {
        Ok(img) => {
            println!("Successfully decoded AVIF with image crate:");
            println!("  Dimensions: {}x{}", img.width(), img.height());
            
            // Save the first frame
            img.save("first_frame.png")?;
            println!("  Saved first frame as first_frame.png");
            return Ok(());
        }
        Err(e) => {
            println!("Failed to decode with image crate: {}", e);
        }
    }
    
    // Fallback to crabbyavif
    decode_with_crabbyavif(path)?;
    
    Ok(())
}

fn decode_with_crabbyavif(path: &str) -> Result<(), Box<dyn std::error::Error>> {
    println!("Trying to decode with crabbyavif...");
    
    // Read the file
    let mut file = File::open(path)?;
    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer)?;
    
    // Create a decoder
    let mut decoder = Decoder::default();
    decoder.set_io_vec(buffer);
    
    // Parse the AVIF file
    decoder.parse()?;
    
    // Check if it's an animated AVIF
    let frame_count = decoder.image_count();
    let is_animated = frame_count > 1;
    
    if is_animated {
        println!("Detected animated AVIF with {} frames", frame_count);
        
        // Decode each frame
        for i in 0..frame_count {
            // Advance to the next frame
            decoder.nth_image(i)?;
            
            // Get the image data
            let image = decoder.image().unwrap();
            let width = image.width;
            let height = image.height;
            
            // Convert YUV to RGB using the public convert_from_yuv function
            let mut rgb_image = rgb::Image::create_from_yuv(image);
            rgb_image.format = rgb::Format::Rgb;
            rgb_image.allocate()?;
            rgb_image.convert_from_yuv(image)?;
            
            // Extract the pixel data
            let data = match rgb_image.pixels {
                Some(pixels) => match pixels {
                    Pixels::Buffer(buffer) => buffer,
                    _ => return Err("Failed to extract pixel data".into()),
                },
                None => return Err("No pixel data found".into()),
            };
            
            println!("  Frame {}: {}x{}", i, width, height);
            
            // Convert to image::DynamicImage
            let img = image::RgbImage::from_raw(width, height, data)
                .ok_or("Failed to create image from raw data")?;
            let dyn_img = image::DynamicImage::ImageRgb8(img);
            
            // Save the frame
            let filename = format!("frame_{:03}.png", i);
            dyn_img.save(&filename)?;
            println!("    Saved as {}", filename);
        }
    } else {
        println!("Detected static AVIF with 1 frame");
        
        // Decode the single frame
        decoder.nth_image(0)?;
        
        // Get the image data
        let image = decoder.image().unwrap();
        let width = image.width;
        let height = image.height;
        
        // Convert YUV to RGB using the public convert_from_yuv function
        let mut rgb_image = rgb::Image::create_from_yuv(image);
        rgb_image.format = rgb::Format::Rgb;
        rgb_image.allocate()?;
        rgb_image.convert_from_yuv(image)?;
        
        // Extract the pixel data
        let data = match rgb_image.pixels {
            Some(pixels) => match pixels {
                Pixels::Buffer(buffer) => buffer,
                _ => return Err("Failed to extract pixel data".into()),
            },
            None => return Err("No pixel data found".into()),
        };
        
        println!("  Dimensions: {}x{}", width, height);
        
        // Convert to image::DynamicImage
        let img = image::RgbImage::from_raw(width, height, data)
            .ok_or("Failed to create image from raw data")?;
        let dyn_img = image::DynamicImage::ImageRgb8(img);
        
        // Save the frame
        dyn_img.save("single_frame.png")?;
        println!("  Saved as single_frame.png");
    }
    
    Ok(())
}