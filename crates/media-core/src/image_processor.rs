//! Image Processing

use image::{DynamicImage, ImageReader};
use std::io::Cursor;
use std::path::Path;

/// Supported image formats
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaImageFormat {
    Png,
    Jpeg,
    Gif,
    WebP,
    Bmp,
    Ico,
    Tiff,
    Avif,
}

impl MediaImageFormat {
    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext.to_lowercase().as_str() {
            "png" => Some(Self::Png),
            "jpg" | "jpeg" => Some(Self::Jpeg),
            "gif" => Some(Self::Gif),
            "webp" => Some(Self::WebP),
            "bmp" => Some(Self::Bmp),
            "ico" => Some(Self::Ico),
            "tiff" | "tif" => Some(Self::Tiff),
            "avif" => Some(Self::Avif),
            _ => None,
        }
    }

    pub fn to_image_format(&self) -> image::ImageFormat {
        match self {
            Self::Png => image::ImageFormat::Png,
            Self::Jpeg => image::ImageFormat::Jpeg,
            Self::Gif => image::ImageFormat::Gif,
            Self::WebP => image::ImageFormat::WebP,
            Self::Bmp => image::ImageFormat::Bmp,
            Self::Ico => image::ImageFormat::Ico,
            Self::Tiff => image::ImageFormat::Tiff,
            Self::Avif => image::ImageFormat::Avif,
        }
    }

    pub fn mime_type(&self) -> &str {
        match self {
            Self::Png => "image/png",
            Self::Jpeg => "image/jpeg",
            Self::Gif => "image/gif",
            Self::WebP => "image/webp",
            Self::Bmp => "image/bmp",
            Self::Ico => "image/x-icon",
            Self::Tiff => "image/tiff",
            Self::Avif => "image/avif",
        }
    }
}

/// Image processing configuration
#[derive(Debug, Clone)]
pub struct ImageConfig {
    pub max_width: Option<u32>,
    pub max_height: Option<u32>,
    pub quality: u8,
    pub format: Option<MediaImageFormat>,
}

impl Default for ImageConfig {
    fn default() -> Self {
        Self {
            max_width: Some(4096),
            max_height: Some(4096),
            quality: 85,
            format: None,
        }
    }
}

/// Resize options
#[derive(Debug, Clone)]
pub struct ResizeOptions {
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub maintain_aspect_ratio: bool,
    pub filter: image::imageops::FilterType,
}

impl Default for ResizeOptions {
    fn default() -> Self {
        Self {
            width: None,
            height: None,
            maintain_aspect_ratio: true,
            filter: image::imageops::FilterType::Lanczos3,
        }
    }
}

/// Image processor
pub struct ImageProcessor {
    _config: ImageConfig,
}

impl ImageProcessor {
    pub fn new(config: ImageConfig) -> Self {
        Self { _config: config }
    }
    
    /// Load image from bytes
    pub fn load(&self, data: &[u8]) -> MediaResult<DynamicImage> {
        let reader = ImageReader::new(Cursor::new(data))
            .with_guessed_format()
            .map_err(|e| MediaError::ImageError(e.to_string()))?
            .decode()
            .map_err(|e| MediaError::ImageError(e.to_string()))?;
        Ok(reader)
    }
    
    /// Load image from file
    pub fn load_file(&self, path: &Path) -> MediaResult<DynamicImage> {
        let reader = ImageReader::open(path)
            .map_err(|e| MediaError::IoError(e.to_string()))?
            .with_guessed_format()
            .map_err(|e| MediaError::ImageError(e.to_string()))?
            .decode()
            .map_err(|e| MediaError::ImageError(e.to_string()))?;
        Ok(reader)
    }
    
    /// Resize image
    pub fn resize(&self, img: &DynamicImage, options: &ResizeOptions) -> DynamicImage {
        let (target_width, target_height) = match (options.width, options.height) {
            (Some(w), Some(h)) => (w, h),
            (Some(w), None) => {
                if options.maintain_aspect_ratio {
                    let ratio = w as f32 / img.width() as f32;
                    let h = (img.height() as f32 * ratio) as u32;
                    (w, h)
                } else {
                    (w, img.height())
                }
            }
            (None, Some(h)) => {
                if options.maintain_aspect_ratio {
                    let ratio = h as f32 / img.height() as f32;
                    let w = (img.width() as f32 * ratio) as u32;
                    (w, h)
                } else {
                    (img.width(), h)
                }
            }
            (None, None) => (img.width(), img.height()),
        };
        
        img.resize(target_width, target_height, options.filter)
    }
    
    /// Convert image to bytes
    pub fn encode(&self, img: &DynamicImage, format: MediaImageFormat) -> MediaResult<Vec<u8>> {
        let mut buffer = Cursor::new(Vec::new());
        img.write_to(&mut buffer, format.to_image_format())
            .map_err(|e| MediaError::ImageError(e.to_string()))?;
        Ok(buffer.into_inner())
    }
    
    /// Get image dimensions
    pub fn dimensions(&self, img: &DynamicImage) -> (u32, u32) {
        (img.width(), img.height())
    }
    
    /// Create thumbnail
    pub fn thumbnail(&self, img: &DynamicImage, max_size: u32) -> DynamicImage {
        img.thumbnail(max_size, max_size)
    }
    
    /// Convert to grayscale
    pub fn grayscale(&self, img: &DynamicImage) -> DynamicImage {
        img.grayscale()
    }
    
    /// Apply blur
    pub fn blur(&self, img: &DynamicImage, sigma: f32) -> DynamicImage {
        img.blur(sigma)
    }
    
    /// Sharpen image
    pub fn sharpen(&self, img: &DynamicImage) -> DynamicImage {
        img.unsharpen(1.0, 5)
    }
    
    /// Rotate image
    pub fn rotate90(&self, img: &DynamicImage) -> DynamicImage {
        img.rotate90()
    }
    
    /// Flip horizontally
    pub fn flop(&self, img: &DynamicImage) -> DynamicImage {
        img.fliph()
    }
    
    /// Flip vertically
    pub fn flip(&self, img: &DynamicImage) -> DynamicImage {
        img.flipv()
    }
    
    /// Crop image
    pub fn crop(&self, img: &DynamicImage, x: u32, y: u32, width: u32, height: u32) -> DynamicImage {
        img.crop_imm(x, y, width, height)
    }
    
    /// Get dominant colors
    pub fn dominant_colors(&self, img: &DynamicImage, count: usize) -> Vec<(u8, u8, u8)> {
        let img = img.to_rgb8();
        let mut color_counts: std::collections::HashMap<(u8, u8, u8), u32> = std::collections::HashMap::new();
        
        for pixel in img.pixels() {
            // Quantize colors to reduce unique count
            let r = (pixel[0] / 32) * 32;
            let g = (pixel[1] / 32) * 32;
            let b = (pixel[2] / 32) * 32;
            *color_counts.entry((r, g, b)).or_insert(0) += 1;
        }
        
        let mut colors: Vec<_> = color_counts.into_iter().collect();
        colors.sort_by(|a, b| b.1.cmp(&a.1));
        
        colors.into_iter()
            .take(count)
            .map(|(c, _)| c)
            .collect()
    }
}

use crate::{MediaError, MediaResult};
