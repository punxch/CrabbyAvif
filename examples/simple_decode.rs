use image::ImageReader;

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
        }
        Err(e) => {
            println!("Failed to decode with image crate: {}", e);
        }
    }
    
    Ok(())
}