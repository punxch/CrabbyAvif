use crabbyavif::{decode, AvifImage};
use image::ImageReader;
use std::fs::File;
use std::io::Read;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = "bar.avif";
    
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
    
    // Decode with crabbyavif
    match decode(&buffer) {
        Ok(AvifImage { data, width, height, .. }) => {
            println!("Successfully decoded with crabbyavif:");
            println!("  Dimensions: {}x{}", width, height);
            
            // Convert to image::DynamicImage
            let img = image::RgbImage::from_raw(width, height, data)
                .ok_or("Failed to create image from raw data")?;
            let dyn_img = image::DynamicImage::ImageRgb8(img);
            
            // Save the frame
            dyn_img.save("crabbyavif_frame.png")?;
            println!("  Saved frame as crabbyavif_frame.png");
        }
        Err(e) => {
            println!("Failed to decode with crabbyavif: {}", e);
            return Err(e.into());
        }
    }
    
    Ok(())
}