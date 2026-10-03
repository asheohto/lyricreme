/// WASAPI loopback level meter driving the visualizer pulse.
///
/// Tuna only ever POSTs track metadata, so the "music" signal has to come from
/// the speakers themselves: this captures the default render endpoint's mix in
/// shared loopback mode and publishes a 0..1 loudness into `PlayerState`.
/// Uses only the `windows` crate (no extra crates) — RMS is all a scale pulse
/// needs, so no FFT.
use std::slice;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use windows::Win32::Media::Audio::{
    eMultimedia, eRender, IAudioCaptureClient, IAudioClient, IMMDeviceEnumerator,
    AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_LOOPBACK,
    MMDeviceEnumerator,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, CLSCTX_ALL, CLSCTX_INPROC_SERVER,
    COINIT_MULTITHREADED,
};

use crate::player::PlayerState;

/// Poll interval for the capture loop. The UI smooths to frame rate on top of
/// this, so ~50 Hz of fresh targets is plenty.
const POLL_INTERVAL: Duration = Duration::from_millis(20);
/// Back-off before rebuilding the client after a device change / engine stop.
const RETRY_DELAY: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SampleFormat {
    F32,
    I16,
}

/// Sample layout of the shared-mode mix format.
///
/// ponytail: 32-bit is assumed float (true for every normal endpoint, including
/// WAVE_FORMAT_EXTENSIBLE); a 32-bit *integer* mix would read quieter than
/// reality, never crash. Upgrade path: read WAVEFORMATEXTENSIBLE.SubFormat and
/// check KSDATAFORMAT_SUBTYPE_PCM.
fn sample_format(bits_per_sample: u16) -> Option<SampleFormat> {
    match bits_per_sample {
        32 => Some(SampleFormat::F32),
        16 => Some(SampleFormat::I16),
        _ => None,
    }
}

/// Sum of squared amplitudes over a raw interleaved block, so packets can be
/// pooled before taking the square root. `samples` is the total across channels.
unsafe fn block_energy(ptr: *const u8, samples: usize, fmt: SampleFormat) -> f64 {
    if ptr.is_null() || samples == 0 {
        return 0.0;
    }
    match fmt {
        SampleFormat::F32 => slice::from_raw_parts(ptr as *const f32, samples)
            .iter()
            .map(|s| (*s as f64) * (*s as f64))
            .sum(),
        SampleFormat::I16 => slice::from_raw_parts(ptr as *const i16, samples)
            .iter()
            .map(|s| {
                let v = *s as f64 / 32768.0;
                v * v
            })
            .sum(),
    }
}

/// Maps RMS amplitude to a 0..1 visualizer level. Music RMS sits around
/// 0.02–0.25, so a 6x gain spreads normal playback over most of the range while
/// still clamping on hot masters.
fn level_from_rms(rms: f32) -> f32 {
    let scaled = (rms * 8.5).clamp(0.0, 1.0);
    scaled.powf(1.1)
}

/// Starts the level meter on its own thread. The thread owns its COM apartment
/// and keeps retrying, so a machine that boots without an audio device recovers
/// as soon as one appears.
pub fn spawn_level_capture(state: Arc<Mutex<PlayerState>>) -> Result<(), String> {
    thread::Builder::new()
        .name("audio_capture".into())
        .spawn(move || unsafe {
            // MTA: no message pump needed for a polled capture client.
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            println!("[LyricReme] Visualizer level meter started (WASAPI loopback).");
            loop {
                if let Err(e) = capture_until_lost(&state) {
                    eprintln!("[LyricReme] Visualizer audio capture: {} (retrying)", e);
                }
                if let Ok(mut s) = state.lock() {
                    s.audio_level = 0.0;
                }
                thread::sleep(RETRY_DELAY);
            }
        })
        .map_err(|e| format!("Failed to spawn audio thread: {}", e))?;
    Ok(())
}

/// Opens the default render endpoint in loopback mode and feeds
/// `PlayerState::audio_level` until the endpoint goes away (device change, engine
/// stop, unplug), which is reported as `Err` so the caller can rebuild.
unsafe fn capture_until_lost(state: &Arc<Mutex<PlayerState>>) -> Result<(), String> {
    let enumerator: IMMDeviceEnumerator =
        CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_INPROC_SERVER)
            .map_err(|e| format!("no audio device enumerator: {}", e))?;
    let device = enumerator
        .GetDefaultAudioEndpoint(eRender, eMultimedia)
        .map_err(|e| format!("no default playback device: {}", e))?;
    let client: IAudioClient = device
        .Activate(CLSCTX_ALL, None)
        .map_err(|e| format!("activate IAudioClient failed: {}", e))?;

    let mix = client
        .GetMixFormat()
        .map_err(|e| format!("GetMixFormat failed: {}", e))?;
    if mix.is_null() {
        return Err("GetMixFormat returned no format".into());
    }
    let bits = (*mix).wBitsPerSample;
    let format = sample_format(bits);
    let channels = (*mix).nChannels.max(1) as usize;

    // 200 ms buffer, no periodic events. Initialize only borrows the format, so
    // it is released immediately afterwards (all paths, including errors).
    let init = client.Initialize(
        AUDCLNT_SHAREMODE_SHARED,
        AUDCLNT_STREAMFLAGS_LOOPBACK,
        2_000_000,
        0,
        mix,
        None,
    );
    CoTaskMemFree(Some(mix as *const core::ffi::c_void));

    let format = format.ok_or_else(|| format!("unsupported mix format ({} bits/sample)", bits))?;
    init.map_err(|e| format!("loopback Initialize failed: {}", e))?;

    let capture: IAudioCaptureClient = client
        .GetService()
        .map_err(|e| format!("IAudioCaptureClient unavailable: {}", e))?;
    client
        .Start()
        .map_err(|e| format!("loopback Start failed: {}", e))?;

    loop {
        thread::sleep(POLL_INTERVAL);

        let mut sum = 0.0f64;
        let mut count = 0usize;
        loop {
            match capture.GetNextPacketSize() {
                Ok(0) => break,
                Ok(_) => {}
                Err(_) => return Err("capture stream lost".into()),
            }

            let mut data: *mut u8 = std::ptr::null_mut();
            let mut frames = 0u32;
            let mut flags = 0u32;
            if capture
                .GetBuffer(&mut data, &mut frames, &mut flags, None, None)
                .is_err()
            {
                break;
            }

            if flags & (AUDCLNT_BUFFERFLAGS_SILENT.0 as u32) == 0 && frames > 0 {
                let samples = frames as usize * channels;
                sum += block_energy(data, samples, format);
                count += samples;
            }

            let _ = capture.ReleaseBuffer(frames);
        }

        let rms = if count > 0 {
            (sum / count as f64).sqrt() as f32
        } else {
            0.0
        };

        if let Ok(mut s) = state.lock() {
            s.audio_level = level_from_rms(rms);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_level_from_rms_spreads_and_clamps() {
        assert_eq!(level_from_rms(0.0), 0.0);
        // Typical music RMS sits mid-range, not pegged at either end.
        let mid = level_from_rms(0.05);
        assert!(mid > 0.2 && mid < 0.4, "mid level was {}", mid);
        assert_eq!(level_from_rms(0.9), 1.0);
    }

    #[test]
    fn test_block_energy_reads_float_and_pcm() {
        let silence = vec![0f32; 64];
        let full = vec![1.0f32; 64];
        unsafe {
            assert_eq!(
                block_energy(silence.as_ptr() as *const u8, silence.len(), SampleFormat::F32),
                0.0
            );
            let e = block_energy(full.as_ptr() as *const u8, full.len(), SampleFormat::F32);
            assert!((e / full.len() as f64 - 1.0).abs() < 1e-9);
        }

        // i16::MIN is full-scale negative → squares to exactly 1.0 per sample.
        let pcm = vec![i16::MIN; 32];
        unsafe {
            let e = block_energy(pcm.as_ptr() as *const u8, pcm.len(), SampleFormat::I16);
            assert!((e / pcm.len() as f64 - 1.0).abs() < 1e-9);
        }
    }

    #[test]
    fn test_sample_format_detection() {
        assert_eq!(sample_format(32), Some(SampleFormat::F32));
        assert_eq!(sample_format(16), Some(SampleFormat::I16));
        assert_eq!(sample_format(8), None);
    }
}
