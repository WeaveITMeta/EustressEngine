//! # Image Utilities for Generate (VIGA)
//!
//! Production-quality image encoding, decoding, and comparison utilities.
//! Implements proper SSIM, perceptual hashing, and histogram comparison.

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use image::{DynamicImage, GenericImageView, ImageBuffer, Luma, Rgb, imageops::FilterType};
use bevy::prelude::*;

/// Standard comparison resolution (normalized for consistent results)
/// 512x512 provides good detail capture while remaining performant
const COMPARISON_SIZE: u32 = 512;

/// Image data container
#[derive(Debug, Clone)]
pub struct ImageData {
    /// Raw image bytes (PNG/JPEG format)
    pub bytes: Vec<u8>,
    /// Width in pixels
    pub width: u32,
    /// Height in pixels
    pub height: u32,
    /// MIME type (e.g., "image/png")
    pub mime_type: String,
    /// Decoded image (cached for comparison)
    decoded: Option<DynamicImage>,
}

impl ImageData {
    /// Create from raw image bytes (auto-detect format)
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self, String> {
        let img = image::load_from_memory(&bytes)
            .map_err(|e| format!("Failed to decode image: {}", e))?;
        
        let (width, height) = img.dimensions();
        
        // Detect MIME type from bytes
        let mime_type = if bytes.len() >= 8 && &bytes[0..8] == b"\x89PNG\r\n\x1a\n" {
            "image/png"
        } else if bytes.len() >= 2 && bytes[0] == 0xFF && bytes[1] == 0xD8 {
            "image/jpeg"
        } else if bytes.len() >= 4 && &bytes[0..4] == b"RIFF" {
            "image/webp"
        } else {
            "image/png" // Default
        };
        
        Ok(Self {
            bytes,
            width,
            height,
            mime_type: mime_type.to_string(),
            decoded: Some(img),
        })
    }
    
    /// Create from raw PNG bytes
    pub fn from_png(bytes: Vec<u8>) -> Result<Self, String> {
        Self::from_bytes(bytes)
    }
    
    /// Create from base64 data URL
    pub fn from_data_url(data_url: &str) -> Result<Self, String> {
        // Parse data URL format: data:image/png;base64,<data>
        let parts: Vec<&str> = data_url.splitn(2, ',').collect();
        if parts.len() != 2 {
            return Err("Invalid data URL format".to_string());
        }
        
        let data = parts[1];
        
        // Decode base64
        let bytes = BASE64.decode(data)
            .map_err(|e| format!("Base64 decode error: {}", e))?;
        
        Self::from_bytes(bytes)
    }
    
    /// Convert to base64 data URL
    pub fn to_data_url(&self) -> String {
        let encoded = BASE64.encode(&self.bytes);
        format!("data:{};base64,{}", self.mime_type, encoded)
    }
    
    /// Get base64 encoded data (without data URL prefix)
    pub fn to_base64(&self) -> String {
        BASE64.encode(&self.bytes)
    }
    
    /// Get decoded image (decode if not cached)
    pub fn get_image(&self) -> Result<DynamicImage, String> {
        if let Some(ref img) = self.decoded {
            Ok(img.clone())
        } else {
            image::load_from_memory(&self.bytes)
                .map_err(|e| format!("Failed to decode image: {}", e))
        }
    }
    
    /// Get normalized grayscale image for comparison
    pub fn get_normalized_grayscale(&self) -> Result<ImageBuffer<Luma<u8>, Vec<u8>>, String> {
        let img = self.get_image()?;
        let resized = img.resize_exact(COMPARISON_SIZE, COMPARISON_SIZE, FilterType::Lanczos3);
        Ok(resized.to_luma8())
    }
    
    /// Get normalized RGB image for comparison
    pub fn get_normalized_rgb(&self) -> Result<ImageBuffer<Rgb<u8>, Vec<u8>>, String> {
        let img = self.get_image()?;
        let resized = img.resize_exact(COMPARISON_SIZE, COMPARISON_SIZE, FilterType::Lanczos3);
        Ok(resized.to_rgb8())
    }
}

/// Encode image bytes to base64
pub fn encode_image_base64(bytes: &[u8]) -> String {
    BASE64.encode(bytes)
}

/// Decode base64 to image bytes
pub fn decode_image_base64(base64_str: &str) -> Result<Vec<u8>, String> {
    BASE64.decode(base64_str)
        .map_err(|e| format!("Base64 decode error: {}", e))
}

/// Image comparison result with multiple metrics
#[derive(Debug, Clone)]
pub struct ImageComparisonResult {
    /// Overall similarity score (0.0 = completely different, 1.0 = identical)
    /// Weighted combination of all metrics
    pub similarity: f32,
    /// Mean squared error between images (lower is better)
    pub mse: f32,
    /// Structural similarity index (SSIM) - perceptual quality metric
    pub ssim: f32,
    /// Histogram similarity (color distribution match)
    pub histogram_similarity: f32,
    /// Perceptual hash similarity (content-based)
    pub phash_similarity: f32,
    /// Edge similarity (structural match)
    pub edge_similarity: f32,
    /// Regions with significant differences
    pub difference_regions: Vec<DifferenceRegion>,
}

impl Default for ImageComparisonResult {
    fn default() -> Self {
        Self {
            similarity: 0.0,
            mse: f32::MAX,
            ssim: 0.0,
            histogram_similarity: 0.0,
            phash_similarity: 0.0,
            edge_similarity: 0.0,
            difference_regions: vec![],
        }
    }
}

/// A region where images differ significantly
#[derive(Debug, Clone)]
pub struct DifferenceRegion {
    /// Region bounds (x, y, width, height) as percentages (0.0-1.0)
    pub bounds: (f32, f32, f32, f32),
    /// Description of the difference
    pub description: String,
    /// Severity (0.0 = minor, 1.0 = major)
    pub severity: f32,
}

/// Production-quality image comparison using multiple algorithms
/// 
/// Combines:
/// - SSIM (Structural Similarity Index) for perceptual quality
/// - Histogram comparison for color distribution
/// - Perceptual hashing for content similarity
/// - Edge detection for structural matching
/// - MSE for pixel-level accuracy
pub fn compare_images(reference: &ImageData, rendered: &ImageData) -> ImageComparisonResult {
    // Handle empty images
    if reference.bytes.is_empty() || rendered.bytes.is_empty() {
        return ImageComparisonResult {
            similarity: 0.0,
            mse: f32::MAX,
            ssim: 0.0,
            histogram_similarity: 0.0,
            phash_similarity: 0.0,
            edge_similarity: 0.0,
            difference_regions: vec![DifferenceRegion {
                bounds: (0.0, 0.0, 1.0, 1.0),
                description: "One or both images are empty".to_string(),
                severity: 1.0,
            }],
        };
    }
    
    // Get normalized images for comparison
    let ref_gray = match reference.get_normalized_grayscale() {
        Ok(img) => img,
        Err(e) => {
            warn!("Failed to normalize reference image: {}", e);
            return ImageComparisonResult::default();
        }
    };
    
    let ren_gray = match rendered.get_normalized_grayscale() {
        Ok(img) => img,
        Err(e) => {
            warn!("Failed to normalize rendered image: {}", e);
            return ImageComparisonResult::default();
        }
    };
    
    let ref_rgb = match reference.get_normalized_rgb() {
        Ok(img) => img,
        Err(_) => return ImageComparisonResult::default(),
    };
    
    let ren_rgb = match rendered.get_normalized_rgb() {
        Ok(img) => img,
        Err(_) => return ImageComparisonResult::default(),
    };
    
    // Calculate all metrics
    let ssim = calculate_ssim(&ref_gray, &ren_gray);
    let mse = calculate_mse(&ref_gray, &ren_gray);
    let histogram_similarity = calculate_histogram_similarity(&ref_rgb, &ren_rgb);
    let phash_similarity = calculate_phash_similarity(&ref_gray, &ren_gray);
    let edge_similarity = calculate_edge_similarity(&ref_gray, &ren_gray);
    
    // Weighted combination for overall similarity
    // SSIM: 35% - Best for perceptual quality
    // Histogram: 20% - Color distribution
    // PHash: 20% - Content structure
    // Edge: 15% - Structural details
    // MSE-based: 10% - Pixel accuracy
    let mse_similarity = 1.0 - (mse / 65025.0).min(1.0);
    
    let similarity = 
        ssim * 0.35 +
        histogram_similarity * 0.20 +
        phash_similarity * 0.20 +
        edge_similarity * 0.15 +
        mse_similarity * 0.10;
    
    // Identify difference regions
    let difference_regions = identify_difference_regions(&ref_gray, &ren_gray, similarity);
    
    info!(
        "Image comparison: SSIM={:.3}, Hist={:.3}, PHash={:.3}, Edge={:.3}, MSE={:.1} → Overall={:.3}",
        ssim, histogram_similarity, phash_similarity, edge_similarity, mse, similarity
    );
    
    ImageComparisonResult {
        similarity,
        mse,
        ssim,
        histogram_similarity,
        phash_similarity,
        edge_similarity,
        difference_regions,
    }
}

/// Calculate Structural Similarity Index (SSIM)
/// Based on luminance, contrast, and structure comparison
fn calculate_ssim(img1: &ImageBuffer<Luma<u8>, Vec<u8>>, img2: &ImageBuffer<Luma<u8>, Vec<u8>>) -> f32 {
    let (width, height) = img1.dimensions();
    let n = (width * height) as f64;
    
    if n == 0.0 {
        return 0.0;
    }
    
    // Calculate means
    let mut sum1: f64 = 0.0;
    let mut sum2: f64 = 0.0;
    
    for (p1, p2) in img1.pixels().zip(img2.pixels()) {
        sum1 += p1.0[0] as f64;
        sum2 += p2.0[0] as f64;
    }
    
    let mean1 = sum1 / n;
    let mean2 = sum2 / n;
    
    // Calculate variances and covariance
    let mut var1: f64 = 0.0;
    let mut var2: f64 = 0.0;
    let mut covar: f64 = 0.0;
    
    for (p1, p2) in img1.pixels().zip(img2.pixels()) {
        let v1 = p1.0[0] as f64 - mean1;
        let v2 = p2.0[0] as f64 - mean2;
        var1 += v1 * v1;
        var2 += v2 * v2;
        covar += v1 * v2;
    }
    
    var1 /= n;
    var2 /= n;
    covar /= n;
    
    // SSIM constants (for 8-bit images)
    let c1 = 6.5025; // (0.01 * 255)^2
    let c2 = 58.5225; // (0.03 * 255)^2
    
    // SSIM formula
    let numerator = (2.0 * mean1 * mean2 + c1) * (2.0 * covar + c2);
    let denominator = (mean1 * mean1 + mean2 * mean2 + c1) * (var1 + var2 + c2);
    
    (numerator / denominator).clamp(0.0, 1.0) as f32
}

/// Calculate Mean Squared Error
fn calculate_mse(img1: &ImageBuffer<Luma<u8>, Vec<u8>>, img2: &ImageBuffer<Luma<u8>, Vec<u8>>) -> f32 {
    let mut sum: f64 = 0.0;
    let mut count: u64 = 0;
    
    for (p1, p2) in img1.pixels().zip(img2.pixels()) {
        let diff = p1.0[0] as f64 - p2.0[0] as f64;
        sum += diff * diff;
        count += 1;
    }
    
    if count == 0 {
        return f32::MAX;
    }
    
    (sum / count as f64) as f32
}

/// Calculate histogram similarity using Bhattacharyya coefficient
fn calculate_histogram_similarity(
    img1: &ImageBuffer<Rgb<u8>, Vec<u8>>,
    img2: &ImageBuffer<Rgb<u8>, Vec<u8>>,
) -> f32 {
    const BINS: usize = 32;
    const BIN_SIZE: usize = 256 / BINS;
    
    // Calculate histograms for each channel
    let mut hist1_r = [0u32; BINS];
    let mut hist1_g = [0u32; BINS];
    let mut hist1_b = [0u32; BINS];
    let mut hist2_r = [0u32; BINS];
    let mut hist2_g = [0u32; BINS];
    let mut hist2_b = [0u32; BINS];
    
    for pixel in img1.pixels() {
        hist1_r[(pixel.0[0] as usize / BIN_SIZE).min(BINS - 1)] += 1;
        hist1_g[(pixel.0[1] as usize / BIN_SIZE).min(BINS - 1)] += 1;
        hist1_b[(pixel.0[2] as usize / BIN_SIZE).min(BINS - 1)] += 1;
    }
    
    for pixel in img2.pixels() {
        hist2_r[(pixel.0[0] as usize / BIN_SIZE).min(BINS - 1)] += 1;
        hist2_g[(pixel.0[1] as usize / BIN_SIZE).min(BINS - 1)] += 1;
        hist2_b[(pixel.0[2] as usize / BIN_SIZE).min(BINS - 1)] += 1;
    }
    
    // Normalize and calculate Bhattacharyya coefficient
    let n1 = img1.pixels().count() as f64;
    let n2 = img2.pixels().count() as f64;
    
    let mut bc_r = 0.0f64;
    let mut bc_g = 0.0f64;
    let mut bc_b = 0.0f64;
    
    for i in 0..BINS {
        let p1_r = hist1_r[i] as f64 / n1;
        let p2_r = hist2_r[i] as f64 / n2;
        bc_r += (p1_r * p2_r).sqrt();
        
        let p1_g = hist1_g[i] as f64 / n1;
        let p2_g = hist2_g[i] as f64 / n2;
        bc_g += (p1_g * p2_g).sqrt();
        
        let p1_b = hist1_b[i] as f64 / n1;
        let p2_b = hist2_b[i] as f64 / n2;
        bc_b += (p1_b * p2_b).sqrt();
    }
    
    // Average across channels
    ((bc_r + bc_g + bc_b) / 3.0).clamp(0.0, 1.0) as f32
}

/// Calculate perceptual hash similarity using difference hash (dHash)
fn calculate_phash_similarity(
    img1: &ImageBuffer<Luma<u8>, Vec<u8>>,
    img2: &ImageBuffer<Luma<u8>, Vec<u8>>,
) -> f32 {
    const HASH_SIZE: u32 = 16;
    
    // Resize to hash size + 1 for gradient calculation
    let resized1 = image::imageops::resize(img1, HASH_SIZE + 1, HASH_SIZE, FilterType::Nearest);
    let resized2 = image::imageops::resize(img2, HASH_SIZE + 1, HASH_SIZE, FilterType::Nearest);
    
    // Calculate difference hash (compare adjacent pixels)
    let mut hash1 = Vec::with_capacity((HASH_SIZE * HASH_SIZE) as usize);
    let mut hash2 = Vec::with_capacity((HASH_SIZE * HASH_SIZE) as usize);
    
    for y in 0..HASH_SIZE {
        for x in 0..HASH_SIZE {
            let left1 = resized1.get_pixel(x, y).0[0];
            let right1 = resized1.get_pixel(x + 1, y).0[0];
            hash1.push(left1 > right1);
            
            let left2 = resized2.get_pixel(x, y).0[0];
            let right2 = resized2.get_pixel(x + 1, y).0[0];
            hash2.push(left2 > right2);
        }
    }
    
    // Calculate Hamming distance
    let mut matching = 0;
    for (b1, b2) in hash1.iter().zip(hash2.iter()) {
        if b1 == b2 {
            matching += 1;
        }
    }
    
    matching as f32 / hash1.len() as f32
}

/// Calculate edge similarity using Sobel operator
fn calculate_edge_similarity(
    img1: &ImageBuffer<Luma<u8>, Vec<u8>>,
    img2: &ImageBuffer<Luma<u8>, Vec<u8>>,
) -> f32 {
    let edges1 = detect_edges(img1);
    let edges2 = detect_edges(img2);
    
    // Compare edge maps
    let mut matching: u64 = 0;
    let mut total: u64 = 0;
    
    for (p1, p2) in edges1.pixels().zip(edges2.pixels()) {
        let e1 = p1.0[0] > 30; // Edge threshold
        let e2 = p2.0[0] > 30;
        
        if e1 == e2 {
            matching += 1;
        }
        total += 1;
    }
    
    if total == 0 {
        return 0.0;
    }
    
    matching as f32 / total as f32
}

/// Simple Sobel edge detection
fn detect_edges(img: &ImageBuffer<Luma<u8>, Vec<u8>>) -> ImageBuffer<Luma<u8>, Vec<u8>> {
    let (width, height) = img.dimensions();
    let mut edges = ImageBuffer::new(width, height);
    
    // Sobel kernels
    let gx: [[i32; 3]; 3] = [[-1, 0, 1], [-2, 0, 2], [-1, 0, 1]];
    let gy: [[i32; 3]; 3] = [[-1, -2, -1], [0, 0, 0], [1, 2, 1]];
    
    for y in 1..height - 1 {
        for x in 1..width - 1 {
            let mut sum_x: i32 = 0;
            let mut sum_y: i32 = 0;
            
            for ky in 0..3 {
                for kx in 0..3 {
                    let px = img.get_pixel(x + kx - 1, y + ky - 1).0[0] as i32;
                    sum_x += px * gx[ky as usize][kx as usize];
                    sum_y += px * gy[ky as usize][kx as usize];
                }
            }
            
            let magnitude = ((sum_x * sum_x + sum_y * sum_y) as f64).sqrt() as u8;
            edges.put_pixel(x, y, Luma([magnitude]));
        }
    }
    
    edges
}

/// Identify regions with significant differences
fn identify_difference_regions(
    img1: &ImageBuffer<Luma<u8>, Vec<u8>>,
    img2: &ImageBuffer<Luma<u8>, Vec<u8>>,
    overall_similarity: f32,
) -> Vec<DifferenceRegion> {
    let mut regions = Vec::new();
    
    // Divide into grid and find high-difference areas
    const GRID_SIZE: u32 = 4;
    let (width, height) = img1.dimensions();
    let cell_w = width / GRID_SIZE;
    let cell_h = height / GRID_SIZE;
    
    for gy in 0..GRID_SIZE {
        for gx in 0..GRID_SIZE {
            let mut diff_sum: u64 = 0;
            let mut count: u64 = 0;
            
            for y in (gy * cell_h)..((gy + 1) * cell_h).min(height) {
                for x in (gx * cell_w)..((gx + 1) * cell_w).min(width) {
                    let p1 = img1.get_pixel(x, y).0[0] as i32;
                    let p2 = img2.get_pixel(x, y).0[0] as i32;
                    diff_sum += (p1 - p2).unsigned_abs() as u64;
                    count += 1;
                }
            }
            
            if count > 0 {
                let avg_diff = diff_sum as f32 / count as f32 / 255.0;
                
                // Mark regions with significant differences
                if avg_diff > 0.15 {
                    regions.push(DifferenceRegion {
                        bounds: (
                            gx as f32 / GRID_SIZE as f32,
                            gy as f32 / GRID_SIZE as f32,
                            1.0 / GRID_SIZE as f32,
                            1.0 / GRID_SIZE as f32,
                        ),
                        description: format!(
                            "Difference in region ({}, {}): {:.1}% mismatch",
                            gx, gy, avg_diff * 100.0
                        ),
                        severity: avg_diff.min(1.0),
                    });
                }
            }
        }
    }
    
    // Add overall assessment if similarity is low
    if overall_similarity < 0.5 && regions.is_empty() {
        regions.push(DifferenceRegion {
            bounds: (0.0, 0.0, 1.0, 1.0),
            description: "Significant overall visual differences detected".to_string(),
            severity: 1.0 - overall_similarity,
        });
    }
    
    regions
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_base64_roundtrip() {
        let original = b"test image data";
        let encoded = encode_image_base64(original);
        let decoded = decode_image_base64(&encoded).unwrap();
        assert_eq!(original.to_vec(), decoded);
    }
    
    #[test]
    fn test_ssim_identical() {
        let img = ImageBuffer::from_fn(64, 64, |x, y| {
            Luma([(x + y) as u8 % 256])
        });
        let ssim = calculate_ssim(&img, &img);
        assert!((ssim - 1.0).abs() < 0.001, "SSIM of identical images should be ~1.0");
    }
    
    #[test]
    fn test_mse_identical() {
        let img = ImageBuffer::from_fn(64, 64, |x, y| {
            Luma([(x + y) as u8 % 256])
        });
        let mse = calculate_mse(&img, &img);
        assert!(mse < 0.001, "MSE of identical images should be ~0");
    }
}
