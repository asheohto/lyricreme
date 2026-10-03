use std::sync::mpsc::Sender;
use std::thread;
use std::time::Duration;

use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSessionManager,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus,
};
use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};

use crate::listener::TunaUpdate;

pub struct GsmtcListener;

impl GsmtcListener {
    pub fn start(tx: Sender<TunaUpdate>) -> Result<(), String> {
        thread::Builder::new()
            .name("gsmtc_listener".into())
            .spawn(move || {
                let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
                println!("[LyricReme] Starting Windows System Media listener (GSMTC)...");

                let mut last_title = String::new();
                let mut last_artist = String::new();
                let mut last_is_playing = false;

                loop {
                    thread::sleep(Duration::from_millis(500));

                    if let Ok(manager) = GlobalSystemMediaTransportControlsSessionManager::RequestAsync().and_then(|op| op.get()) {
                        if let Ok(session) = manager.GetCurrentSession() {
                            let media_props = match session.TryGetMediaPropertiesAsync().and_then(|op| op.get()) {
                                Ok(props) => props,
                                Err(_) => continue,
                            };

                            let title = media_props.Title().map(|s| s.to_string()).unwrap_or_default().trim().to_string();
                            let artist = media_props.Artist().map(|s| s.to_string()).unwrap_or_default().trim().to_string();

                            if title.is_empty() {
                                continue;
                            }

                            let is_playing = if let Ok(info) = session.GetPlaybackInfo() {
                                if let Ok(status) = info.PlaybackStatus() {
                                    status == GlobalSystemMediaTransportControlsSessionPlaybackStatus::Playing
                                } else {
                                    false
                                }
                            } else {
                                false
                            };

                            let mut duration_ms = 0u64;
                            let mut progress_ms = 0u64;

                            if let Ok(timeline) = session.GetTimelineProperties() {
                                if let Ok(end) = timeline.EndTime() {
                                    // TimeSpan is in 100-nanosecond units (10_000 per ms)
                                    duration_ms = (end.Duration / 10_000) as u64;
                                }
                                if let Ok(pos) = timeline.Position() {
                                    progress_ms = (pos.Duration / 10_000) as u64;
                                }
                            }

                            let state_changed = title != last_title
                                || artist != last_artist
                                || is_playing != last_is_playing;

                            if state_changed {
                                last_title = title.clone();
                                last_artist = artist.clone();
                                last_is_playing = is_playing;

                                let _ = tx.send(TunaUpdate {
                                    title,
                                    artist,
                                    duration_ms,
                                    progress_ms,
                                    is_playing,
                                });
                            }
                        }
                    }
                }
            })
            .map_err(|e| format!("Failed to spawn GSMTC listener thread: {}", e))?;

        Ok(())
    }
}
