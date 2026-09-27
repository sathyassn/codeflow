use crate::{document::MediaMime, limits};

pub(crate) fn matches_declared_media(mime: &MediaMime, bytes: &[u8]) -> bool {
    match mime {
        MediaMime::ImagePng => valid_png(bytes),
        MediaMime::ImageJpeg => valid_jpeg(bytes),
        MediaMime::ImageGif => valid_gif(bytes),
        MediaMime::ImageWebp => valid_webp(bytes),
        MediaMime::VideoMp4 => valid_mp4(bytes),
        MediaMime::VideoWebm => valid_webm(bytes),
        MediaMime::AudioMpeg => valid_mp3(bytes),
        MediaMime::AudioOgg => valid_audio_ogg(bytes),
    }
}

fn valid_png(bytes: &[u8]) -> bool {
    bytes.len() >= 45
        && bytes.starts_with(b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR")
        && be_u32(bytes, 16)
            .zip(be_u32(bytes, 20))
            .is_some_and(|(width, height)| bounded_dimensions(width, height))
        && bytes[bytes.len() - 12..bytes.len() - 8] == [0, 0, 0, 0]
        && &bytes[bytes.len() - 8..bytes.len() - 4] == b"IEND"
}

/// A PNG crop as the page writes it: the signature, an IHDR of the crop's
/// bounded size first, whole chunks whose CRCs match, at least one IDAT, and
/// IEND as the last bytes. The size bound caps what any reader decodes, so a
/// small file cannot declare a huge raster.
pub(crate) fn crop_png_refusal(bytes: &[u8]) -> Option<&'static str> {
    const SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";
    if !bytes.starts_with(SIGNATURE) {
        return Some("is not a PNG");
    }
    let mut offset = SIGNATURE.len();
    let mut first = true;
    let mut data = false;
    loop {
        let (Some(length), Some(kind)) = (be_u32(bytes, offset), bytes.get(offset + 4..offset + 8))
        else {
            return Some("is a truncated PNG");
        };
        let Ok(length) = usize::try_from(length) else {
            return Some("is a truncated PNG");
        };
        let end = offset + 8 + length;
        let Some(stored) = be_u32(bytes, end) else {
            return Some("is a truncated PNG");
        };
        if crc32(&bytes[offset + 4..end]) != stored {
            return Some("is a PNG with a damaged chunk");
        }
        if first {
            if kind != b"IHDR" || length != 13 {
                return Some("is a PNG without its header first");
            }
            let (Some(width), Some(height)) =
                (be_u32(bytes, offset + 8), be_u32(bytes, offset + 12))
            else {
                return Some("is a truncated PNG");
            };
            if width == 0
                || height == 0
                || width > limits::MAX_CROP_WIDTH
                || height > limits::MAX_CROP_HEIGHT
            {
                return Some("is larger than a crop may be");
            }
            first = false;
        } else if kind == b"IHDR" {
            return Some("is a PNG with two headers");
        }
        data |= kind == b"IDAT";
        offset = end + 4;
        if kind == b"IEND" {
            return if offset != bytes.len() {
                Some("carries bytes after its PNG end")
            } else if data {
                None
            } else {
                Some("is a PNG with no image data")
            };
        }
    }
}

/// A minimal valid PNG of the given size (one stored zlib block of grey
/// scanlines), for tests of the crop rule.
#[cfg(test)]
pub(crate) fn test_png(width: u32, height: u32) -> Vec<u8> {
    fn chunk(out: &mut Vec<u8>, kind: &[u8], data: &[u8]) {
        out.extend_from_slice(&u32::try_from(data.len()).unwrap().to_be_bytes());
        let start = out.len();
        out.extend_from_slice(kind);
        out.extend_from_slice(data);
        let crc = crc32(&out[start..]);
        out.extend_from_slice(&crc.to_be_bytes());
    }
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut header = Vec::new();
    header.extend_from_slice(&width.to_be_bytes());
    header.extend_from_slice(&height.to_be_bytes());
    header.extend_from_slice(&[8, 0, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &header);
    // Scanlines of filter byte 0 and grey 128 pixels, stored (not deflated).
    let row = usize::try_from(width).unwrap() + 1;
    let raw: Vec<u8> = (0..usize::try_from(height).unwrap())
        .flat_map(|_| std::iter::once(0).chain(std::iter::repeat_n(128, row - 1)))
        .collect();
    let mut zlib = vec![0x78, 0x01];
    for (index, block) in raw.chunks(65_535).enumerate() {
        let last = index == raw.len().div_ceil(65_535) - 1;
        let length = u16::try_from(block.len()).unwrap();
        zlib.push(u8::from(last));
        zlib.extend_from_slice(&length.to_le_bytes());
        zlib.extend_from_slice(&(!length).to_le_bytes());
        zlib.extend_from_slice(block);
    }
    let (mut a, mut b) = (1_u32, 0_u32);
    for byte in &raw {
        a = (a + u32::from(*byte)) % 65_521;
        b = (b + a) % 65_521;
    }
    zlib.extend_from_slice(&((b << 16) | a).to_be_bytes());
    chunk(&mut out, b"IDAT", &zlib);
    chunk(&mut out, b"IEND", &[]);
    out
}

/// CRC-32 (ISO-HDLC), as PNG chunks carry it.
pub(crate) fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = u32::MAX;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0xedb8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

fn valid_jpeg(bytes: &[u8]) -> bool {
    bytes.len() >= 4
        && bytes.starts_with(b"\xff\xd8\xff")
        && bytes.ends_with(b"\xff\xd9")
        && jpeg_dimensions(bytes).is_some_and(|(width, height)| bounded_dimensions(width, height))
}

fn valid_gif(bytes: &[u8]) -> bool {
    bytes.len() >= 14
        && (bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a"))
        && le_u16(bytes, 6)
            .zip(le_u16(bytes, 8))
            .is_some_and(|(width, height)| bounded_dimensions(u32::from(width), u32::from(height)))
        && bytes.last() == Some(&0x3b)
}

fn valid_webp(bytes: &[u8]) -> bool {
    bytes.len() >= 20
        && bytes.starts_with(b"RIFF")
        && &bytes[8..12] == b"WEBP"
        && u32::from_le_bytes(bytes[4..8].try_into().expect("four-byte slice"))
            == u32::try_from(bytes.len() - 8).unwrap_or(u32::MAX)
        && webp_dimensions(bytes).is_some_and(|(width, height)| bounded_dimensions(width, height))
}

fn bounded_dimensions(width: u32, height: u32) -> bool {
    width > 0
        && height > 0
        && width <= limits::MAX_RASTER_DIMENSION
        && height <= limits::MAX_RASTER_DIMENSION
        && u64::from(width) * u64::from(height) <= limits::MAX_RASTER_PIXELS
}

fn be_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    bytes
        .get(offset..offset + 4)?
        .try_into()
        .ok()
        .map(u32::from_be_bytes)
}

fn le_u16(bytes: &[u8], offset: usize) -> Option<u16> {
    bytes
        .get(offset..offset + 2)?
        .try_into()
        .ok()
        .map(u16::from_le_bytes)
}

fn jpeg_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    let mut offset = 2;
    while offset + 4 <= bytes.len() {
        if bytes[offset] != 0xff {
            return None;
        }
        while offset < bytes.len() && bytes[offset] == 0xff {
            offset += 1;
        }
        let marker = *bytes.get(offset)?;
        offset += 1;
        if marker == 0xd9 || marker == 0xda {
            return None;
        }
        if marker == 0x01 || (0xd0..=0xd7).contains(&marker) {
            continue;
        }
        let length = usize::from(u16::from_be_bytes(
            bytes.get(offset..offset + 2)?.try_into().ok()?,
        ));
        if length < 2 || offset + length > bytes.len() {
            return None;
        }
        if matches!(marker, 0xc0..=0xc3 | 0xc5..=0xc7 | 0xc9..=0xcb | 0xcd..=0xcf) {
            if length < 7 {
                return None;
            }
            let height = u32::from(u16::from_be_bytes(
                bytes.get(offset + 3..offset + 5)?.try_into().ok()?,
            ));
            let width = u32::from(u16::from_be_bytes(
                bytes.get(offset + 5..offset + 7)?.try_into().ok()?,
            ));
            return Some((width, height));
        }
        offset += length;
    }
    None
}

fn webp_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    match bytes.get(12..16)? {
        b"VP8X" => {
            let data = bytes.get(20..30)?;
            let width = u32::from(data[4]) | (u32::from(data[5]) << 8) | (u32::from(data[6]) << 16);
            let height =
                u32::from(data[7]) | (u32::from(data[8]) << 8) | (u32::from(data[9]) << 16);
            Some((width + 1, height + 1))
        }
        b"VP8L" => {
            let data = bytes.get(20..25)?;
            if data[0] != 0x2f {
                return None;
            }
            let width = u32::from(data[1]) | (u32::from(data[2] & 0x3f) << 8);
            let height = u32::from(data[2] >> 6)
                | (u32::from(data[3]) << 2)
                | (u32::from(data[4] & 0x0f) << 10);
            Some((width + 1, height + 1))
        }
        b"VP8 " => {
            let data = bytes.get(20..30)?;
            if data.get(3..6) != Some(b"\x9d\x01\x2a") {
                return None;
            }
            Some((
                u32::from(le_u16(data, 6)? & 0x3fff),
                u32::from(le_u16(data, 8)? & 0x3fff),
            ))
        }
        _ => None,
    }
}

fn valid_mp4(bytes: &[u8]) -> bool {
    if bytes.len() < 16 || &bytes[4..8] != b"ftyp" {
        return false;
    }
    let box_size = u32::from_be_bytes(bytes[..4].try_into().expect("four-byte slice"));
    if box_size < 16 || usize::try_from(box_size).is_ok_and(|size| size > bytes.len()) {
        return false;
    }
    matches!(
        &bytes[8..12],
        b"isom" | b"iso2" | b"mp41" | b"mp42" | b"avc1" | b"dash" | b"M4V " | b"MSNV"
    )
}

fn valid_webm(bytes: &[u8]) -> bool {
    if !bytes.starts_with(b"\x1a\x45\xdf\xa3") {
        return false;
    }
    let search = &bytes[..bytes.len().min(4_096)];
    search.windows(2).enumerate().any(|(index, marker)| {
        if marker != b"\x42\x82" {
            return false;
        }
        let size_index = index + 2;
        let Some((&size, rest)) = search[size_index..].split_first() else {
            return false;
        };
        let length = usize::from(size & 0x7f);
        size & 0x80 != 0 && length == 4 && rest.get(..length) == Some(b"webm")
    })
}

fn valid_mp3(bytes: &[u8]) -> bool {
    let frame = if bytes.starts_with(b"ID3") {
        if bytes.len() < 14 || bytes[6..10].iter().any(|byte| byte & 0x80 != 0) {
            return false;
        }
        let tag_size = bytes[6..10]
            .iter()
            .fold(0_usize, |size, byte| (size << 7) | usize::from(*byte));
        let Some(frame) = bytes.get(10_usize.saturating_add(tag_size)..) else {
            return false;
        };
        frame
    } else {
        bytes
    };
    let Some(header) = frame.get(..4) else {
        return false;
    };
    header[0] == 0xff
        && header[1] & 0xe0 == 0xe0
        && header[1] & 0x18 != 0x08
        && header[1] & 0x06 != 0
        && !matches!(header[2] >> 4, 0 | 15)
        && header[2] & 0x0c != 0x0c
}

fn valid_audio_ogg(bytes: &[u8]) -> bool {
    if bytes.len() < 28 || !bytes.starts_with(b"OggS") || bytes[4] != 0 {
        return false;
    }
    let segments = usize::from(bytes[26]);
    let Some(table) = bytes.get(27..27 + segments) else {
        return false;
    };
    let payload_offset = 27 + segments;
    let payload_len = table.iter().map(|size| usize::from(*size)).sum::<usize>();
    let Some(payload) = bytes.get(payload_offset..payload_offset.saturating_add(payload_len))
    else {
        return false;
    };
    payload.starts_with(b"OpusHead")
        || payload.starts_with(b"\x01vorbis")
        || payload.starts_with(b"Speex   ")
        || payload.starts_with(b"\x7fFLAC")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixtures() -> Vec<(MediaMime, Vec<u8>)> {
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\0\x01\0\0\0\x01".to_vec();
        png.extend_from_slice(&[0; 9]);
        png.extend_from_slice(b"\0\0\0\0IEND\0\0\0\0");
        let jpeg = b"\xff\xd8\xff\xc0\0\x0b\x08\0\x01\0\x01\x01\x01\x11\0\xff\xd9".to_vec();
        let gif = b"GIF89a\x01\0\x01\0\0\0\0;".to_vec();
        let webp_bytes = b"RIFF\x16\0\0\0WEBPVP8 \x0a\0\0\0\0\0\0\x9d\x01\x2a\x01\0\x01\0".to_vec();
        let mp4 = b"\0\0\0\x10ftypisom\0\0\0\0".to_vec();
        let ebml_video = b"\x1a\x45\xdf\xa3\x42\x82\x84webm".to_vec();
        let mp3 = b"\xff\xfb\x90\x64".to_vec();
        let mut ogg = vec![0; 28];
        ogg[..4].copy_from_slice(b"OggS");
        ogg[26] = 1;
        ogg[27] = 8;
        ogg.extend_from_slice(b"OpusHead");
        vec![
            (MediaMime::ImagePng, png),
            (MediaMime::ImageJpeg, jpeg),
            (MediaMime::ImageGif, gif),
            (MediaMime::ImageWebp, webp_bytes),
            (MediaMime::VideoMp4, mp4),
            (MediaMime::VideoWebm, ebml_video),
            (MediaMime::AudioMpeg, mp3),
            (MediaMime::AudioOgg, ogg),
        ]
    }

    #[test]
    fn every_closed_media_family_accepts_its_structure_and_rejects_a_mismatch() {
        let fixtures = fixtures();
        for (index, (mime, bytes)) in fixtures.iter().enumerate() {
            assert!(matches_declared_media(mime, bytes), "valid fixture {index}");
            let wrong = &fixtures[(index + 1) % fixtures.len()].0;
            assert!(
                !matches_declared_media(wrong, bytes),
                "fixture {index} matched the next MIME family"
            );
        }
    }

    #[test]
    fn obvious_prefix_or_suffix_polyglots_are_rejected() {
        for (mime, mut bytes) in fixtures() {
            bytes.extend_from_slice(b"<script>");
            if matches!(
                mime,
                MediaMime::ImageJpeg | MediaMime::ImageGif | MediaMime::ImageWebp
            ) {
                assert!(!matches_declared_media(&mime, &bytes));
            }
        }
    }

    #[test]
    fn a_crop_png_is_whole_and_crop_sized() {
        assert_eq!(crop_png_refusal(&test_png(1, 1)), None);
        assert_eq!(crop_png_refusal(&test_png(480, 360)), None);
        let valid = test_png(4, 3);
        let mut damaged = valid.clone();
        damaged[40] ^= 0xff;
        let mut trailing = valid.clone();
        trailing.push(0);
        let without_data = {
            let mut png = test_png(1, 1);
            // Drop the IDAT chunk: header ends at 33, IEND is the last 12.
            png.drain(33..png.len() - 12);
            png
        };
        for (bytes, expected) in [
            (b"\x89PNG\r\n\x1a\n".to_vec(), "is a truncated PNG"),
            (valid[..valid.len() - 5].to_vec(), "is a truncated PNG"),
            (damaged, "is a PNG with a damaged chunk"),
            (trailing, "carries bytes after its PNG end"),
            (without_data, "is a PNG with no image data"),
            (test_png(481, 1), "is larger than a crop may be"),
            (test_png(1, 361), "is larger than a crop may be"),
            (b"GIF89a".to_vec(), "is not a PNG"),
        ] {
            assert_eq!(crop_png_refusal(&bytes), Some(expected));
        }
        // A small file declaring a huge raster: 8192 by 8192 in 60 bytes.
        let mut huge = test_png(1, 1);
        huge[16..20].copy_from_slice(&8192_u32.to_be_bytes());
        huge[20..24].copy_from_slice(&8192_u32.to_be_bytes());
        let crc = crc32(&huge[12..29]);
        huge[29..33].copy_from_slice(&crc.to_be_bytes());
        assert_eq!(
            crop_png_refusal(&huge),
            Some("is larger than a crop may be")
        );
    }

    #[test]
    fn raster_dimensions_and_pixel_count_are_bounded() {
        let mut png = fixtures().remove(0).1;
        png[16..20].copy_from_slice(&limits::MAX_RASTER_DIMENSION.to_be_bytes());
        png[20..24].copy_from_slice(&limits::MAX_RASTER_DIMENSION.to_be_bytes());
        assert!(!matches_declared_media(&MediaMime::ImagePng, &png));
    }
}
