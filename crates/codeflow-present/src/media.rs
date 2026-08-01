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
    fn raster_dimensions_and_pixel_count_are_bounded() {
        let mut png = fixtures().remove(0).1;
        png[16..20].copy_from_slice(&limits::MAX_RASTER_DIMENSION.to_be_bytes());
        png[20..24].copy_from_slice(&limits::MAX_RASTER_DIMENSION.to_be_bytes());
        assert!(!matches_declared_media(&MediaMime::ImagePng, &png));
    }
}
