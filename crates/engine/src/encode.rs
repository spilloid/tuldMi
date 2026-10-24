//! Ogg Opus encoding of speech segments (16 kHz mono in, voice-tuned), waveform peaks, and
//! decoding back to PCM for transcription.
//!
//! Files are written atomically: encode to `<final>.part` in the same directory, fsync, then
//! rename. A crash never leaves a truncated file at the final path.
//!
//! The stream is RFC 7845 compliant (OpusHead with correct pre-skip, OpusTags, granule
//! positions at 48 kHz) so WebView2/Chromium `<audio>` can play and seek it directly.

use std::fs::{File, OpenOptions};
use std::io::{BufReader, BufWriter, Write};
use std::path::Path;

use crate::{Error, Result};
use ogg::{PacketReader, PacketWriteEndInfo, PacketWriter};
use opus::{Application, Bitrate, Channels, Decoder, Encoder};

pub const OPUS_BITRATE: i32 = 24_000;
/// 20 ms Opus frames at 16 kHz.
pub const OPUS_FRAME_SAMPLES: usize = 320;

#[derive(Debug, Clone, PartialEq)]
pub struct EncodedAudio {
    pub bytes: u64,
    pub duration_ms: i64,
    /// `types::PEAKS_PER_SECOND` values per second of audio, each the max |sample| in its
    /// window scaled to 0..=255. Length = ceil(duration_s * PEAKS_PER_SECOND).
    pub peaks: Vec<u8>,
}

/// Encode 16 kHz mono f32 samples (-1..=1; out-of-range values are clamped) to `path`.
pub fn write_ogg_opus(path: &Path, samples_16k: &[f32]) -> Result<EncodedAudio> {
    let mut encoder = Encoder::new(16_000, Channels::Mono, Application::Voip).map_err(codec)?;
    encoder
        .set_bitrate(Bitrate::Bits(OPUS_BITRATE))
        .map_err(codec)?;
    let lookahead = u64::try_from(encoder.get_lookahead().map_err(codec)?).map_err(codec)?;
    let pre_skip = u16::try_from(
        lookahead
            .checked_mul(3)
            .ok_or_else(|| codec("Opus lookahead overflow"))?,
    )
    .map_err(codec)?;
    let original_len = u64::try_from(samples_16k.len()).map_err(codec)?;
    let final_granule = original_len
        .checked_mul(3)
        .and_then(|len| len.checked_add(u64::from(pre_skip)))
        .filter(|&gp| gp <= i64::MAX as u64)
        .ok_or_else(|| codec("audio length exceeds the Ogg granule range"))?;
    // Feed the encoder's delay out as well as padding the final frame. Otherwise a clip
    // ending on a frame boundary loses its last lookahead samples after pre-skip removal.
    let frames = (original_len + lookahead)
        .div_ceil(OPUS_FRAME_SAMPLES as u64)
        .max(1);
    let mut head = b"OpusHead".to_vec();
    head.extend_from_slice(&[1, 1]);
    head.extend_from_slice(&pre_skip.to_le_bytes());
    head.extend_from_slice(&16_000u32.to_le_bytes());
    head.extend_from_slice(&0i16.to_le_bytes());
    head.push(0); // Mapping family zero: mono, no channel mapping table.
    let mut tags = b"OpusTags".to_vec();
    tags.extend_from_slice(&6u32.to_le_bytes());
    tags.extend_from_slice(b"tuldMi");
    tags.extend_from_slice(&0u32.to_le_bytes());

    let mut part_name = path.as_os_str().to_os_string();
    part_name.push(".part");
    let part_path = std::path::PathBuf::from(part_name);
    // Do not truncate a temporary file belonging to another concurrent writer.
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&part_path)?;
    let result = (|| -> Result<u64> {
        let mut writer = PacketWriter::new(BufWriter::new(file));
        let serial = uuid::Uuid::new_v4().as_u128() as u32;
        writer.write_packet(head, serial, PacketWriteEndInfo::EndPage, 0)?;
        writer.write_packet(tags, serial, PacketWriteEndInfo::EndPage, 0)?;
        let mut pcm = [0.0f32; OPUS_FRAME_SAMPLES];
        let mut packet = [0u8; 1275];
        for index in 0..frames {
            pcm.fill(0.0);
            let start = usize::try_from(index * OPUS_FRAME_SAMPLES as u64).map_err(codec)?;
            if let Some(tail) = samples_16k.get(start..) {
                for (dst, &src) in pcm.iter_mut().zip(tail) {
                    *dst = clean_sample(src);
                }
            }
            let len = encoder.encode_float(&pcm, &mut packet).map_err(codec)?;
            let last = index + 1 == frames;
            let end = if last {
                PacketWriteEndInfo::EndStream
            } else if (index + 1).is_multiple_of(50) {
                PacketWriteEndInfo::EndPage
            } else {
                PacketWriteEndInfo::NormalPacket
            };
            let granule = if last {
                final_granule
            } else {
                (index + 1) * OPUS_FRAME_SAMPLES as u64 * 3
            };
            writer.write_packet(packet[..len].to_vec(), serial, end, granule)?;
        }
        let mut output = writer.into_inner();
        output.flush()?;
        output.get_ref().sync_all()?;
        let bytes = output.get_ref().metadata()?.len();
        // Close the handle before renaming, including on Windows.
        drop(output);
        std::fs::rename(&part_path, path)?;
        Ok(bytes)
    })();
    if result.is_err() {
        // Best effort cleanup; preserve the original codec or I/O error.
        let _ = std::fs::remove_file(&part_path);
    }
    Ok(EncodedAudio {
        bytes: result?,
        duration_ms: (original_len / 16) as i64,
        peaks: peaks(samples_16k),
    })
}

/// Decode an Ogg Opus file written by `write_ogg_opus` back to 16 kHz mono f32, with
/// pre-skip removed and the end trimmed to the original length (from the final granule).
pub fn read_ogg_opus_16k(path: &Path) -> Result<Vec<f32>> {
    let mut reader = PacketReader::new(BufReader::new(File::open(path)?));
    let head = reader
        .read_packet()
        .map_err(ogg_error)?
        .ok_or_else(|| codec("missing OpusHead"))?;
    if head.data.len() != 19
        || !head.data.starts_with(b"OpusHead")
        || head.data[8] != 1
        || head.data[9] != 1
        || head.data[18] != 0
        || !head.first_in_stream()
        || !head.last_in_page()
        || head.absgp_page() != 0
    {
        return Err(codec("unsupported or invalid mono OpusHead"));
    }
    let serial = head.stream_serial();
    let pre_skip = u64::from(u16::from_le_bytes([head.data[10], head.data[11]]));
    let gain = i16::from_le_bytes([head.data[16], head.data[17]]);
    let tags = reader
        .read_packet()
        .map_err(ogg_error)?
        .ok_or_else(|| codec("missing OpusTags"))?;
    if tags.stream_serial() != serial || !tags.data.starts_with(b"OpusTags") {
        return Err(codec("invalid OpusTags"));
    }
    validate_tags(&tags.data[8..])?;

    let mut decoder = Decoder::new(16_000, Channels::Mono).map_err(codec)?;
    decoder.set_gain(i32::from(gain)).map_err(codec)?;
    let mut skip_left = (pre_skip / 3) as usize;
    let mut decoded = Vec::new();
    let mut pcm = [0.0f32; 1920]; // Opus packets may contain up to 120 ms.
    let mut total = 0u64;
    let mut previous_granule = 0;
    let final_granule = loop {
        let packet = reader
            .read_packet()
            .map_err(ogg_error)?
            .ok_or_else(|| codec("missing Ogg end-of-stream page"))?;
        if packet.stream_serial() != serial || packet.data.is_empty() || packet.data.len() > 61_440
        {
            return Err(codec("invalid Opus audio packet"));
        }
        let n = decoder
            .decode_float(&packet.data, &mut pcm, false)
            .map_err(codec)?;
        total = total
            .checked_add(n as u64)
            .ok_or_else(|| codec("decoded length overflow"))?;
        let skip = skip_left.min(n);
        skip_left -= skip;
        decoded.try_reserve(n - skip).map_err(codec)?;
        decoded.extend_from_slice(&pcm[skip..n]);
        if packet.last_in_page() {
            let granule = packet.absgp_page();
            let available = total
                .checked_mul(3)
                .ok_or_else(|| codec("decoded length overflow"))?;
            if granule < previous_granule || granule > available || granule > i64::MAX as u64 {
                return Err(codec("invalid Ogg Opus granule position"));
            }
            previous_granule = granule;
        }
        if packet.last_in_stream() {
            break packet.absgp_page();
        }
    };
    let length = final_granule
        .checked_sub(pre_skip)
        .ok_or_else(|| codec("final granule precedes pre-skip"))?
        / 3;
    let length = usize::try_from(length).map_err(codec)?;
    if skip_left != 0 || length > decoded.len() {
        return Err(codec("truncated Opus audio"));
    }
    if reader.read_packet().map_err(ogg_error)?.is_some() {
        return Err(codec("multiple logical streams are not supported"));
    }
    decoded.truncate(length);
    Ok(decoded)
}

/// Peak envelope as described on `EncodedAudio::peaks`.
pub fn peaks(samples_16k: &[f32]) -> Vec<u8> {
    let window = (16_000 / crate::types::PEAKS_PER_SECOND) as usize;
    samples_16k
        .chunks(window)
        .map(|chunk| {
            let peak = chunk
                .iter()
                .fold(0.0f32, |peak, &sample| peak.max(clean_sample(sample).abs()));
            (peak * 255.0).round() as u8
        })
        .collect()
}

fn clean_sample(sample: f32) -> f32 {
    if sample.is_nan() {
        0.0
    } else {
        sample.clamp(-1.0, 1.0)
    }
}

fn codec(error: impl std::fmt::Display) -> Error {
    Error::Codec(error.to_string())
}

fn ogg_error(error: ogg::OggReadError) -> Error {
    match error {
        ogg::OggReadError::ReadError(error) => Error::Io(error),
        other => codec(other),
    }
}

fn validate_tags(mut data: &[u8]) -> Result<()> {
    fn take_u32(data: &mut &[u8]) -> Result<usize> {
        let word = data.get(..4).ok_or_else(|| codec("truncated OpusTags"))?;
        let value = u32::from_le_bytes([word[0], word[1], word[2], word[3]]);
        *data = &data[4..];
        usize::try_from(value).map_err(codec)
    }
    let vendor_len = take_u32(&mut data)?;
    data = data
        .get(vendor_len..)
        .ok_or_else(|| codec("truncated OpusTags vendor"))?;
    let comments = take_u32(&mut data)?;
    if comments > data.len() / 4 {
        return Err(codec("truncated OpusTags comments"));
    }
    for _ in 0..comments {
        let length = take_u32(&mut data)?;
        data = data
            .get(length..)
            .ok_or_else(|| codec("truncated OpusTags comment"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_roundtrips_and_granules_include_encoder_delay() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("short.ogg");
        for len in [0usize, 1, 103, 104, 105, 319, 320, 321, 640, 16_017] {
            let input: Vec<f32> = (0..len)
                .map(|index| (index as f32 * 0.2).sin() * 0.5)
                .collect();
            write_ogg_opus(&path, &input)?;
            let decoded = read_ogg_opus_16k(&path)?;
            assert_eq!(decoded.len(), len);
            assert!(decoded.iter().all(|sample| sample.is_finite()));
            let mut reader = PacketReader::new(File::open(&path)?);
            let head = reader.read_packet_expected().map_err(ogg_error)?;
            let skip = u16::from_le_bytes([head.data[10], head.data[11]]);
            let mut encoder =
                Encoder::new(16_000, Channels::Mono, Application::Voip).map_err(codec)?;
            assert_eq!(i32::from(skip), encoder.get_lookahead().map_err(codec)? * 3);
            let mut total = 0;
            while let Some(packet) = reader.read_packet().map_err(ogg_error)? {
                if packet.data.starts_with(b"OpusTags") {
                    continue;
                }
                total += OPUS_FRAME_SAMPLES * 3;
                if packet.last_in_stream() {
                    assert_eq!(packet.absgp_page(), u64::from(skip) + len as u64 * 3);
                    assert!(packet.absgp_page() <= total as u64);
                } else if packet.last_in_page() {
                    assert_eq!(packet.absgp_page(), total as u64);
                }
            }
        }
        Ok(())
    }

    #[test]
    fn failed_atomic_write_cleans_up_and_does_not_truncate_existing_part() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("occupied.ogg");
        std::fs::create_dir(&path)?;
        assert!(matches!(
            write_ogg_opus(&path, &[0.0; 320]),
            Err(Error::Io(_))
        ));
        let part = directory.path().join("occupied.ogg.part");
        assert!(!part.exists());
        std::fs::write(&part, b"existing writer")?;
        assert!(write_ogg_opus(&path, &[]).is_err());
        assert_eq!(std::fs::read(&part)?, b"existing writer");
        Ok(())
    }

    #[test]
    fn malformed_files_and_nonfinite_audio_are_handled() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("bad.ogg");
        for data in [&b""[..], &b"OggS"[..], &b"not an Ogg file"[..]] {
            std::fs::write(&path, data)?;
            assert!(read_ogg_opus_16k(&path).is_err());
        }
        write_ogg_opus(&path, &[f32::NAN; 321])?;
        assert_eq!(peaks(&[f32::NAN; 321]), [0, 0]);
        let decoded = read_ogg_opus_16k(&path)?;
        assert!(
            decoded
                .iter()
                .all(|sample| sample.is_finite() && sample.abs() < 0.01)
        );
        let bytes = std::fs::read(&path)?;
        std::fs::write(&path, &bytes[..bytes.len() - 1])?;
        assert!(read_ogg_opus_16k(&path).is_err());
        assert!(validate_tags(&u32::MAX.to_le_bytes()).is_err());
        Ok(())
    }
}
