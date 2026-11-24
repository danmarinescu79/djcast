//! Opus in Ogg (RFC 7845), resampled to 48 kHz.
//!
//! Stream titles travel inside the stream: a title change ends the current logical
//! stream and starts a new one whose `OpusTags` carry the title ("chained Ogg"), which is
//! how Icecast and players learn the new title for Ogg streams.

use crate::encoder::AudioEncoder;
use anyhow::{Result, anyhow};
use ogg::writing::{PacketWriteEndInfo, PacketWriter};
use rubato::audioadapter_buffers::direct::InterleavedSlice;
use rubato::{Fft, FixedSync, Resampler};

const RATE: u32 = 48_000;
/// 20 ms frames.
const FRAME: usize = 960;
/// Packets per Ogg page (100 ms): keeps the page overhead around 2 kbps.
const PACKETS_PER_PAGE: u32 = 5;
const VENDOR: &str = concat!("djcast ", env!("CARGO_PKG_VERSION"));

pub struct OggOpusEncoder {
    enc: opus::Encoder,
    resampler: Option<StereoResampler>,
    /// 48 kHz interleaved samples not yet encoded (less than one frame).
    pending: Vec<f32>,
    writer: PacketWriter<'static, Vec<u8>>,
    serial: u32,
    /// Encoded but not yet written packet; held back so it can be marked end-of-stream.
    held: Option<Vec<u8>>,
    /// Packets written in the current logical stream.
    packets: u64,
    pre_skip: u16,
    title: Option<String>,
    headers_written: bool,
}

impl OggOpusEncoder {
    pub fn new(bitrate_kbps: u32, sample_rate: u32) -> Result<Self> {
        if !(6..=510).contains(&bitrate_kbps) {
            anyhow::bail!("unsupported Opus bitrate {bitrate_kbps} kbps");
        }
        let mut enc = opus::Encoder::new(RATE, opus::Channels::Stereo, opus::Application::Audio)
            .map_err(|e| anyhow!("opus init: {e}"))?;
        enc.set_bitrate(opus::Bitrate::Bits(bitrate_kbps as i32 * 1000))
            .map_err(|e| anyhow!("opus bitrate: {e}"))?;
        let pre_skip = enc.get_lookahead().map_err(|e| anyhow!("opus lookahead: {e}"))? as u16;
        let resampler = if sample_rate == RATE { None } else { Some(StereoResampler::new(sample_rate, RATE)?) };
        Ok(Self {
            enc,
            resampler,
            pending: Vec::with_capacity(FRAME * 4),
            writer: PacketWriter::new(Vec::new()),
            serial: new_serial(),
            held: None,
            packets: 0,
            pre_skip,
            title: None,
            headers_written: false,
        })
    }

    fn write_headers(&mut self) -> Result<()> {
        let mut head = Vec::with_capacity(19);
        head.extend_from_slice(b"OpusHead");
        head.push(1); // version
        head.push(2); // channels
        head.extend_from_slice(&self.pre_skip.to_le_bytes());
        head.extend_from_slice(&RATE.to_le_bytes()); // original input rate (informational)
        head.extend_from_slice(&0i16.to_le_bytes()); // output gain
        head.push(0); // channel mapping family
        self.writer.write_packet(head, self.serial, PacketWriteEndInfo::EndPage, 0)?;
        let tags = opus_tags(self.title.as_deref());
        self.writer.write_packet(tags, self.serial, PacketWriteEndInfo::EndPage, 0)?;
        self.headers_written = true;
        Ok(())
    }

    /// Write the held packet; `end` marks it as the last of the logical stream.
    fn write_held(&mut self, end: bool) -> Result<()> {
        let Some(pkt) = self.held.take() else { return Ok(()) };
        self.packets += 1;
        let granule = self.packets * FRAME as u64;
        let info = if end {
            PacketWriteEndInfo::EndStream
        } else if self.packets.is_multiple_of(PACKETS_PER_PAGE as u64) {
            PacketWriteEndInfo::EndPage
        } else {
            PacketWriteEndInfo::NormalPacket
        };
        self.writer.write_packet(pkt, self.serial, info, granule)?;
        Ok(())
    }

    fn encode_frames(&mut self) -> Result<()> {
        let mut buf = [0u8; 4000];
        let mut start = 0;
        while self.pending.len() - start >= FRAME * 2 {
            if !self.headers_written {
                self.write_headers()?;
            }
            let n = self
                .enc
                .encode_float(&self.pending[start..start + FRAME * 2], &mut buf)
                .map_err(|e| anyhow!("opus encode: {e}"))?;
            start += FRAME * 2;
            self.write_held(false)?;
            self.held = Some(buf[..n].to_vec());
        }
        self.pending.drain(..start);
        Ok(())
    }

    /// End the current logical stream; the next encoded audio begins a new one whose
    /// headers carry the current title.
    fn chain(&mut self) -> Result<()> {
        if self.headers_written {
            self.write_held(true)?;
        }
        self.enc.reset_state().map_err(|e| anyhow!("opus reset: {e}"))?;
        self.serial = new_serial();
        self.packets = 0;
        self.headers_written = false;
        Ok(())
    }

    fn take_output(&mut self) -> Vec<u8> {
        std::mem::take(self.writer.inner_mut())
    }
}

impl AudioEncoder for OggOpusEncoder {
    fn content_type(&self) -> &'static str {
        "audio/ogg"
    }

    fn extension(&self) -> &'static str {
        "opus"
    }

    fn in_band_metadata(&self) -> bool {
        true
    }

    fn encode(&mut self, pcm: &[f32]) -> Result<Vec<u8>> {
        match self.resampler.as_mut() {
            Some(r) => r.process(pcm, &mut self.pending)?,
            None => self.pending.extend_from_slice(pcm),
        }
        self.encode_frames()?;
        Ok(self.take_output())
    }

    fn set_title(&mut self, title: &str) -> Result<Vec<u8>> {
        self.title = Some(title.to_string());
        self.chain()?;
        Ok(self.take_output())
    }

    fn restart(&mut self) -> Result<Vec<u8>> {
        self.chain()?;
        Ok(self.take_output())
    }

    fn flush(&mut self) -> Result<Vec<u8>> {
        if !self.pending.is_empty() {
            // Pad the last partial frame with silence.
            let pad = FRAME * 2 - self.pending.len() % (FRAME * 2);
            self.pending.extend(std::iter::repeat_n(0.0, pad));
            self.encode_frames()?;
        }
        if self.headers_written {
            self.write_held(true)?;
        }
        Ok(self.take_output())
    }
}

fn opus_tags(title: Option<&str>) -> Vec<u8> {
    let comments: Vec<String> = title.map(|t| format!("TITLE={t}")).into_iter().collect();
    let mut v = Vec::with_capacity(64);
    v.extend_from_slice(b"OpusTags");
    v.extend_from_slice(&(VENDOR.len() as u32).to_le_bytes());
    v.extend_from_slice(VENDOR.as_bytes());
    v.extend_from_slice(&(comments.len() as u32).to_le_bytes());
    for c in comments {
        v.extend_from_slice(&(c.len() as u32).to_le_bytes());
        v.extend_from_slice(c.as_bytes());
    }
    v
}

fn new_serial() -> u32 {
    use std::sync::atomic::{AtomicU32, Ordering};
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    seed.wrapping_add(NEXT.fetch_add(0x9E37_79B9, Ordering::Relaxed))
}

/// Stereo sample-rate converter (interleaved in, interleaved out).
pub(crate) struct StereoResampler {
    fft: Fft<f32>,
    input: Vec<f32>,
}

impl StereoResampler {
    pub(crate) fn new(rate_in: u32, rate_out: u32) -> Result<Self> {
        // ~10 ms chunks keep the added latency small.
        let chunk = (rate_in as usize / 100).max(64);
        let fft = Fft::new(rate_in as usize, rate_out as usize, chunk, 2, FixedSync::Input)
            .map_err(|e| anyhow!("resampler {rate_in} → {rate_out} Hz: {e}"))?;
        Ok(Self { fft, input: Vec::with_capacity(chunk * 4) })
    }

    pub(crate) fn process(&mut self, pcm: &[f32], out: &mut Vec<f32>) -> Result<()> {
        self.input.extend_from_slice(pcm);
        let mut start = 0;
        loop {
            let need = self.fft.input_frames_next();
            if (self.input.len() - start) / 2 < need {
                break;
            }
            let chunk = &self.input[start..start + need * 2];
            let adapter = InterleavedSlice::new(chunk, 2, need).map_err(|e| anyhow!("resampler input: {e}"))?;
            let res = self.fft.process(&adapter, None).map_err(|e| anyhow!("resample: {e}"))?;
            out.extend_from_slice(&res.take_data());
            start += need * 2;
        }
        self.input.drain(..start);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(rate: u32, secs: f32) -> Vec<f32> {
        (0..(rate as f32 * secs) as usize)
            .flat_map(|i| {
                let s = (i as f32 * 440.0 * std::f32::consts::TAU / rate as f32).sin() * 0.5;
                [s, s]
            })
            .collect()
    }

    /// Decode every Opus packet of every logical stream; returns (titles, decoded 48 kHz frames).
    fn decode(bytes: &[u8]) -> (Vec<Option<String>>, usize) {
        let mut reader = ogg::reading::PacketReader::new(std::io::Cursor::new(bytes.to_vec()));
        let mut dec = opus::Decoder::new(RATE, opus::Channels::Stereo).unwrap();
        let mut titles = Vec::new();
        let mut frames = 0;
        let mut out = vec![0f32; 5760 * 2];
        while let Some(p) = reader.read_packet().unwrap() {
            if p.data.starts_with(b"OpusHead") {
                assert!(p.first_in_stream());
                dec = opus::Decoder::new(RATE, opus::Channels::Stereo).unwrap();
            } else if p.data.starts_with(b"OpusTags") {
                let s = String::from_utf8_lossy(&p.data);
                titles.push(s.split("TITLE=").nth(1).map(str::to_string));
            } else {
                frames += dec.decode_float(&p.data, &mut out, false).unwrap();
            }
        }
        (titles, frames)
    }

    #[test]
    fn encodes_44k1_into_ogg_opus_and_chains_titles() {
        let mut enc = OggOpusEncoder::new(128, 44_100).unwrap();
        let pcm = sine(44_100, 1.0);
        let mut bytes = enc.encode(&pcm[..pcm.len() / 2]).unwrap();
        bytes.extend(enc.set_title("Artist - Track").unwrap());
        bytes.extend(enc.encode(&pcm[pcm.len() / 2..]).unwrap());
        bytes.extend(enc.flush().unwrap());
        assert!(bytes.starts_with(b"OggS"));

        let (titles, frames) = decode(&bytes);
        assert_eq!(titles, vec![None, Some("Artist - Track".to_string())]);
        // 1 s of input → about 48 000 frames at 48 kHz (minus resampler delay, plus padding).
        assert!((45_000..=50_000).contains(&frames), "decoded {frames} frames");
    }

    #[test]
    fn restart_begins_a_new_stream_with_headers() {
        let flags = |b: &[u8]| -> Vec<u8> {
            b.windows(4).enumerate().filter(|(_, w)| *w == b"OggS").map(|(i, _)| b[i + 5]).collect()
        };
        let mut enc = OggOpusEncoder::new(96, 48_000).unwrap();
        let first = enc.encode(&sine(48_000, 0.2)).unwrap();
        assert_eq!(flags(&first)[0] & 2, 2, "first page is not beginning-of-stream");
        // restart() returns only the end of the old stream ...
        let tail = enc.restart().unwrap();
        assert!(flags(&tail).iter().all(|f| f & 2 == 0));
        assert_eq!(flags(&tail).last().map(|f| f & 4), Some(4), "no end-of-stream page");
        // ... and the next audio starts the new one.
        let next = enc.encode(&sine(48_000, 0.1)).unwrap();
        assert_eq!(flags(&next)[0] & 2, 2);
    }
}
