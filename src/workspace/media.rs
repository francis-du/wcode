use super::*;

const MAX_MEDIA_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Debug)]
pub(crate) struct MediaView {
    pub path: String,
    pub sha256: String,
    pub size: u64,
    pub kind: &'static str,
    pub mime_type: &'static str,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub(crate) data: Vec<u8>,
}

impl MediaView {
    pub fn metadata(&self) -> serde_json::Value {
        serde_json::json!({
            "path": self.path,
            "sha256": self.sha256,
            "size": self.size,
            "kind": self.kind,
            "mime_type": self.mime_type,
            "width": self.width,
            "height": self.height,
        })
    }
}

impl Workspace {
    pub(crate) fn read_media(&self, path: &str) -> Result<MediaView> {
        let file = self.existing_path(path)?;
        let before = fs::metadata(&file)?;
        if !before.is_file() {
            bail!("path is not a file");
        }
        if before.len() > MAX_MEDIA_BYTES {
            bail!("media file exceeds 4 MiB read limit");
        }

        let before_stamp = source_stamp(&before);
        let data = fs::read(&file)?;
        let after = fs::metadata(&file)?;
        if before_stamp != source_stamp(&after) || after.len() as usize != data.len() {
            bail!("media file changed while it was being read; retry the operation");
        }

        let media = sniff_media(&data)?;
        Ok(MediaView {
            path: portable_relative_path(file.strip_prefix(&self.root)?),
            sha256: sha256(&data),
            size: data.len() as u64,
            kind: media.kind,
            mime_type: media.mime_type,
            width: media.width,
            height: media.height,
            data,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct MediaMetadata {
    kind: &'static str,
    mime_type: &'static str,
    width: Option<u32>,
    height: Option<u32>,
}

fn sniff_media(data: &[u8]) -> Result<MediaMetadata> {
    if let Some((width, height)) = png_dimensions(data) {
        return Ok(image("image/png", width, height));
    }
    if let Some((width, height)) = jpeg_dimensions(data) {
        return Ok(image("image/jpeg", width, height));
    }
    if let Some((width, height)) = gif_dimensions(data) {
        return Ok(image("image/gif", width, height));
    }
    if let Some((width, height)) = webp_dimensions(data) {
        return Ok(image("image/webp", width, height));
    }
    if data.starts_with(b"ID3")
        || data.starts_with(&[0xff, 0xfb])
        || data.starts_with(&[0xff, 0xf3])
        || data.starts_with(&[0xff, 0xf2])
    {
        return Ok(media("audio", "audio/mpeg"));
    }
    if data.len() >= 12 && data.starts_with(b"RIFF") && &data[8..12] == b"WAVE" {
        return Ok(media("audio", "audio/wav"));
    }
    if data.starts_with(b"OggS") {
        return Ok(media("audio", "audio/ogg"));
    }
    if data.starts_with(b"fLaC") {
        return Ok(media("audio", "audio/flac"));
    }
    if data.len() >= 12 && &data[4..8] == b"ftyp" {
        return Ok(media("video", "video/mp4"));
    }
    if data.starts_with(&[0x1a, 0x45, 0xdf, 0xa3]) {
        return Ok(media("video", "video/webm"));
    }
    bail!("unsupported media format; supported images are PNG/JPEG/GIF/WebP, audio metadata supports MP3/WAV/Ogg/FLAC, and video metadata supports MP4/WebM")
}

fn image(mime_type: &'static str, width: u32, height: u32) -> MediaMetadata {
    MediaMetadata {
        kind: "image",
        mime_type,
        width: Some(width),
        height: Some(height),
    }
}

fn media(kind: &'static str, mime_type: &'static str) -> MediaMetadata {
    MediaMetadata {
        kind,
        mime_type,
        width: None,
        height: None,
    }
}

fn png_dimensions(data: &[u8]) -> Option<(u32, u32)> {
    (data.len() >= 24 && data.starts_with(b"\x89PNG\r\n\x1a\n"))
        .then(|| {
            let width = u32::from_be_bytes(data[16..20].try_into().ok()?);
            let height = u32::from_be_bytes(data[20..24].try_into().ok()?);
            (width > 0 && height > 0).then_some((width, height))
        })
        .flatten()
}

fn gif_dimensions(data: &[u8]) -> Option<(u32, u32)> {
    if data.len() < 10 || !(data.starts_with(b"GIF87a") || data.starts_with(b"GIF89a")) {
        return None;
    }
    let width = u16::from_le_bytes([data[6], data[7]]) as u32;
    let height = u16::from_le_bytes([data[8], data[9]]) as u32;
    (width > 0 && height > 0).then_some((width, height))
}

fn jpeg_dimensions(data: &[u8]) -> Option<(u32, u32)> {
    if data.len() < 4 || data[..2] != [0xff, 0xd8] {
        return None;
    }
    let mut index = 2usize;
    while index + 4 <= data.len() {
        while index < data.len() && data[index] != 0xff {
            index += 1;
        }
        while index < data.len() && data[index] == 0xff {
            index += 1;
        }
        if index >= data.len() {
            return None;
        }
        let marker = data[index];
        index += 1;
        if matches!(marker, 0xd8 | 0xd9 | 0x01 | 0xd0..=0xd7) {
            continue;
        }
        if index + 2 > data.len() {
            return None;
        }
        let length = u16::from_be_bytes([data[index], data[index + 1]]) as usize;
        if length < 2 || index + length > data.len() {
            return None;
        }
        if matches!(marker, 0xc0..=0xc3 | 0xc5..=0xc7 | 0xc9..=0xcb | 0xcd..=0xcf) {
            if length < 7 {
                return None;
            }
            let height = u16::from_be_bytes([data[index + 3], data[index + 4]]) as u32;
            let width = u16::from_be_bytes([data[index + 5], data[index + 6]]) as u32;
            return (width > 0 && height > 0).then_some((width, height));
        }
        index += length;
    }
    None
}

fn webp_dimensions(data: &[u8]) -> Option<(u32, u32)> {
    if data.len() < 30 || !data.starts_with(b"RIFF") || &data[8..12] != b"WEBP" {
        return None;
    }
    match &data[12..16] {
        b"VP8X" => {
            let width = 1 + u32::from_le_bytes([data[24], data[25], data[26], 0]);
            let height = 1 + u32::from_le_bytes([data[27], data[28], data[29], 0]);
            Some((width, height))
        }
        b"VP8L" if data[20] == 0x2f => {
            let width = 1 + data[21] as u32 + (((data[22] & 0x3f) as u32) << 8);
            let height = 1
                + ((data[22] as u32) >> 6)
                + ((data[23] as u32) << 2)
                + (((data[24] & 0x0f) as u32) << 10);
            Some((width, height))
        }
        b"VP8 " if data[23..26] == [0x9d, 0x01, 0x2a] => {
            let width = u16::from_le_bytes([data[26], data[27]]) as u32 & 0x3fff;
            let height = u16::from_le_bytes([data[28], data[29]]) as u32 & 0x3fff;
            (width > 0 && height > 0).then_some((width, height))
        }
        _ => None,
    }
}

#[cfg(test)]
#[path = "../../tests/unit/workspace/media.rs"]
mod tests;
