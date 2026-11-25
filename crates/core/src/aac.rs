//! AAC-LC in ADTS frames, from the operating system's encoder (no patent-encumbered code
//! is shipped with djcast). Windows: the Media Foundation AAC encoder. macOS: AudioToolbox's
//! AudioConverter. Other platforms: not available.

use crate::encoder::AudioEncoder;
use anyhow::{Result, bail};

/// Bitrates the Windows AAC encoder accepts (macOS accepts them too).
pub const BITRATES: [u32; 4] = [96, 128, 160, 192];

pub fn create(bitrate_kbps: u32, sample_rate: u32) -> Result<Box<dyn AudioEncoder>> {
    if !BITRATES.contains(&bitrate_kbps) {
        bail!("unsupported AAC bitrate {bitrate_kbps} kbps (use 96, 128, 160 or 192)");
    }
    #[cfg(windows)]
    {
        Ok(Box::new(mf::MfAacEncoder::new(bitrate_kbps, sample_rate)?))
    }
    #[cfg(target_os = "macos")]
    {
        Ok(Box::new(at::AtAacEncoder::new(bitrate_kbps, sample_rate)?))
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = sample_rate;
        bail!("AAC is only available on Windows and macOS; choose MP3 or Opus")
    }
}

/// The encoder takes 44.1 or 48 kHz; anything else is resampled to 48 kHz.
#[cfg(any(windows, target_os = "macos"))]
fn encoder_rate(sample_rate: u32) -> Result<(u32, Option<crate::ogg_opus::StereoResampler>)> {
    Ok(match sample_rate {
        44_100 | 48_000 => (sample_rate, None),
        other => (48_000, Some(crate::ogg_opus::StereoResampler::new(other, 48_000)?)),
    })
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
            let (rate, resampler) = encoder_rate(sample_rate)?;
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
}

#[cfg(target_os = "macos")]
mod at {
    //! AudioToolbox's AudioConverter: interleaved f32 PCM in, raw AAC packets out, each
    //! wrapped in an ADTS header here so the stream is the same as on Windows.

    use super::*;
    use crate::ogg_opus::StereoResampler;
    use std::ffi::c_void;

    type OSStatus = i32;
    type AudioConverterRef = *mut c_void;

    #[repr(C)]
    struct AudioStreamBasicDescription {
        sample_rate: f64,
        format_id: u32,
        format_flags: u32,
        bytes_per_packet: u32,
        frames_per_packet: u32,
        bytes_per_frame: u32,
        channels_per_frame: u32,
        bits_per_channel: u32,
        reserved: u32,
    }

    #[repr(C)]
    struct AudioBuffer {
        number_channels: u32,
        data_byte_size: u32,
        data: *mut c_void,
    }

    #[repr(C)]
    struct AudioBufferList {
        number_buffers: u32,
        buffers: [AudioBuffer; 1],
    }

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct AudioStreamPacketDescription {
        start_offset: i64,
        variable_frames_in_packet: u32,
        data_byte_size: u32,
    }

    type InputProc = extern "C" fn(
        converter: AudioConverterRef,
        io_packets: *mut u32,
        io_data: *mut AudioBufferList,
        out_descriptions: *mut *mut AudioStreamPacketDescription,
        user_data: *mut c_void,
    ) -> OSStatus;

    #[link(name = "AudioToolbox", kind = "framework")]
    unsafe extern "C" {
        fn AudioConverterNew(
            source: *const AudioStreamBasicDescription,
            destination: *const AudioStreamBasicDescription,
            converter: *mut AudioConverterRef,
        ) -> OSStatus;
        fn AudioConverterDispose(converter: AudioConverterRef) -> OSStatus;
        fn AudioConverterSetProperty(converter: AudioConverterRef, id: u32, size: u32, data: *const c_void) -> OSStatus;
        fn AudioConverterGetProperty(converter: AudioConverterRef, id: u32, size: *mut u32, data: *mut c_void)
        -> OSStatus;
        fn AudioConverterFillComplexBuffer(
            converter: AudioConverterRef,
            input: InputProc,
            user_data: *mut c_void,
            io_packets: *mut u32,
            output: *mut AudioBufferList,
            descriptions: *mut AudioStreamPacketDescription,
        ) -> OSStatus;
    }

    const fn fourcc(c: &[u8; 4]) -> u32 {
        u32::from_be_bytes(*c)
    }
    const FORMAT_LPCM: u32 = fourcc(b"lpcm");
    const FORMAT_AAC: u32 = fourcc(b"aac ");
    const FLAG_FLOAT_PACKED: u32 = 1 | 8; // kAudioFormatFlagIsFloat | kAudioFormatFlagIsPacked
    const PROP_ENCODE_BITRATE: u32 = fourcc(b"brat");
    /// kAudioCodecPropertyBitRateControlMode; 0 = constant bitrate, as listeners and the
    /// server's stats expect (the default saves bits on simple material).
    const PROP_BITRATE_CONTROL_MODE: u32 = fourcc(b"acbf");
    const BITRATE_CONSTANT: u32 = 0;
    const PROP_MAX_OUTPUT_PACKET_SIZE: u32 = fourcc(b"xops");
    /// Returned by our input callback when all queued PCM was handed over: the converter
    /// keeps its state and returns what it could encode.
    const NO_MORE_INPUT: OSStatus = fourcc(b"nomo") as OSStatus;
    /// AAC packets asked for per call.
    const PACKETS_PER_CALL: u32 = 16;
    const FRAMES_PER_PACKET: u32 = 1024;

    /// PCM waiting for the converter, shared with the input callback.
    struct Feed {
        /// Samples queued by `encode`, not yet handed to the converter.
        queued: Vec<f32>,
        /// Samples handed over in the last callback: the converter may read them until it
        /// calls back again, so they stay alive here until then.
        handed: Vec<f32>,
        end_of_stream: bool,
    }

    extern "C" fn supply(
        _: AudioConverterRef,
        io_packets: *mut u32,
        io_data: *mut AudioBufferList,
        _: *mut *mut AudioStreamPacketDescription,
        user_data: *mut c_void,
    ) -> OSStatus {
        let feed = unsafe { &mut *(user_data as *mut Feed) };
        let data = unsafe { &mut *io_data };
        std::mem::swap(&mut feed.handed, &mut feed.queued);
        feed.queued.clear();
        let frames = (feed.handed.len() / 2) as u32;
        data.number_buffers = 1;
        data.buffers[0] = AudioBuffer {
            number_channels: 2,
            data_byte_size: frames * 8,
            data: feed.handed.as_mut_ptr() as *mut c_void,
        };
        unsafe { *io_packets = frames };
        if frames == 0 && !feed.end_of_stream { NO_MORE_INPUT } else { 0 }
    }

    pub struct AtAacEncoder {
        converter: AudioConverterRef,
        feed: Box<Feed>,
        resampler: Option<StereoResampler>,
        resampled: Vec<f32>,
        max_packet: usize,
        out_buf: Vec<u8>,
        descriptions: Vec<AudioStreamPacketDescription>,
        /// ADTS sampling frequency index (3 = 48 kHz, 4 = 44.1 kHz).
        freq_index: u8,
    }

    // The converter is only used by the thread that owns the encoder.
    unsafe impl Send for AtAacEncoder {}

    fn check(status: OSStatus, what: &str) -> Result<()> {
        if status != 0 {
            bail!("{what} failed (OSStatus {status})");
        }
        Ok(())
    }

    impl AtAacEncoder {
        pub fn new(bitrate_kbps: u32, sample_rate: u32) -> Result<Self> {
            let (rate, resampler) = encoder_rate(sample_rate)?;
            let pcm = AudioStreamBasicDescription {
                sample_rate: rate as f64,
                format_id: FORMAT_LPCM,
                format_flags: FLAG_FLOAT_PACKED,
                bytes_per_packet: 8,
                frames_per_packet: 1,
                bytes_per_frame: 8,
                channels_per_frame: 2,
                bits_per_channel: 32,
                reserved: 0,
            };
            let aac = AudioStreamBasicDescription {
                sample_rate: rate as f64,
                format_id: FORMAT_AAC,
                format_flags: 0,
                bytes_per_packet: 0,
                frames_per_packet: FRAMES_PER_PACKET,
                bytes_per_frame: 0,
                channels_per_frame: 2,
                bits_per_channel: 0,
                reserved: 0,
            };
            let mut converter: AudioConverterRef = std::ptr::null_mut();
            check(unsafe { AudioConverterNew(&pcm, &aac, &mut converter) }, "creating the AAC encoder")?;
            let mut enc = Self {
                converter,
                feed: Box::new(Feed { queued: Vec::new(), handed: Vec::new(), end_of_stream: false }),
                resampler,
                resampled: Vec::new(),
                max_packet: 0,
                out_buf: Vec::new(),
                descriptions: vec![AudioStreamPacketDescription::default(); PACKETS_PER_CALL as usize],
                freq_index: if rate == 44_100 { 4 } else { 3 },
            };
            check(
                unsafe {
                    AudioConverterSetProperty(
                        converter,
                        PROP_BITRATE_CONTROL_MODE,
                        4,
                        &BITRATE_CONSTANT as *const u32 as *const c_void,
                    )
                },
                "setting constant AAC bitrate",
            )?;
            let bps = bitrate_kbps * 1000;
            check(
                unsafe { AudioConverterSetProperty(converter, PROP_ENCODE_BITRATE, 4, &bps as *const u32 as *const c_void) },
                "setting the AAC bitrate",
            )?;
            let mut max = 0u32;
            let mut size = 4u32;
            check(
                unsafe {
                    AudioConverterGetProperty(converter, PROP_MAX_OUTPUT_PACKET_SIZE, &mut size, &mut max as *mut u32 as *mut c_void)
                },
                "reading the AAC packet size",
            )?;
            enc.max_packet = (max as usize).max(1536);
            enc.out_buf = vec![0; enc.max_packet * PACKETS_PER_CALL as usize];
            Ok(enc)
        }

        /// Run the converter until it wants more input; append ADTS frames to `out`.
        fn pump(&mut self, out: &mut Vec<u8>) -> Result<()> {
            loop {
                let mut packets = PACKETS_PER_CALL;
                let mut list = AudioBufferList {
                    number_buffers: 1,
                    buffers: [AudioBuffer {
                        number_channels: 2,
                        data_byte_size: self.out_buf.len() as u32,
                        data: self.out_buf.as_mut_ptr() as *mut c_void,
                    }],
                };
                let status = unsafe {
                    AudioConverterFillComplexBuffer(
                        self.converter,
                        supply,
                        &mut *self.feed as *mut Feed as *mut c_void,
                        &mut packets,
                        &mut list,
                        self.descriptions.as_mut_ptr(),
                    )
                };
                if status != 0 && status != NO_MORE_INPUT {
                    bail!("AAC encoder failed (OSStatus {status})");
                }
                for d in &self.descriptions[..packets as usize] {
                    let start = d.start_offset as usize;
                    let payload = &self.out_buf[start..start + d.data_byte_size as usize];
                    out.extend_from_slice(&adts_header(payload.len(), self.freq_index));
                    out.extend_from_slice(payload);
                }
                if packets == 0 || status == NO_MORE_INPUT {
                    return Ok(());
                }
            }
        }

        fn feed(&mut self, pcm: &[f32]) -> Result<Vec<u8>> {
            self.feed.queued.extend_from_slice(pcm);
            let mut out = Vec::new();
            self.pump(&mut out)?;
            Ok(out)
        }
    }

    impl Drop for AtAacEncoder {
        fn drop(&mut self) {
            unsafe { AudioConverterDispose(self.converter) };
        }
    }

    impl AudioEncoder for AtAacEncoder {
        fn content_type(&self) -> &'static str {
            "audio/aac"
        }

        fn extension(&self) -> &'static str {
            "aac"
        }

        fn encode(&mut self, pcm: &[f32]) -> Result<Vec<u8>> {
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
            self.feed.end_of_stream = true;
            let mut out = Vec::new();
            self.pump(&mut out)?;
            Ok(out)
        }
    }
}

/// 7-byte ADTS header for one AAC-LC stereo frame (no CRC).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn adts_header(payload_len: usize, freq_index: u8) -> [u8; 7] {
    let len = payload_len + 7;
    const PROFILE_LC: u8 = 1; // audio object type 2 (AAC LC) minus 1
    const CHANNELS: u8 = 2;
    [
        0xFF,
        0xF1, // sync, MPEG-4, layer 0, no CRC
        (PROFILE_LC << 6) | (freq_index << 2) | (CHANNELS >> 2),
        ((CHANNELS & 3) << 6) | ((len >> 11) & 3) as u8,
        ((len >> 3) & 0xFF) as u8,
        (((len & 7) << 5) as u8) | 0x1F, // buffer fullness 0x7FF (variable bitrate)
        0xFC,
    ]
}

#[cfg(all(test, any(windows, target_os = "macos")))]
mod tests {
    use super::*;

    fn sine(block: usize, rate: usize) -> Vec<f32> {
        let n = rate / 100;
        (0..n)
            .flat_map(|i| {
                let s = ((block * n + i) as f32 * 440.0 * std::f32::consts::TAU / rate as f32).sin() * 0.5;
                [s, s]
            })
            .collect()
    }

    /// Walk ADTS frames by their length field; returns the number of frames.
    fn count_frames(bytes: &[u8]) -> usize {
        let (mut pos, mut frames) = (0, 0);
        while pos + 7 <= bytes.len() {
            assert_eq!(bytes[pos], 0xFF, "lost ADTS sync at {pos}");
            assert_eq!(bytes[pos + 1] & 0xF6, 0xF0);
            let len = ((bytes[pos + 3] as usize & 3) << 11) | ((bytes[pos + 4] as usize) << 3) | (bytes[pos + 5] as usize >> 5);
            pos += len;
            frames += 1;
        }
        assert_eq!(pos, bytes.len());
        frames
    }

    #[test]
    fn encodes_a_sine_into_adts_frames() {
        let mut enc = create(128, 44_100).unwrap();
        let mut bytes = Vec::new();
        for block in 0..100 {
            bytes.extend(enc.encode(&sine(block, 44_100)).unwrap());
        }
        bytes.extend(enc.flush().unwrap());
        // 1 s at 128 kbps ≈ 16 KB of ADTS frames.
        assert!((12_000..=22_000).contains(&bytes.len()), "got {} bytes", bytes.len());
        let frames = count_frames(&bytes);
        assert!(frames >= 40, "{frames} frames"); // 1024 samples per frame → ~43 per second
    }

    #[test]
    fn other_rates_are_resampled() {
        let mut enc = create(96, 32_000).unwrap();
        let mut bytes = Vec::new();
        for block in 0..100 {
            bytes.extend(enc.encode(&sine(block, 32_000)).unwrap());
        }
        bytes.extend(enc.flush().unwrap());
        assert!(count_frames(&bytes) >= 40);
    }

    #[test]
    fn adts_header_fields() {
        let h = adts_header(300, 4);
        assert_eq!(&h[..2], &[0xFF, 0xF1]);
        assert_eq!(h[2] >> 6, 1); // LC
        assert_eq!((h[2] >> 2) & 0xF, 4); // 44.1 kHz
        let len = ((h[3] as usize & 3) << 11) | ((h[4] as usize) << 3) | (h[5] as usize >> 5);
        assert_eq!(len, 307);
    }
}
