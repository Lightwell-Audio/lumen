//! The ICO container: one 32-bit image per size, the 256 px one as PNG
//! (the form the Windows shell reads for its large tiles), the others as
//! uncompressed bitmaps with an empty mask (their alpha carries the shape).

/// An ICO file holding `images`: `(size, straight RGBA)` pairs, each image
/// `size` pixels square.
///
/// # Panics
/// If an image is larger than 256 pixels or its pixel data is the wrong
/// length: both are mistakes in the caller's code, not in input.
pub fn ico(images: &[(u32, &[u8])]) -> Vec<u8> {
    let entries: Vec<Vec<u8>> = images
        .iter()
        .map(|&(size, rgba)| {
            assert!((1..=256).contains(&size), "an ICO image is 1 to 256 px");
            assert_eq!(rgba.len(), (size * size * 4) as usize, "RGBA of {size} px");
            if size == 256 {
                png(size, rgba)
            } else {
                bitmap(size, rgba)
            }
        })
        .collect();
    let mut out = Vec::new();
    // ICONDIR: reserved, type 1 (icon), count.
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&(images.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * images.len() as u32;
    for (&(size, _), data) in images.iter().zip(&entries) {
        // ICONDIRENTRY: 256 is written as 0.
        let side = if size == 256 { 0 } else { size as u8 };
        out.extend_from_slice(&[side, side, 0, 0]);
        out.extend_from_slice(&1u16.to_le_bytes()); // planes
        out.extend_from_slice(&32u16.to_le_bytes()); // bits per pixel
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&offset.to_le_bytes());
        offset += data.len() as u32;
    }
    for data in entries {
        out.extend_from_slice(&data);
    }
    out
}

/// A BITMAPINFOHEADER, the pixels bottom up as BGRA, then a mask of zeros.
fn bitmap(size: u32, rgba: &[u8]) -> Vec<u8> {
    let mask_row = size.div_ceil(32) * 4;
    let pixels = size * size * 4;
    let mut out = Vec::with_capacity((40 + pixels + mask_row * size) as usize);
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&(size as i32).to_le_bytes());
    // The height counts the colour image and the mask.
    out.extend_from_slice(&(2 * size as i32).to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&32u16.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes()); // BI_RGB
    out.extend_from_slice(&(pixels + mask_row * size).to_le_bytes());
    out.extend_from_slice(&[0; 16]); // resolution and palette: unused
    for y in (0..size).rev() {
        for x in 0..size {
            let i = ((y * size + x) * 4) as usize;
            out.extend_from_slice(&[rgba[i + 2], rgba[i + 1], rgba[i], rgba[i + 3]]);
        }
    }
    out.resize(out.len() + (mask_row * size) as usize, 0);
    out
}

fn png(size: u32, rgba: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, size, size);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    // Writing to memory cannot fail but for a wrong length, checked above.
    let mut writer = encoder.write_header().expect("PNG header in memory");
    writer.write_image_data(rgba).expect("PNG data in memory");
    writer.finish().expect("PNG end in memory");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ICO_SIZES, app_ico};

    fn u16_at(b: &[u8], i: usize) -> u16 {
        u16::from_le_bytes([b[i], b[i + 1]])
    }
    fn u32_at(b: &[u8], i: usize) -> u32 {
        u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]])
    }

    #[test]
    fn the_directory_lists_every_size_and_points_at_its_image() {
        let file = app_ico();
        assert_eq!(u16_at(&file, 0), 0);
        assert_eq!(u16_at(&file, 2), 1);
        assert_eq!(u16_at(&file, 4) as usize, ICO_SIZES.len());
        let mut end = 6 + 16 * ICO_SIZES.len();
        for (n, &size) in ICO_SIZES.iter().enumerate() {
            let e = 6 + 16 * n;
            let side = if size == 256 { 0 } else { size as u8 };
            assert_eq!(&file[e..e + 2], &[side, side]);
            assert_eq!(u16_at(&file, e + 6), 32);
            let len = u32_at(&file, e + 8) as usize;
            let at = u32_at(&file, e + 12) as usize;
            assert_eq!(at, end, "images follow one another");
            end += len;
            let image = &file[at..at + len];
            if size == 256 {
                assert_eq!(&image[..8], b"\x89PNG\r\n\x1a\n");
            } else {
                assert_eq!(u32_at(image, 0), 40);
                assert_eq!(u32_at(image, 4), size);
                assert_eq!(u32_at(image, 8), 2 * size);
            }
        }
        assert_eq!(end, file.len());
    }

    #[test]
    fn bitmaps_are_bottom_up_bgra() {
        // A 2 px image: top left red, bottom left blue.
        let mut rgba = vec![0u8; 16];
        rgba[..4].copy_from_slice(&[255, 0, 0, 255]);
        rgba[8..12].copy_from_slice(&[0, 0, 255, 128]);
        let file = ico(&[(2, &rgba)]);
        let image = &file[6 + 16..];
        // The first row written is the bottom one: blue as BGRA.
        assert_eq!(&image[40..44], &[255, 0, 0, 128]);
        // Then the top row: red as BGRA.
        assert_eq!(&image[48..52], &[0, 0, 255, 255]);
        // A mask row is padded to 4 bytes: 2 rows of zeros.
        assert_eq!(image.len(), 40 + 16 + 8);
    }

    #[test]
    fn the_256_px_image_is_a_png_that_decodes_to_the_drawing() {
        let rgba = crate::project_icon(256);
        let file = ico(&[(256, &rgba)]);
        let at = u32_at(&file, 6 + 12) as usize;
        let decoder = png::Decoder::new(std::io::Cursor::new(&file[at..]));
        let mut reader = decoder.read_info().unwrap();
        let mut buf = vec![0; reader.output_buffer_size().unwrap()];
        reader.next_frame(&mut buf).unwrap();
        assert_eq!(buf, rgba);
    }
}
