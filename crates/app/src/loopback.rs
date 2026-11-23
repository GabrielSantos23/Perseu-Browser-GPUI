//! Records what this app's pages play: WASAPI process loopback of the
//! WebView2 browser process and its children, never other apps. Windows does
//! not count WebView2's processes as part of the app's own tree, so the
//! browser process is the target, not this process.

use std::sync::mpsc::{Sender, channel};
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result};
use windows::Win32::Media::Audio::WAVE_FORMAT_PCM;
use windows::Win32::Media::Audio::{
    AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM,
    AUDCLNT_STREAMFLAGS_LOOPBACK, AUDIOCLIENT_ACTIVATION_PARAMS,
    AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK, ActivateAudioInterfaceAsync,
    IActivateAudioInterfaceAsyncOperation, IActivateAudioInterfaceCompletionHandler,
    IActivateAudioInterfaceCompletionHandler_Impl, IAudioCaptureClient, IAudioClient,
    PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE, VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK,
    WAVEFORMATEX,
};
use windows::Win32::System::Com::StructuredStorage::PROPVARIANT;
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, IAgileObject};
use windows::core::{Interface, implement};

/// The rate the capture is converted to.
pub const SAMPLE_RATE: u32 = 48_000;
const CHANNELS: u16 = 2;
const BUFFER_100NS: i64 = 2_000_000;
const POLL: Duration = Duration::from_millis(10);
const VT_BLOB: u16 = 65;

/// `PROPVARIANT` holding a `VT_BLOB`, laid out as Windows reads it.
#[repr(C)]
struct BlobVariant {
    vt: u16,
    reserved: [u16; 3],
    size: u32,
    data: *const u8,
}

#[implement(IActivateAudioInterfaceCompletionHandler, IAgileObject)]
struct Activated(Sender<()>);

impl windows::Win32::System::Com::IAgileObject_Impl for Activated_Impl {}

impl IActivateAudioInterfaceCompletionHandler_Impl for Activated_Impl {
    fn ActivateCompleted(
        &self,
        _: windows::core::Ref<IActivateAudioInterfaceAsyncOperation>,
    ) -> windows::core::Result<()> {
        let _ = self.0.send(());
        Ok(())
    }
}

/// Records mono samples of `process` and its children at [`SAMPLE_RATE`] and hands the whole recording so
/// far to `snapshot` at each of `marks`; stops after the last mark or as
/// soon as `snapshot` returns false. Blocks, so run it on its own thread.
pub fn record(
    process: u32,
    marks: &[Duration],
    mut snapshot: impl FnMut(&[f32]) -> bool,
) -> Result<()> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let client = activate(process)?;
        let format = WAVEFORMATEX {
            wFormatTag: WAVE_FORMAT_PCM as u16,
            nChannels: CHANNELS,
            nSamplesPerSec: SAMPLE_RATE,
            nAvgBytesPerSec: SAMPLE_RATE * u32::from(CHANNELS) * 2,
            nBlockAlign: CHANNELS * 2,
            wBitsPerSample: 16,
            cbSize: 0,
        };
        client
            .Initialize(
                AUDCLNT_SHAREMODE_SHARED,
                AUDCLNT_STREAMFLAGS_LOOPBACK | AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM,
                BUFFER_100NS,
                0,
                &format,
                None,
            )
            .context("starting the audio capture")?;
        let capture: IAudioCaptureClient = client.GetService().context("audio capture")?;
        client.Start().context("starting the audio capture")?;
        let started = Instant::now();
        let mut samples: Vec<f32> = Vec::new();
        let mut next = 0;
        while next < marks.len() {
            while capture.GetNextPacketSize()? > 0 {
                let mut data = std::ptr::null_mut();
                let mut frames = 0;
                let mut flags = 0;
                capture.GetBuffer(&mut data, &mut frames, &mut flags, None, None)?;
                let frames = frames as usize;
                if flags & (AUDCLNT_BUFFERFLAGS_SILENT.0 as u32) != 0 || data.is_null() {
                    samples.extend(std::iter::repeat_n(0.0, frames));
                } else {
                    let interleaved = std::slice::from_raw_parts(
                        data.cast::<i16>(),
                        frames * usize::from(CHANNELS),
                    );
                    samples.extend(
                        interleaved
                            .as_chunks::<2>()
                            .0
                            .iter()
                            .map(|[left, right]| (f32::from(*left) + f32::from(*right)) / 65_536.0),
                    );
                }
                capture.ReleaseBuffer(frames as u32)?;
            }
            // Silence produces no packets, so time, not samples, sets the marks.
            if started.elapsed() >= marks[next] {
                next += 1;
                if !snapshot(&samples) {
                    break;
                }
            }
            std::thread::sleep(POLL);
        }
        let _ = client.Stop();
        Ok(())
    }
}

unsafe fn activate(process: u32) -> Result<IAudioClient> {
    let mut params = AUDIOCLIENT_ACTIVATION_PARAMS {
        ActivationType: AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK,
        ..Default::default()
    };
    params.Anonymous.ProcessLoopbackParams.TargetProcessId = process;
    params.Anonymous.ProcessLoopbackParams.ProcessLoopbackMode =
        PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE;
    let blob = BlobVariant {
        vt: VT_BLOB,
        reserved: [0; 3],
        size: std::mem::size_of::<AUDIOCLIENT_ACTIVATION_PARAMS>() as u32,
        data: (&raw const params).cast(),
    };
    let (done, finished) = channel();
    let handler: IActivateAudioInterfaceCompletionHandler = Activated(done).into();
    unsafe {
        let operation = ActivateAudioInterfaceAsync(
            VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK,
            &IAudioClient::IID,
            Some((&raw const blob).cast::<PROPVARIANT>()),
            &handler,
        )
        .context("asking Windows for this app's audio")?;
        finished
            .recv_timeout(Duration::from_secs(5))
            .context("Windows did not hand over the audio")?;
        let mut result = windows::core::HRESULT(0);
        let mut client = None;
        operation.GetActivateResult(&mut result, &mut client)?;
        result
            .ok()
            .context("Windows refused to share this app's audio")?;
        client
            .context("no audio client")?
            .cast()
            .context("audio client")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Needs an audio device, so it only runs when asked for.
    #[test]
    #[ignore]
    fn this_process_can_be_recorded() {
        let mut lengths = Vec::new();
        record(
            std::process::id(),
            &[Duration::from_millis(300), Duration::from_millis(600)],
            |samples| {
                lengths.push(samples.len());
                true
            },
        )
        .unwrap();
        assert_eq!(lengths.len(), 2);
    }

    /// Saves 8 s of another process's audio as raw 48 kHz mono f32, for
    /// checking recognition by hand: `PERSEU_LOOPBACK_PID`, `..._OUT`.
    #[test]
    #[ignore]
    fn dump_process_audio() {
        let pid = std::env::var("PERSEU_LOOPBACK_PID")
            .unwrap()
            .parse()
            .unwrap();
        let out = std::env::var("PERSEU_LOOPBACK_OUT").unwrap();
        record(pid, &[Duration::from_secs(8)], |samples| {
            let bytes: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
            std::fs::write(&out, bytes).unwrap();
            false
        })
        .unwrap();
    }
}
