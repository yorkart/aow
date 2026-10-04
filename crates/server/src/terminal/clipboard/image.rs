#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::terminal) struct ClipboardImageType {
    pub(in crate::terminal) mime: &'static str,
    pub(in crate::terminal) extension: &'static str,
}

pub(in crate::terminal) fn clipboard_image_type(bytes: &[u8]) -> Option<ClipboardImageType> {
    if valid_png(bytes) {
        Some(ClipboardImageType {
            mime: "image/png",
            extension: "png",
        })
    } else if valid_jpeg(bytes) {
        Some(ClipboardImageType {
            mime: "image/jpeg",
            extension: "jpg",
        })
    } else if valid_gif(bytes) {
        Some(ClipboardImageType {
            mime: "image/gif",
            extension: "gif",
        })
    } else if valid_webp(bytes) {
        Some(ClipboardImageType {
            mime: "image/webp",
            extension: "webp",
        })
    } else {
        None
    }
}

fn valid_png(bytes: &[u8]) -> bool {
    if bytes.len() < 45 || !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return false;
    }
    let mut offset = 8;
    let mut saw_header = false;
    let mut saw_image_data = false;
    while offset + 12 <= bytes.len() {
        let length = u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
        let end = match offset
            .checked_add(12)
            .and_then(|base| base.checked_add(length))
        {
            Some(end) if end <= bytes.len() => end,
            _ => return false,
        };
        let kind = &bytes[offset + 4..offset + 8];
        if !saw_header {
            if kind != b"IHDR" || length != 13 {
                return false;
            }
            let width = u32::from_be_bytes(bytes[offset + 8..offset + 12].try_into().unwrap());
            let height = u32::from_be_bytes(bytes[offset + 12..offset + 16].try_into().unwrap());
            if width == 0 || height == 0 {
                return false;
            }
            saw_header = true;
        }
        if kind == b"IDAT" && length > 0 {
            saw_image_data = true;
        }
        if kind == b"IEND" {
            return saw_header && saw_image_data && length == 0 && end == bytes.len();
        }
        offset = end;
    }
    false
}

fn valid_jpeg(bytes: &[u8]) -> bool {
    if bytes.len() < 24 || !bytes.starts_with(b"\xff\xd8") || !bytes.ends_with(b"\xff\xd9") {
        return false;
    }
    let mut offset = 2;
    let mut saw_frame = false;
    while offset + 1 < bytes.len() - 2 {
        if bytes[offset] != 0xff {
            return false;
        }
        while offset < bytes.len() && bytes[offset] == 0xff {
            offset += 1;
        }
        if offset >= bytes.len() {
            return false;
        }
        let marker = bytes[offset];
        offset += 1;
        if marker == 0xda {
            return saw_frame;
        }
        if marker == 0x01 || (0xd0..=0xd7).contains(&marker) {
            continue;
        }
        if offset + 2 > bytes.len() - 2 {
            return false;
        }
        let length = u16::from_be_bytes(bytes[offset..offset + 2].try_into().unwrap()) as usize;
        if length < 2 || offset + length > bytes.len() - 2 {
            return false;
        }
        if matches!(marker, 0xc0..=0xc3 | 0xc5..=0xc7 | 0xc9..=0xcb | 0xcd..=0xcf) {
            if length < 8 {
                return false;
            }
            let height = u16::from_be_bytes(bytes[offset + 3..offset + 5].try_into().unwrap());
            let width = u16::from_be_bytes(bytes[offset + 5..offset + 7].try_into().unwrap());
            if width == 0 || height == 0 {
                return false;
            }
            saw_frame = true;
        }
        offset += length;
    }
    false
}

fn valid_gif(bytes: &[u8]) -> bool {
    bytes.len() >= 14
        && (bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a"))
        && bytes[6..10].iter().any(|byte| *byte != 0)
        && bytes[10..bytes.len() - 1].contains(&0x2c)
        && bytes.last() == Some(&0x3b)
}

fn valid_webp(bytes: &[u8]) -> bool {
    if bytes.len() < 20 || &bytes[..4] != b"RIFF" || &bytes[8..12] != b"WEBP" {
        return false;
    }
    let declared = u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize + 8;
    if declared != bytes.len() {
        return false;
    }

    let mut offset = 12;
    let mut saw_extended_header = false;
    let mut saw_extended_chunk = false;
    let mut saw_image_data = false;
    while offset + 8 <= bytes.len() {
        let kind = &bytes[offset..offset + 4];
        let size = u32::from_le_bytes(bytes[offset + 4..offset + 8].try_into().unwrap()) as usize;
        let data_start = offset + 8;
        let Some(data_end) = data_start.checked_add(size) else {
            return false;
        };
        let Some(next) = data_end.checked_add(size & 1) else {
            return false;
        };
        if next > bytes.len() {
            return false;
        }
        match kind {
            b"VP8 " if size > 0 => saw_image_data = true,
            b"VP8L" if size >= 5 && bytes[data_start] == 0x2f => saw_image_data = true,
            b"VP8X" if size == 10 && !saw_extended_header => saw_extended_header = true,
            b"ALPH" if size > 0 => saw_extended_chunk = true,
            b"ANIM" if size == 6 => saw_extended_chunk = true,
            b"ANMF" if size >= 16 => {
                saw_extended_chunk = true;
                saw_image_data = true;
            }
            b"ICCP" | b"EXIF" | b"XMP " => saw_extended_chunk = true,
            b"VP8 " | b"VP8L" | b"VP8X" | b"ALPH" | b"ANIM" | b"ANMF" => return false,
            _ => {}
        }
        offset = next;
    }
    offset == bytes.len() && saw_image_data && (!saw_extended_chunk || saw_extended_header)
}
