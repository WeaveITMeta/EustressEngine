// Quick script to create icon.png from icon.ico
use std::fs;

fn main() {
    // Read the ICO file
    let ico_data = fs::read("assets/icon.ico").expect("Failed to read icon.ico");
    
    // Parse ICO and extract largest PNG image
    // ICO format: header (6 bytes) + directory entries (16 bytes each) + image data
    
    // Skip header (6 bytes), read first entry
    if ico_data.len() < 22 {
        eprintln!("ICO file too small");
        return;
    }
    
    // For simplicity, just extract the first embedded PNG if it exists
    // Most modern ICO files contain PNG data directly
    
    // Try to find PNG signature (89 50 4E 47) in the file
    for i in 0..ico_data.len() - 4 {
        if ico_data[i] == 0x89 && ico_data[i+1] == 0x50 && 
           ico_data[i+2] == 0x4E && ico_data[i+3] == 0x47 {
            println!("Found PNG at offset {}", i);
            
            // Extract PNG data from this point
            let png_data = &ico_data[i..];
            
            // Write to file
            fs::write("assets/icon.png", png_data).expect("Failed to write PNG");
            println!("✅ Created assets/icon.png from ICO file");
            return;
        }
    }
    
    eprintln!("No PNG data found in ICO file");
}
