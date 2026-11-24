//! Capture the audio of one program (and its child processes) only — Windows 10 2004+
//! "process loopback". Other sounds on the computer (browser, notifications) stay out of
//! the stream. Like any loopback this sees only audio that goes through Windows: it works
//! when the DJ software plays through WASAPI / Windows audio, not through ASIO.

use anyhow::{Context, Result, anyhow, bail};
use crossbeam_channel::Sender;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;
use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
use windows::Win32::Media::Audio::*;
use windows::Win32::System::Com::StructuredStorage::{PROPVARIANT, PROPVARIANT_0, PROPVARIANT_0_0, PROPVARIANT_0_0_0};
use windows::Win32::System::Com::{BLOB, COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject};
use windows::Win32::System::Variant::VT_BLOB;
use windows::core::{Interface, implement};

/// The capture format we ask Windows for (it converts from whatever the program plays).
pub const RATE: u32 = 48_000;
const CHANNELS: usize = 2;

/// A running process capture; dropping it stops the capture thread.
pub struct ProcessCapture {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    pub process_name: String,
}

impl Drop for ProcessCapture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// Start capturing the program whose executable name contains `wanted` (e.g. "rekordbox").
/// `on_audio` receives interleaved stereo f32 at [`RATE`].
pub fn start(wanted: &str, on_audio: impl FnMut(&[f32]) + Send + 'static) -> Result<ProcessCapture> {
    let (pid, process_name) = find_process(wanted)?;
    let stop = Arc::new(AtomicBool::new(false));
    let (ready_tx, ready_rx) = crossbeam_channel::bounded::<Result<()>>(1);
    let thread = {
        let stop = stop.clone();
        std::thread::Builder::new().name("process-capture".into()).spawn(move || {
            unsafe {
                let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            }
            if let Err(e) = run(pid, &stop, &ready_tx, on_audio) {
                let _ = ready_tx.try_send(Err(e));
            }
            unsafe { CoUninitialize() };
        })?
    };
    match ready_rx.recv_timeout(Duration::from_secs(10)) {
        Ok(Ok(())) => {
            log::info!("capture: audio of {process_name} (pid {pid}), {RATE} Hz stereo");
            Ok(ProcessCapture { stop, thread: Some(thread), process_name })
        }
        Ok(Err(e)) => {
            let _ = thread.join();
            Err(e)
        }
        Err(_) => {
            stop.store(true, Ordering::SeqCst);
            bail!("Windows did not start the program capture in time")
        }
    }
}

/// First running process whose executable name contains `wanted` (case-insensitive).
fn find_process(wanted: &str) -> Result<(u32, String)> {
    let needle = wanted.trim().to_lowercase();
    if needle.is_empty() {
        bail!("no program name given");
    }
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0).context("listing processes")?;
        let mut entry = PROCESSENTRY32W { dwSize: size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        let mut found = None;
        let mut ok = Process32FirstW(snap, &mut entry).is_ok();
        while ok {
            let len = entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(entry.szExeFile.len());
            let name = String::from_utf16_lossy(&entry.szExeFile[..len]);
            if name.to_lowercase().contains(&needle) {
                found = Some((entry.th32ProcessID, name));
                break;
            }
            ok = Process32NextW(snap, &mut entry).is_ok();
        }
        let _ = CloseHandle(snap);
        found.ok_or_else(|| anyhow!("\"{wanted}\" is not running — start it before going live"))
    }
}

#[implement(IActivateAudioInterfaceCompletionHandler)]
struct Activated(crossbeam_channel::Sender<()>);

impl IActivateAudioInterfaceCompletionHandler_Impl for Activated_Impl {
    fn ActivateCompleted(&self, _op: windows::core::Ref<IActivateAudioInterfaceAsyncOperation>) -> windows::core::Result<()> {
        let _ = self.0.try_send(());
        Ok(())
    }
}

fn activate(pid: u32) -> Result<IAudioClient> {
    let mut params = AUDIOCLIENT_ACTIVATION_PARAMS {
        ActivationType: AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK,
        Anonymous: AUDIOCLIENT_ACTIVATION_PARAMS_0 {
            ProcessLoopbackParams: AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS {
                TargetProcessId: pid,
                ProcessLoopbackMode: PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE,
            },
        },
    };
    // Points at `params` on the stack: never let PropVariantClear (PROPVARIANT's Drop) free it.
    let prop = std::mem::ManuallyDrop::new(PROPVARIANT {
        Anonymous: PROPVARIANT_0 {
            Anonymous: std::mem::ManuallyDrop::new(PROPVARIANT_0_0 {
                vt: VT_BLOB,
                Anonymous: PROPVARIANT_0_0_0 {
                    blob: BLOB {
                        cbSize: size_of::<AUDIOCLIENT_ACTIVATION_PARAMS>() as u32,
                        pBlobData: &mut params as *mut _ as *mut u8,
                    },
                },
                ..Default::default()
            }),
        },
    });
    let (done_tx, done_rx) = crossbeam_channel::bounded(1);
    let handler: IActivateAudioInterfaceCompletionHandler = Activated(done_tx).into();
    unsafe {
        let op = ActivateAudioInterfaceAsync(VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK, &IAudioClient::IID, Some(&*prop), &handler)
            .context("starting the program capture (needs Windows 10 version 2004 or newer)")?;
        done_rx.recv_timeout(Duration::from_secs(5)).map_err(|_| anyhow!("Windows did not answer the capture request"))?;
        let mut hr = windows::core::HRESULT(0);
        let mut unk = None;
        op.GetActivateResult(&mut hr, &mut unk)?;
        hr.ok().context("Windows refused the program capture")?;
        unk.ok_or_else(|| anyhow!("no audio client"))?.cast::<IAudioClient>().map_err(Into::into)
    }
}

fn run(pid: u32, stop: &AtomicBool, ready: &Sender<Result<()>>, mut on_audio: impl FnMut(&[f32])) -> Result<()> {
    let client = activate(pid)?;
    let format = WAVEFORMATEX {
        wFormatTag: 3, // WAVE_FORMAT_IEEE_FLOAT
        nChannels: CHANNELS as u16,
        nSamplesPerSec: RATE,
        nAvgBytesPerSec: RATE * (CHANNELS * 4) as u32,
        nBlockAlign: (CHANNELS * 4) as u16,
        wBitsPerSample: 32,
        cbSize: 0,
    };
    unsafe {
        client
            .Initialize(
                AUDCLNT_SHAREMODE_SHARED,
                AUDCLNT_STREAMFLAGS_LOOPBACK | AUDCLNT_STREAMFLAGS_EVENTCALLBACK | AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM,
                200 * 10_000, // 200 ms buffer, in 100 ns units
                0,
                &format,
                None,
            )
            .context("setting up the program capture")?;
        let event: HANDLE = CreateEventW(None, false, false, None)?;
        client.SetEventHandle(event)?;
        let capture: IAudioCaptureClient = client.GetService()?;
        client.Start()?;
        let _ = ready.try_send(Ok(()));

        let mut buf: Vec<f32> = Vec::with_capacity(RATE as usize / 10 * CHANNELS);
        while !stop.load(Ordering::SeqCst) {
            if WaitForSingleObject(event, 200) != WAIT_OBJECT_0 {
                continue;
            }
            loop {
                let frames = capture.GetNextPacketSize()?;
                if frames == 0 {
                    break;
                }
                let mut data = std::ptr::null_mut();
                let mut n = 0u32;
                let mut flags = 0u32;
                capture.GetBuffer(&mut data, &mut n, &mut flags, None, None)?;
                buf.clear();
                if flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32 != 0 || data.is_null() {
                    buf.resize(n as usize * CHANNELS, 0.0);
                } else {
                    buf.extend_from_slice(std::slice::from_raw_parts(data as *const f32, n as usize * CHANNELS));
                }
                capture.ReleaseBuffer(n)?;
                on_audio(&buf);
            }
        }
        let _ = client.Stop();
        let _ = CloseHandle(event);
    }
    Ok(())
}
