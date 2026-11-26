use agge_core::{Result, fail, read_asset};
use image::{ImageDecoder, codecs::png::PngDecoder};
use serde::Serialize;
use std::{io::Cursor, path::Path};
use symphonia::core::{
    codecs::DecoderOptions, formats::FormatOptions, io::MediaSourceStream, meta::MetadataOptions,
    probe::Hint,
};

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AssetInfo {
    Image {
        bytes: usize,
        size: [u32; 2],
    },
    Audio {
        bytes: usize,
        hz: u32,
        channels: usize,
        #[serde(skip_serializing_if = "Option::is_none")]
        frames: Option<u64>,
    },
}

pub fn asset_info(root: &Path, file: &str) -> Result<AssetInfo> {
    let data = read_asset(root, file)?;
    match Path::new(file).extension().and_then(|ext| ext.to_str()) {
        Some("png") => {
            let bytes = data.len();
            let decoder = png_decoder(data)?;
            let (w, h) = decoder.dimensions();
            Ok(AssetInfo::Image {
                bytes,
                size: [w, h],
            })
        }
        Some("wav" | "ogg") => audio_info(file, data),
        _ => Err(fail("asset must be PNG, WAV or Vorbis OGG")),
    }
}

pub(super) fn png_decoder(data: Vec<u8>) -> Result<PngDecoder<Cursor<Vec<u8>>>> {
    let mut decoder = PngDecoder::new(Cursor::new(data))?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(64 * 1024 * 1024);
    decoder.set_limits(limits)?;
    let (width, height) = decoder.dimensions();
    if u64::from(width) * u64::from(height) * 4 > 64 * 1024 * 1024
        || decoder.total_bytes() > 64 * 1024 * 1024
    {
        return Err(fail("decoded image exceeds 64 MiB"));
    }
    Ok(decoder)
}

pub(super) fn audio_info(file: &str, data: Vec<u8>) -> Result<AssetInfo> {
    let (extension, valid) = if file.ends_with(".wav") {
        (
            "wav",
            data.starts_with(b"RIFF") && data.get(8..12) == Some(b"WAVE"),
        )
    } else {
        ("ogg", data.starts_with(b"OggS"))
    };
    if !valid {
        return Err(fail("invalid audio container"));
    }
    let bytes = data.len();
    let mut hint = Hint::new();
    hint.with_extension(extension);
    let stream = MediaSourceStream::new(Box::new(Cursor::new(data)), Default::default());
    let mut format = symphonia::default::get_probe()
        .format(
            &hint,
            stream,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )?
        .format;
    let track = format
        .default_track()
        .ok_or_else(|| fail("no audio track"))?;
    let track_id = track.id;
    let params = &track.codec_params;
    let hz = params
        .sample_rate
        .filter(|&hz| (1..=384_000).contains(&hz))
        .ok_or_else(|| fail("invalid audio sample rate"))?;
    let channels = params
        .channels
        .map(|channels| channels.count())
        .filter(|&n| (1..=32).contains(&n))
        .ok_or_else(|| fail("invalid audio channels"))?;
    let info = AssetInfo::Audio {
        bytes,
        hz,
        channels,
        frames: params.n_frames,
    };
    let mut decoder = symphonia::default::get_codecs().make(params, &DecoderOptions::default())?;
    for _ in 0..64 {
        let packet = format.next_packet()?;
        if packet.track_id() == track_id && decoder.decode(&packet)?.frames() != 0 {
            return Ok(info);
        }
    }
    Err(fail("audio track has no samples at its start"))
}
