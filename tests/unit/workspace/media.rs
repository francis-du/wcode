use super::*;

#[test]
fn detects_png_dimensions_without_decoder_dependency() {
    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    png.extend_from_slice(&[0, 0, 0, 13]);
    png.extend_from_slice(b"IHDR");
    png.extend_from_slice(&1920u32.to_be_bytes());
    png.extend_from_slice(&1080u32.to_be_bytes());
    let metadata = sniff_media(&png).unwrap();
    assert_eq!(metadata.kind, "image");
    assert_eq!(metadata.mime_type, "image/png");
    assert_eq!((metadata.width, metadata.height), (Some(1920), Some(1080)));
}

#[test]
fn rejects_unknown_binary_content() {
    assert!(sniff_media(b"not-media").is_err());
}
