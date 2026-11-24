//! AAC-LC in ADTS frames, from the operating system's encoder (no patent-encumbered code
//! is shipped with djcast). Windows: the Media Foundation AAC encoder. Other platforms: not
//! yet (macOS AudioToolbox is planned).

use crate::encoder::AudioEncoder;
use anyhow::{Result, bail};

/// Bitrates the Windows AAC encoder accepts.
pub const BITRATES: [u32; 4] = [96, 128, 160, 192];

pub fn create(bitrate_kbps: u32, sample_rate: u32) -> Result<Box<dyn AudioEncoder>> {
    if !BITRATES.contains(&bitrate_kbps) {
        bail!("unsupported AAC bitrate {bitrate_kbps} kbps (use 96, 128, 160 or 192)");
    }
    #[cfg(windows)]
    {
        Ok(Box::new(mf::MfAacEncoder::new(bitrate_kbps, sample_rate)?))
    }
    #[cfg(not(windows))]
    {
        let _ = sample_rate;
        bail!("AAC is only available on Windows for now; choose MP3 or Opus")
    }
}

#[cfg(windows)]
mod mf {
    use super::*;
    use crate::ogg_opus::StereoResampler;
    use anyhow::{Context, anyhow};
    use windows::Win32::Media::MediaFoundation::*;
    use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx};

    pub struct MfAacEncoder {
        mft: IMFTransform,
        resampler: Option<StereoResampler>,
        rate: u32,
        out_size: u32,
        /// Frames sent so far (for sample timestamps).
        frames: u64,
        pcm16: Vec<u8>,
        resampled: Vec<f32>,
    }

    // The transform lives in the multithreaded apartment: it is created in a thread that
    // joined the MTA and used by the encoder thread, which joins it too.
    unsafe impl Send for MfAacEncoder {}

    fn join_mta() {
        // Already-initialized threads return S_FALSE or RPC_E_CHANGED_MODE; both are fine
        // for an in-process, free-threaded transform.
        let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    }

    impl MfAacEncoder {
        pub fn new(bitrate_kbps: u32, sample_rate: u32) -> Result<Self> {
            // The encoder takes 44.1 or 48 kHz; anything else is resampled to 48 kHz.
            let (rate, resampler) = match sample_rate {
                44_100 | 48_000 => (sample_rate, None),
                other => (48_000, Some(StereoResampler::new(other, 48_000)?)),
            };
            // Build the transform in an MTA thread so it never gets tied to a caller's STA.
            let (mft, out_size) = std::thread::scope(|s| {
                s.spawn(|| {
                    join_mta();
                    unsafe { build(bitrate_kbps, rate) }.map(|(m, n)| (SendBox(m), n))
                })
                .join()
                .map_err(|_| anyhow!("AAC encoder setup panicked"))?
            })?;
            Ok(Self { mft: mft.0, resampler, rate, out_size, frames: 0, pcm16: Vec::new(), resampled: Vec::new() })
        }

        fn feed(&mut self, pcm: &[f32]) -> Result<Vec<u8>> {
            let mut out = Vec::new();
            if pcm.is_empty() {
                return Ok(out);
            }
            self.pcm16.clear();
            self.pcm16.extend(pcm.iter().flat_map(|s| ((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes()));
            let frames = (pcm.len() / 2) as u64;
            unsafe {
                let buf = MFCreateMemoryBuffer(self.pcm16.len() as u32)?;
                let mut ptr = std::ptr::null_mut();
                buf.Lock(&mut ptr, None, None)?;
                std::ptr::copy_nonoverlapping(self.pcm16.as_ptr(), ptr, self.pcm16.len());
                buf.Unlock()?;
                buf.SetCurrentLength(self.pcm16.len() as u32)?;
                let sample = MFCreateSample()?;
                sample.AddBuffer(&buf)?;
                sample.SetSampleTime((self.frames * 10_000_000 / self.rate as u64) as i64)?;
                sample.SetSampleDuration((frames * 10_000_000 / self.rate as u64) as i64)?;
                self.frames += frames;
                if let Err(e) = self.mft.ProcessInput(0, &sample, 0) {
                    // Full: collect output first, then retry once.
                    if e.code() != MF_E_NOTACCEPTING {
                        return Err(e).context("AAC encoder input");
                    }
                    self.drain_output(&mut out)?;
                    self.mft.ProcessInput(0, &sample, 0).context("AAC encoder input")?;
                }
            }
            self.drain_output(&mut out)?;
            Ok(out)
        }

        fn drain_output(&mut self, out: &mut Vec<u8>) -> Result<()> {
            loop {
                unsafe {
                    let sample = MFCreateSample()?;
                    let buf = MFCreateMemoryBuffer(self.out_size.max(8192))?;
                    sample.AddBuffer(&buf)?;
                    let mut data = [MFT_OUTPUT_DATA_BUFFER {
                        dwStreamID: 0,
                        pSample: std::mem::ManuallyDrop::new(Some(sample)),
                        dwStatus: 0,
                        pEvents: std::mem::ManuallyDrop::new(None),
                    }];
                    let mut status = 0u32;
                    let res = self.mft.ProcessOutput(0, &mut data, &mut status);
                    let sample = std::mem::ManuallyDrop::take(&mut data[0].pSample);
                    drop(std::mem::ManuallyDrop::take(&mut data[0].pEvents));
                    match res {
                        Ok(()) => {
                            if let Some(sample) = sample {
                                let b = sample.ConvertToContiguousBuffer()?;
                                let mut ptr = std::ptr::null_mut();
                                let mut len = 0u32;
                                b.Lock(&mut ptr, None, Some(&mut len))?;
                                out.extend_from_slice(std::slice::from_raw_parts(ptr, len as usize));
                                b.Unlock()?;
                            }
                        }
                        Err(e) if e.code() == MF_E_TRANSFORM_NEED_MORE_INPUT => return Ok(()),
                        Err(e) => return Err(e).context("AAC encoder output"),
                    }
                }
            }
        }
    }

    /// Moves the transform out of the setup thread (see `unsafe impl Send` above).
    struct SendBox(IMFTransform);
    unsafe impl Send for SendBox {}

    unsafe fn build(bitrate_kbps: u32, rate: u32) -> Result<(IMFTransform, u32)> {
        unsafe {
            MFStartup(MF_VERSION, MFSTARTUP_LITE).context("Media Foundation start-up")?;
            let mft: IMFTransform = CoCreateInstance(&AACMFTEncoder, None, CLSCTX_INPROC_SERVER)
                .context("the Windows AAC encoder is not available (Windows N editions need the Media Feature Pack)")?;

            let out = MFCreateMediaType()?;
            out.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio)?;
            out.SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_AAC)?;
            out.SetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE, 16)?;
            out.SetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND, rate)?;
            out.SetUINT32(&MF_MT_AUDIO_NUM_CHANNELS, 2)?;
            out.SetUINT32(&MF_MT_AUDIO_AVG_BYTES_PER_SECOND, bitrate_kbps * 1000 / 8)?;
            out.SetUINT32(&MF_MT_AAC_PAYLOAD_TYPE, 1)?; // ADTS
            mft.SetOutputType(0, &out, 0).context("AAC output format")?;

            let inp = MFCreateMediaType()?;
            inp.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio)?;
            inp.SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_PCM)?;
            inp.SetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE, 16)?;
            inp.SetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND, rate)?;
            inp.SetUINT32(&MF_MT_AUDIO_NUM_CHANNELS, 2)?;
            inp.SetUINT32(&MF_MT_AUDIO_BLOCK_ALIGNMENT, 4)?;
            inp.SetUINT32(&MF_MT_AUDIO_AVG_BYTES_PER_SECOND, rate * 4)?;
            mft.SetInputType(0, &inp, 0).context("AAC input format")?;

            mft.ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0)?;
            mft.ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0)?;
            let info = mft.GetOutputStreamInfo(0)?;
            Ok((mft, info.cbSize))
        }
    }

    impl AudioEncoder for MfAacEncoder {
        fn content_type(&self) -> &'static str {
            "audio/aac"
        }

        fn extension(&self) -> &'static str {
            "aac"
        }

        fn encode(&mut self, pcm: &[f32]) -> Result<Vec<u8>> {
            join_mta();
            match self.resampler.as_mut() {
                Some(r) => {
                    let mut resampled = std::mem::take(&mut self.resampled);
                    resampled.clear();
                    r.process(pcm, &mut resampled)?;
                    let out = self.feed(&resampled);
                    self.resampled = resampled;
                    out
                }
                None => self.feed(pcm),
            }
        }

        fn flush(&mut self) -> Result<Vec<u8>> {
            join_mta();
            let mut out = Vec::new();
            unsafe {
                self.mft.ProcessMessage(MFT_MESSAGE_NOTIFY_END_OF_STREAM, 0)?;
                self.mft.ProcessMessage(MFT_MESSAGE_COMMAND_DRAIN, 0)?;
            }
            self.drain_output(&mut out)?;
            Ok(out)
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn encodes_a_sine_into_adts_frames() {
            let mut enc = MfAacEncoder::new(128, 44_100).unwrap();
            let mut bytes = Vec::new();
            for block in 0..100 {
                let pcm: Vec<f32> = (0..441)
                    .flat_map(|i| {
                        let n = block * 441 + i;
                        let s = (n as f32 * 440.0 * std::f32::consts::TAU / 44_100.0).sin() * 0.5;
                        [s, s]
                    })
                    .collect();
                bytes.extend(enc.encode(&pcm).unwrap());
            }
            bytes.extend(enc.flush().unwrap());
            // 1 s at 128 kbps ≈ 16 KB of ADTS frames (sync word 0xFFF, layer 0).
            assert!((12_000..=22_000).contains(&bytes.len()), "got {} bytes", bytes.len());
            assert_eq!(bytes[0], 0xFF);
            assert_eq!(bytes[1] & 0xF6, 0xF0);
            // Walk the frames by their length field.
            let (mut pos, mut frames) = (0, 0);
            while pos + 7 <= bytes.len() {
                assert_eq!(bytes[pos], 0xFF, "lost ADTS sync at {pos}");
                let len = ((bytes[pos + 3] as usize & 3) << 11) | ((bytes[pos + 4] as usize) << 3) | (bytes[pos + 5] as usize >> 5);
                pos += len;
                frames += 1;
            }
            assert_eq!(pos, bytes.len());
            assert!(frames >= 40, "{frames} frames"); // 1024 samples per frame → ~43 per second
        }
    }
}
