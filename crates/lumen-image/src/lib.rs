//! Bounded local codecs and pinned Gemma image preprocessing. No shell/runtime types.
#![forbid(unsafe_code)]

use std::io::{Cursor, Read};
use std::path::Path;

use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader};
use sha2::{Digest, Sha256};

pub const MAX_SOURCE_BYTES: u64 = 16 * 1024 * 1024;
pub const MAX_PIXELS: u64 = 32_000_000;
pub const MAX_SIDE: u32 = 16_384;
pub const MAX_OCR_SIDE: u32 = 4096;
pub const MAX_OCR_PIXELS: u64 = 8_000_000;
pub const PREPROCESSING_VERSION: u32 = 1;
pub const PATCH_SIZE: usize = 16;
pub const MAX_PATCHES: usize = 280 * 9;
pub const PATCH_DIM: usize = PATCH_SIZE * PATCH_SIZE * 3;
pub const EXTENSIONS: &[&str] = &[
    "jpg", "jpeg", "png", "webp", "bmp", "gif", "tif", "tiff", "heic", "heif", "avif", "svg", "ico",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    Io(std::io::ErrorKind),
    Unsupported,
    TooLarge,
    Dimensions,
    Decode,
    Changed,
    Cancelled,
    Placeholder,
}
impl Error {
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Io(_) => "image:io",
            Self::Unsupported => "image:unsupported",
            Self::TooLarge => "image:source_limit",
            Self::Dimensions => "image:pixel_limit",
            Self::Decode => "image:decode",
            Self::Changed => "image:changed",
            Self::Cancelled => "image:cancelled",
            Self::Placeholder => "image:placeholder",
        }
    }
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.code())
    }
}
impl std::error::Error for Error {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Metadata {
    /// Display dimensions after EXIF orientation.
    pub width: u32,
    pub height: u32,
    pub orientation: u8,
    pub format: &'static str,
    pub digest: [u8; 32],
}

pub struct Decoded {
    pub metadata: Metadata,
    pub rgb: Vec<u8>,
}

fn check_cancel(cancelled: &dyn Fn() -> bool) -> Result<(), Error> {
    if cancelled() {
        Err(Error::Cancelled)
    } else {
        Ok(())
    }
}

fn checked_dimensions(width: u32, height: u32) -> Result<(), Error> {
    if width == 0
        || height == 0
        || width > MAX_SIDE
        || height > MAX_SIDE
        || u64::from(width) * u64::from(height) > MAX_PIXELS
    {
        return Err(Error::Dimensions);
    }
    Ok(())
}

fn read(path: &Path, cancelled: &dyn Fn() -> bool) -> Result<Vec<u8>, Error> {
    check_cancel(cancelled)?;
    let before = std::fs::metadata(path).map_err(|e| Error::Io(e.kind()))?;
    if !before.is_file() || before.len() > MAX_SOURCE_BYTES {
        return Err(Error::TooLarge);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if before.file_attributes() & (0x1000 | 0x40000 | 0x400000) != 0 {
            return Err(Error::Placeholder);
        }
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(|e| Error::Io(e.kind()))?
        .take(MAX_SOURCE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| Error::Io(e.kind()))?;
    if bytes.len() as u64 > MAX_SOURCE_BYTES {
        return Err(Error::TooLarge);
    }
    let after = std::fs::metadata(path).map_err(|e| Error::Io(e.kind()))?;
    if before.len() != bytes.len() as u64
        || before.len() != after.len()
        || before.modified().ok() != after.modified().ok()
        || before.created().ok() != after.created().ok()
    {
        return Err(Error::Changed);
    }
    check_cancel(cancelled)?;
    Ok(bytes)
}

fn decoder(bytes: &[u8]) -> Result<(impl ImageDecoder + '_, Metadata), Error> {
    let format = image::guess_format(bytes).map_err(|_| Error::Unsupported)?;
    let name = match format {
        ImageFormat::Png => "PNG",
        ImageFormat::Jpeg => "JPEG",
        ImageFormat::WebP => "WebP",
        ImageFormat::Bmp => "BMP",
        _ => return Err(Error::Unsupported),
    };
    let mut reader = ImageReader::with_format(Cursor::new(bytes), format);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_SIDE);
    limits.max_image_height = Some(MAX_SIDE);
    limits.max_alloc = Some(192 * 1024 * 1024);
    reader.limits(limits);
    let mut decoder = reader.into_decoder().map_err(|_| Error::Decode)?;
    let (width, height) = decoder.dimensions();
    checked_dimensions(width, height)?;
    let orientation = decoder.orientation().map_err(|_| Error::Decode)?.to_exif();
    let (width, height) = if orientation >= 5 {
        (height, width)
    } else {
        (width, height)
    };
    Ok((
        decoder,
        Metadata {
            width,
            height,
            orientation,
            format: name,
            digest: Sha256::digest(bytes).into(),
        },
    ))
}

/// Header/EXIF orientation and bounded source digest, without decoding all pixels.
/// # Errors
/// Typed resource/format/source errors; no content or path in error messages.
pub fn inspect(path: &Path, cancelled: &dyn Fn() -> bool) -> Result<Metadata, Error> {
    let bytes = read(path, cancelled)?;
    let (_, metadata) = decoder(&bytes)?;
    check_cancel(cancelled)?;
    Ok(metadata)
}

/// Decode one oriented RGB image; expected digest rejects edits since metadata indexing.
/// # Errors
/// Typed resource/format/source errors. Codec calls are synchronous, not a hard time sandbox.
pub fn decode(
    path: &Path,
    expected: Option<&[u8]>,
    cancelled: &dyn Fn() -> bool,
) -> Result<Decoded, Error> {
    decode_inner(path, expected, cancelled, false)
}

/// Decode a bounded OCR image, compositing transparency over white.
/// # Errors
/// Typed resource/format/source errors, before full pixel allocation when oversized.
pub fn decode_ocr(
    path: &Path,
    expected: Option<&[u8]>,
    cancelled: &dyn Fn() -> bool,
) -> Result<Decoded, Error> {
    decode_inner(path, expected, cancelled, true)
}

fn decode_inner(
    path: &Path,
    expected: Option<&[u8]>,
    cancelled: &dyn Fn() -> bool,
    ocr: bool,
) -> Result<Decoded, Error> {
    let bytes = read(path, cancelled)?;
    let (decoder, metadata) = decoder(&bytes)?;
    if expected.is_some_and(|hash| hash != metadata.digest) {
        return Err(Error::Changed);
    }
    if ocr
        && (metadata.width > MAX_OCR_SIDE
            || metadata.height > MAX_OCR_SIDE
            || u64::from(metadata.width) * u64::from(metadata.height) > MAX_OCR_PIXELS)
    {
        return Err(Error::Dimensions);
    }
    check_cancel(cancelled)?;
    let mut image = DynamicImage::from_decoder(decoder).map_err(|_| Error::Decode)?;
    image.apply_orientation(
        image::metadata::Orientation::from_exif(metadata.orientation).ok_or(Error::Decode)?,
    );
    let rgb = if ocr && image.color().has_alpha() {
        image
            .to_rgba8()
            .pixels()
            .flat_map(|pixel| {
                let alpha = u16::from(pixel[3]);
                [pixel[0], pixel[1], pixel[2]].map(|value| {
                    ((u16::from(value) * alpha + 255 * (255 - alpha) + 127) / 255) as u8
                })
            })
            .collect()
    } else {
        image.into_rgb8().into_raw()
    };
    check_cancel(cancelled)?;
    // Re-read header identity/digest at the queue commit boundary separately if required.
    Ok(Decoded { metadata, rgb })
}

/// Pinned 280-token Gemma4 aspect policy; multiples of 3*16, including extreme ratios.
#[must_use]
pub fn target_size(width: u32, height: u32) -> Option<(u32, u32)> {
    checked_dimensions(width, height).ok()?;
    let factor = ((MAX_PATCHES * PATCH_SIZE * PATCH_SIZE) as f64
        / (f64::from(width) * f64::from(height)))
    .sqrt();
    let mult = 48.0;
    let mut h = (factor * f64::from(height) / mult).floor() as u32 * 48;
    let mut w = (factor * f64::from(width) / mult).floor() as u32 * 48;
    if h == 0 {
        h = 48;
        w = (width / height * 48).min(280 * 48);
    } else if w == 0 {
        w = 48;
        h = (height / width * 48).min(280 * 48);
    }
    Some((w, h))
}

pub struct Patches {
    pub pixels: Vec<f32>,
    pub positions: Vec<i64>,
    pub tokens: usize,
}

/// RGB bytes -> aspect-preserving bicubic resize -> padded HWC 16x16 patches/XY ids.
/// # Errors
/// Invalid shape/dimensions. Buffer sizes are bounded before allocation.
pub fn prepare(width: u32, height: u32, rgb: &[u8]) -> Result<Patches, Error> {
    let (w, h) = target_size(width, height).ok_or(Error::Dimensions)?;
    let frame = image::ImageBuffer::<image::Rgb<u8>, &[u8]>::from_raw(width, height, rgb)
        .ok_or(Error::Dimensions)?;
    if rgb.len() as u64 != u64::from(width) * u64::from(height) * 3 {
        return Err(Error::Dimensions);
    }
    let resized = image::imageops::resize(&frame, w, h, image::imageops::FilterType::CatmullRom);
    let (pw, ph) = (w as usize / PATCH_SIZE, h as usize / PATCH_SIZE);
    if pw * ph > MAX_PATCHES {
        return Err(Error::Dimensions);
    }
    let mut pixels = vec![0.0; MAX_PATCHES * PATCH_DIM];
    let mut positions = vec![-1; MAX_PATCHES * 2];
    let mut output = 0;
    for row in 0..ph {
        for col in 0..pw {
            let patch = row * pw + col;
            positions[patch * 2] = col as i64;
            positions[patch * 2 + 1] = row as i64;
            for dy in 0..PATCH_SIZE {
                for dx in 0..PATCH_SIZE {
                    let p = resized.get_pixel(
                        (col * PATCH_SIZE + dx) as u32,
                        (row * PATCH_SIZE + dy) as u32,
                    );
                    for &value in &p.0 {
                        pixels[output] = f32::from(value) / 255.0;
                        output += 1;
                    }
                }
            }
        }
    }
    Ok(Patches {
        pixels,
        positions,
        tokens: pw * ph / 9,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ocr_composites_visible_alpha_and_bounds_before_decoding() {
        let path = std::env::temp_dir().join(format!("lumen-ocr-alpha-{}.png", std::process::id()));
        let rgba = image::RgbaImage::from_fn(3, 1, |x, _| {
            image::Rgba(match x {
                0 => [0, 0, 0, 0],
                1 => [0, 0, 0, 128],
                _ => [20, 40, 60, 255],
            })
        });
        rgba.save(&path).unwrap();
        let visual = decode(&path, None, &|| false).unwrap();
        let ocr = decode_ocr(&path, Some(&visual.metadata.digest), &|| false).unwrap();
        assert_eq!(ocr.rgb, [255, 255, 255, 127, 127, 127, 20, 40, 60]);
        assert_eq!(visual.rgb, [0, 0, 0, 0, 0, 0, 20, 40, 60]);
        assert_eq!(ocr.metadata, visual.metadata);
        image::RgbImage::new(MAX_OCR_SIDE + 1, 1)
            .save(&path)
            .unwrap();
        assert!(matches!(
            decode_ocr(&path, None, &|| false),
            Err(Error::Dimensions)
        ));
        assert!(decode(&path, None, &|| false).is_ok());
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn aspect_padding_and_patch_channel_order() {
        assert_eq!(target_size(100, 100), Some((768, 768)));
        assert_eq!(target_size(16384, 1), Some((13440, 48)));
        assert_eq!(target_size(1, 16384), Some((48, 13440)));
        assert!(target_size(0, 100).is_none());
        assert!(target_size(8000, 8000).is_none());
        let rgb = [255, 0, 128].repeat(48 * 13440);
        let p = prepare(13440, 48, &rgb).unwrap();
        assert_eq!(p.tokens, 280);
        assert_eq!(&p.pixels[..3], &[1.0, 0.0, 128.0 / 255.0]);
        assert_eq!(&p.positions[..4], &[0, 0, 1, 0]);
        let p = prepare(768, 768, &[0, 255, 0].repeat(768 * 768)).unwrap();
        assert_eq!(p.tokens, 256);
        assert_eq!(p.positions[2304 * 2], -1);
        assert!(p.pixels[2304 * PATCH_DIM..].iter().all(|&p| p == 0.0));
        let frame = image::RgbImage::from_fn(768, 768, |x, y| {
            image::Rgb([(x % 256) as u8, (y % 256) as u8, 0])
        });
        let p = prepare(768, 768, frame.as_raw()).unwrap();
        assert_eq!(&p.pixels[15 * 3..15 * 3 + 3], &[15.0 / 255.0, 0.0, 0.0]);
        assert_eq!(&p.pixels[16 * 3..16 * 3 + 3], &[0.0, 1.0 / 255.0, 0.0]);
        assert_eq!(
            &p.pixels[PATCH_DIM..PATCH_DIM + 3],
            &[16.0 / 255.0, 0.0, 0.0]
        );
        assert_eq!(&p.positions[48 * 2..48 * 2 + 2], &[0, 1]);
    }
    #[test]
    fn jpeg_orientation_is_applied_before_visual_preprocessing_and_all_codecs_decode() {
        let path =
            std::env::temp_dir().join(format!("lumen-image-codecs-{}.jpg", std::process::id()));
        let frame = image::RgbImage::from_fn(4, 3, |x, y| {
            image::Rgb([(x * 60) as u8, (y * 70) as u8, 20])
        });
        let mut encoded = Cursor::new(Vec::new());
        DynamicImage::ImageRgb8(frame.clone())
            .write_to(&mut encoded, ImageFormat::Jpeg)
            .unwrap();
        let original = image::load_from_memory(encoded.get_ref())
            .unwrap()
            .to_rgb8();
        // EXIF TIFF: one orientation SHORT entry, value 6 (90 degrees clockwise).
        let exif = b"Exif\0\0II\x2a\0\x08\0\0\0\x01\0\x12\x01\x03\0\x01\0\0\0\x06\0\0\0\0\0\0\0";
        let mut oriented = encoded.get_ref()[..2].to_vec();
        oriented.extend_from_slice(&[0xff, 0xe1]);
        oriented.extend_from_slice(&u16::try_from(exif.len() + 2).unwrap().to_be_bytes());
        oriented.extend_from_slice(exif);
        oriented.extend_from_slice(&encoded.get_ref()[2..]);
        std::fs::write(&path, oriented).unwrap();
        let decoded = decode(&path, None, &|| false).unwrap();
        assert_eq!(
            (
                decoded.metadata.width,
                decoded.metadata.height,
                decoded.metadata.orientation
            ),
            (3, 4, 6)
        );
        assert_eq!(decoded.rgb, image::imageops::rotate90(&original).into_raw());
        for format in [ImageFormat::WebP, ImageFormat::Bmp] {
            let mut bytes = Cursor::new(Vec::new());
            DynamicImage::ImageRgb8(frame.clone())
                .write_to(&mut bytes, format)
                .unwrap();
            std::fs::write(&path, bytes.into_inner()).unwrap();
            assert_eq!(
                decode(&path, None, &|| false).unwrap().rgb,
                frame.as_raw().clone()
            );
        }
        assert_eq!(checked_dimensions(8000, 4001), Err(Error::Dimensions));
        assert_eq!(checked_dimensions(16385, 1), Err(Error::Dimensions));
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn codec_digest_edit_cancel_and_resource_bounds() {
        let path = std::env::temp_dir().join(format!("lumen-image-{}.png", std::process::id()));
        image::RgbImage::from_pixel(3, 2, image::Rgb([10, 20, 30]))
            .save(&path)
            .unwrap();
        let meta = inspect(&path, &|| false).unwrap();
        assert_eq!(
            (meta.width, meta.height, meta.orientation, meta.format),
            (3, 2, 1, "PNG")
        );
        assert_eq!(
            decode(&path, Some(&meta.digest), &|| false)
                .unwrap()
                .rgb
                .len(),
            18
        );
        image::RgbImage::from_pixel(3, 2, image::Rgb([30, 20, 10]))
            .save(&path)
            .unwrap();
        assert!(matches!(
            decode(&path, Some(&meta.digest), &|| false),
            Err(Error::Changed)
        ));
        assert_eq!(inspect(&path, &|| true), Err(Error::Cancelled));
        std::fs::write(&path, b"broken").unwrap();
        assert_eq!(inspect(&path, &|| false), Err(Error::Unsupported));
        std::fs::File::create(&path)
            .unwrap()
            .set_len(MAX_SOURCE_BYTES + 1)
            .unwrap();
        assert_eq!(inspect(&path, &|| false), Err(Error::TooLarge));
        std::fs::remove_file(path).unwrap();
    }
}
