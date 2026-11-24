//! Audio encoders. Input is always interleaved stereo f32 PCM.

use crate::config::{Codec, EncoderConfig};
use anyhow::{Result, anyhow, bail};
use mp3lame_encoder::{Bitrate, Builder, FlushNoGap, InterleavedPcm, Quality};

pub trait AudioEncoder: Send {
    /// MIME type sent to the server (`Content-Type`).
    fn content_type(&self) -> &'static str;
    /// File extension for recordings.
    fn extension(&self) -> &'static str;
    /// Encode a chunk of interleaved stereo samples; returns encoded bytes (may be empty).
    fn encode(&mut self, pcm: &[f32]) -> Result<Vec<u8>>;
    /// Flush buffered frames at end of stream.
    fn flush(&mut self) -> Result<Vec<u8>>;

    /// Titles travel inside the stream (Ogg) instead of through the server's admin API.
    /// Such streams must start with header pages, so a new listener connection waits for
    /// a fresh logical stream (see [`restart`](Self::restart)).
    fn in_band_metadata(&self) -> bool {
        false
    }
    /// In-band title change. Returns the bytes that end the current logical stream; the
    /// next [`encode`](Self::encode) output starts a new one.
    fn set_title(&mut self, _title: &str) -> Result<Vec<u8>> {
        Ok(Vec::new())
    }
    /// Start a new logical stream (for a server that just connected). Same return as `set_title`.
    fn restart(&mut self) -> Result<Vec<u8>> {
        Ok(Vec::new())
    }
}

pub fn create(cfg: &EncoderConfig, sample_rate: u32) -> Result<Box<dyn AudioEncoder>> {
    match cfg.codec {
        Codec::Mp3 => Ok(Box::new(Mp3Encoder::new(cfg.bitrate, sample_rate)?)),
        Codec::Opus => Ok(Box::new(crate::ogg_opus::OggOpusEncoder::new(cfg.bitrate, sample_rate)?)),
        Codec::Aac => crate::aac::create(cfg.bitrate, sample_rate),
    }
}

pub struct Mp3Encoder {
    inner: mp3lame_encoder::Encoder,
}

impl Mp3Encoder {
    pub fn new(bitrate_kbps: u32, sample_rate: u32) -> Result<Self> {
        let mut b = Builder::new().ok_or_else(|| anyhow!("failed to create LAME encoder"))?;
        b.set_num_channels(2).map_err(|e| anyhow!("lame channels: {e}"))?;
        b.set_sample_rate(sample_rate).map_err(|e| anyhow!("lame sample rate {sample_rate}: {e}"))?;
        b.set_brate(mp3_bitrate(bitrate_kbps)?).map_err(|e| anyhow!("lame bitrate: {e}"))?;
        b.set_quality(Quality::Good).map_err(|e| anyhow!("lame quality: {e}"))?;
        let inner = b.build().map_err(|e| anyhow!("lame init: {e}"))?;
        Ok(Self { inner })
    }
}

impl AudioEncoder for Mp3Encoder {
    fn content_type(&self) -> &'static str {
        "audio/mpeg"
    }

    fn extension(&self) -> &'static str {
        "mp3"
    }

    fn encode(&mut self, pcm: &[f32]) -> Result<Vec<u8>> {
        let frames = pcm.len() / 2;
        let mut out = Vec::with_capacity(mp3lame_encoder::max_required_buffer_size(frames));
        self.inner
            .encode_to_vec(InterleavedPcm(pcm), &mut out)
            .map_err(|e| anyhow!("mp3 encode: {e}"))?;
        Ok(out)
    }

    fn flush(&mut self) -> Result<Vec<u8>> {
        let mut out = Vec::with_capacity(7200);
        self.inner.flush_to_vec::<FlushNoGap>(&mut out).map_err(|e| anyhow!("mp3 flush: {e}"))?;
        Ok(out)
    }
}

fn mp3_bitrate(kbps: u32) -> Result<Bitrate> {
    use Bitrate::*;
    Ok(match kbps {
        32 => Kbps32,
        40 => Kbps40,
        48 => Kbps48,
        64 => Kbps64,
        80 => Kbps80,
        96 => Kbps96,
        112 => Kbps112,
        128 => Kbps128,
        160 => Kbps160,
        192 => Kbps192,
        224 => Kbps224,
        256 => Kbps256,
        320 => Kbps320,
        other => bail!("unsupported MP3 bitrate {other} kbps"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_a_sine_into_mp3_frames() {
        let mut enc = Mp3Encoder::new(192, 48_000).unwrap();
        let pcm: Vec<f32> = (0..48_000)
            .flat_map(|i| {
                let s = (i as f32 * 440.0 * std::f32::consts::TAU / 48_000.0).sin() * 0.5;
                [s, s]
            })
            .collect();
        let mut bytes = enc.encode(&pcm).unwrap();
        bytes.extend(enc.flush().unwrap());
        // 1 s at 192 kbps ≈ 24 KB; must contain an MPEG frame sync.
        assert!(bytes.len() > 20_000, "got {} bytes", bytes.len());
        assert!(bytes.windows(2).any(|w| w[0] == 0xFF && w[1] & 0xE0 == 0xE0));
    }
}
