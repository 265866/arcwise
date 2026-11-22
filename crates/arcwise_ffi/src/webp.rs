//! WebP pictures through libwebp, which decodes the library's stills three times faster than the
//! `image` crate's decoder.

use std::sync::Once;
use std::{ptr, slice};

use libwebp_sys::{
    WebPDecodeBGRAInto, WebPEncodeLosslessRGBA, WebPEncodeRGBA, WebPFree, WebPGetInfo,
};
use thiserror::Error;

/// A decoded picture: BGRA, top row first, rows packed with no padding.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bgra {
    /// The width in pixels.
    pub width: u32,
    /// The height in pixels.
    pub height: u32,
    /// Four bytes a pixel, blue, green, red and alpha, `width * 4` bytes a row.
    pub pixels: Vec<u8>,
}

/// Why libwebp refused a picture.
#[must_use]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Error)]
pub enum WebpError {
    /// The bytes don't start a WebP picture.
    #[error("not a WebP picture")]
    NotWebp,
    /// The picture's header is readable but its data isn't.
    #[error("the WebP data is damaged")]
    Damaged,
    /// The picture's size overflows the arithmetic of its buffer or of libwebp's arguments.
    #[error("the WebP picture is too large")]
    TooLarge,
    /// The pixels given don't hold exactly the width by height asked for.
    #[error("the pixels don't match the size given")]
    WrongLength,
    /// The encoder refused the pixels.
    #[error("libwebp could not encode the picture")]
    Encode,
}

/// Decodes a WebP picture to BGRA.
///
/// # Errors
///
/// [`WebpError::NotWebp`] when `bytes` isn't a WebP picture, [`WebpError::Damaged`] when its data
/// can't be decoded, and [`WebpError::TooLarge`] when its size overflows the buffer's arithmetic.
#[expect(
    unsafe_code,
    reason = "WebPGetInfo and WebPDecodeBGRAInto are libwebp's C functions"
)]
pub fn decode_bgra(bytes: &[u8]) -> Result<Bgra, WebpError> {
    set_up();
    let mut width = 0;
    let mut height = 0;
    let (data, data_len) = (bytes.as_ptr(), bytes.len());
    // SAFETY: libwebp reads at most `data_len` bytes from `data` and writes only the two sizes.
    let known = unsafe { WebPGetInfo(data, data_len, &raw mut width, &raw mut height) };
    if known == 0 {
        return Err(WebpError::NotWebp);
    }
    let width = u32::try_from(width).map_err(|_| WebpError::NotWebp)?;
    let height = u32::try_from(height).map_err(|_| WebpError::NotWebp)?;
    let stride = usize::try_from(width)
        .ok()
        .and_then(|width| width.checked_mul(4))
        .ok_or(WebpError::TooLarge)?;
    let len = usize::try_from(height)
        .ok()
        .and_then(|height| height.checked_mul(stride))
        .ok_or(WebpError::TooLarge)?;
    let stride_arg = i32::try_from(stride).map_err(|_| WebpError::TooLarge)?;
    let mut pixels = vec![0_u8; len];
    // SAFETY: `pixels` holds `stride` bytes for each of `height` rows, which bounds what libwebp
    // writes; it reads at most `bytes.len()` bytes.
    let decoded = unsafe {
        WebPDecodeBGRAInto(
            bytes.as_ptr(),
            bytes.len(),
            pixels.as_mut_ptr(),
            pixels.len(),
            stride_arg,
        )
    };
    if decoded.is_null() {
        return Err(WebpError::Damaged);
    }
    Ok(Bgra {
        width,
        height,
        pixels,
    })
}

/// Encodes RGBA pixels, `width * 4` bytes a row, as a lossless WebP picture.
///
/// # Errors
///
/// [`WebpError::WrongLength`] when `rgba` doesn't hold `width` by `height` pixels,
/// [`WebpError::TooLarge`] when the size doesn't fit libwebp's arguments, and
/// [`WebpError::Encode`] when libwebp refuses the pixels.
#[expect(
    unsafe_code,
    reason = "WebPEncodeLosslessRGBA and WebPFree are libwebp's C functions"
)]
pub fn encode_lossless_rgba(rgba: &[u8], width: u32, height: u32) -> Result<Vec<u8>, WebpError> {
    let width_arg = i32::try_from(width).map_err(|_| WebpError::TooLarge)?;
    let height_arg = i32::try_from(height).map_err(|_| WebpError::TooLarge)?;
    let stride = width_arg.checked_mul(4).ok_or(WebpError::TooLarge)?;
    let needed = usize::try_from(stride)
        .ok()
        .and_then(|stride| stride.checked_mul(usize::try_from(height).ok()?))
        .ok_or(WebpError::TooLarge)?;
    if rgba.len() != needed {
        return Err(WebpError::WrongLength);
    }
    let mut out = ptr::null_mut();
    // SAFETY: `rgba` holds `height` rows of `stride` bytes, which bounds what libwebp reads, and
    // libwebp writes only `out`.
    let len = unsafe {
        WebPEncodeLosslessRGBA(rgba.as_ptr(), width_arg, height_arg, stride, &raw mut out)
    };
    if len == 0 || out.is_null() {
        return Err(WebpError::Encode);
    }
    // SAFETY: libwebp allocated `len` bytes at `out` for this call, and nothing frees them until
    // after they are copied.
    let encoded = unsafe { slice::from_raw_parts(out, len) }.to_vec();
    // SAFETY: `out` is libwebp's own allocation, freed once, after the copy above.
    unsafe { WebPFree(out.cast()) };
    Ok(encoded)
}

/// Has libwebp fill in its tables of decoding functions, once, before any decode. It fills them
/// on first use, without a lock on Windows, and pictures decoded at once on several threads at
/// launch could catch a table half filled and loop forever. Decoding a tiny picture of each kind
/// here, on one thread, fills them all: lossy, lossy with alpha (whose alpha is lossless), and
/// lossless.
fn set_up() {
    static SET_UP: Once = Once::new();
    SET_UP.call_once(|| {
        for kind in [
            Kind::Lossy { alpha: 255 },
            Kind::Lossy { alpha: 128 },
            Kind::Lossless,
        ] {
            decode_tiny(kind);
        }
    });
}

/// The kinds of WebP picture whose decoding tables differ.
#[derive(Clone, Copy)]
enum Kind {
    Lossy { alpha: u8 },
    Lossless,
}

/// Encodes and decodes a 2x2 picture of one kind, for the tables it fills.
#[expect(
    unsafe_code,
    reason = "the WebPEncode functions, WebPDecodeBGRAInto and WebPFree are libwebp's C functions"
)]
fn decode_tiny(kind: Kind) {
    let alpha = match kind {
        Kind::Lossy { alpha } => alpha,
        Kind::Lossless => 255,
    };
    let rgba = [200_u8, 100, 50, alpha].repeat(4);
    let mut encoded = ptr::null_mut();
    let mut bgra = [0_u8; 16];
    let len = match kind {
        // SAFETY: `rgba` holds 2x2 pixels at a stride of 8 bytes, and libwebp writes only
        // `encoded`.
        Kind::Lossless => unsafe {
            WebPEncodeLosslessRGBA(rgba.as_ptr(), 2, 2, 8, &raw mut encoded)
        },
        // SAFETY: as above.
        Kind::Lossy { .. } => unsafe {
            WebPEncodeRGBA(rgba.as_ptr(), 2, 2, 8, 90., &raw mut encoded)
        },
    };
    if len > 0 && !encoded.is_null() {
        // SAFETY: `encoded` holds the `len` bytes libwebp just wrote, and `bgra` has room for 2x2
        // pixels at a stride of 8 bytes.
        let decoded = unsafe { WebPDecodeBGRAInto(encoded, len, bgra.as_mut_ptr(), 16, 8) };
        debug_assert!(!decoded.is_null(), "a picture libwebp just encoded decodes");
    }
    // SAFETY: `encoded` is null or libwebp's own allocation from above, freed once.
    unsafe { WebPFree(encoded.cast()) };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_what_it_encodes_as_bgra() {
        let top_row = [255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255];
        let bottom_row = [10, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110, 120];
        let rgba = [top_row, bottom_row].concat();
        let encoded = encode_lossless_rgba(&rgba, 3, 2).expect("3x2 pixels encode");
        let picture = decode_bgra(&encoded).expect("the picture decodes");
        assert_eq!((picture.width, picture.height), (3, 2), "the size survives");
        assert_eq!(
            &picture.pixels[..4],
            &[0, 0, 255, 255],
            "red, stored blue first"
        );
        assert_eq!(
            &picture.pixels[12..16],
            &[30, 20, 10, 40],
            "lossless keeps every channel, alpha included"
        );
    }

    #[test]
    fn refuses_what_is_not_webp() {
        assert_eq!(
            decode_bgra(b"not a picture"),
            Err(WebpError::NotWebp),
            "arbitrary bytes"
        );
    }

    #[test]
    fn refuses_pixels_that_do_not_fit_the_size() {
        assert_eq!(
            encode_lossless_rgba(&[0; 12], 2, 2),
            Err(WebpError::WrongLength),
            "2x2 pixels need 16 bytes"
        );
    }

    #[test]
    fn refuses_a_picture_cut_off_after_its_header() {
        let rgba: Vec<u8> = (0..64_u8).map(|i| i.wrapping_mul(37)).collect();
        let encoded = encode_lossless_rgba(&rgba, 4, 4).expect("4x4 pixels encode");
        let header_only = &encoded[..30];
        assert_eq!(
            decode_bgra(header_only),
            Err(WebpError::Damaged),
            "the size is readable, the pixels aren't"
        );
    }
}
