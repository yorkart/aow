use super::image::clipboard_image_type;
fn minimal_png() -> Vec<u8> {
    let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
    bytes.extend_from_slice(&13_u32.to_be_bytes());
    bytes.extend_from_slice(b"IHDR");
    bytes.extend_from_slice(&1_u32.to_be_bytes());
    bytes.extend_from_slice(&1_u32.to_be_bytes());
    bytes.extend_from_slice(&[8, 6, 0, 0, 0]);
    bytes.extend_from_slice(&[0; 4]);
    bytes.extend_from_slice(&1_u32.to_be_bytes());
    bytes.extend_from_slice(b"IDAT");
    bytes.push(0);
    bytes.extend_from_slice(&[0; 4]);
    bytes.extend_from_slice(&0_u32.to_be_bytes());
    bytes.extend_from_slice(b"IEND");
    bytes.extend_from_slice(&[0; 4]);
    bytes
}

fn minimal_jpeg() -> Vec<u8> {
    b"\xff\xd8\xff\xc0\x00\x0b\x08\x00\x01\x00\x01\x01\x01\x11\x00\xff\xda\x00\x08\x01\x01\x00\x00\x3f\x00\x00\xff\xd9".to_vec()
}

fn minimal_gif() -> Vec<u8> {
    b"GIF89a\x01\x00\x01\x00\x80\x00\x00\x00\x00\x00\xff\xff\xff,\x00\x00\x00\x00\x01\x00\x01\x00\x00\x02\x01L\x00;".to_vec()
}

fn minimal_webp() -> Vec<u8> {
    b"RIFF\x10\x00\x00\x00WEBPVP8 \x04\x00\x00\x00data".to_vec()
}

fn extended_webp() -> Vec<u8> {
    let mut bytes = b"RIFF\x00\x00\x00\x00WEBP".to_vec();
    bytes.extend_from_slice(b"VP8X\x0a\x00\x00\x00");
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(b"EXIF\x03\x00\x00\x00abc\x00");
    bytes.extend_from_slice(b"VP8 \x04\x00\x00\x00data");
    let riff_size = (bytes.len() - 8) as u32;
    bytes[4..8].copy_from_slice(&riff_size.to_le_bytes());
    bytes
}
#[test]
fn clipboard_magic_identifies_supported_formats() {
    assert_eq!(
        clipboard_image_type(&minimal_png()).unwrap().mime,
        "image/png"
    );
    assert_eq!(
        clipboard_image_type(&minimal_jpeg()).unwrap().mime,
        "image/jpeg"
    );
    assert_eq!(
        clipboard_image_type(&minimal_gif()).unwrap().mime,
        "image/gif"
    );
    assert_eq!(
        clipboard_image_type(&minimal_webp()).unwrap().mime,
        "image/webp"
    );
    assert_eq!(
        clipboard_image_type(&extended_webp()).unwrap().mime,
        "image/webp"
    );
    assert!(clipboard_image_type(b"\x89PNG\r\n\x1a\n").is_none());
    assert!(clipboard_image_type(b"<svg").is_none());
}

#[test]
fn storage_cleans_expired_images_on_open() {
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("cache/clipboard-images");
    std::fs::create_dir_all(&directory).unwrap();
    let expired = directory.join("expired.png");
    let live = directory.join("live.png");
    std::fs::write(&expired, b"old").unwrap();
    std::fs::write(&live, b"live").unwrap();
    std::fs::File::options()
        .write(true)
        .open(&expired)
        .unwrap()
        .set_times(std::fs::FileTimes::new().set_modified(
            std::time::SystemTime::now() - super::TTL - std::time::Duration::from_secs(1),
        ))
        .unwrap();
    let storage = super::Clipboard::open(root.path()).unwrap();
    assert!(!expired.exists());
    assert!(live.exists());
    assert_eq!(storage.clean().unwrap(), 4);
}
