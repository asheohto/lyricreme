use std::sync::mpsc::Sender;
use std::time::Instant;

use crate::listener::TunaUpdate;
use crate::lrclib::ParsedLrc;

#[derive(Debug, Clone, PartialEq)]
pub enum LyricsStatus {
    Idle,
    Loading,
    Loaded,
    NotFound,
}

#[derive(Debug, Clone)]
pub struct LrcFetchRequest {
    pub title: String,
    pub artist: String,
    pub duration_sec: Option<u64>,
}

pub struct PlayerState {
    pub current_track: String,
    pub current_artist: String,
    pub duration_ms: u64,
    pub is_playing: bool,
    pub reported_progress_ms: u64,
    pub last_update: Option<Instant>,
    pub parsed_lrc: Option<ParsedLrc>,
    pub lyrics_status: LyricsStatus,
    /// Normalised 0.0–1.0 loudness of the system audio mix, published by the
    /// WASAPI loopback capture thread (`audio` module) and read by the UI to
    /// drive the visualizer pulse. 0.0 = silence / capture unavailable.
    pub audio_level: f32,
}

impl Default for PlayerState {
    fn default() -> Self {
        Self {
            current_track: String::new(),
            current_artist: String::new(),
            duration_ms: 0,
            is_playing: false,
            reported_progress_ms: 0,
            last_update: None,
            parsed_lrc: None,
            lyrics_status: LyricsStatus::Idle,
            audio_level: 0.0,
        }
    }
}

impl PlayerState {
    pub fn update_from_tuna(
        &mut self,
        update: TunaUpdate,
        fetch_tx: &Sender<LrcFetchRequest>,
    ) {
        let track_changed = self.current_track != update.title || self.current_artist != update.artist;

        self.current_track = update.title.clone();
        self.current_artist = update.artist.clone();
        self.duration_ms = update.duration_ms;
        self.is_playing = update.is_playing;
        self.reported_progress_ms = update.progress_ms;
        self.last_update = Some(Instant::now());

        if track_changed {
            self.parsed_lrc = None;
            self.lyrics_status = LyricsStatus::Loading;

            let duration_sec = if update.duration_ms > 0 {
                Some(update.duration_ms / 1000)
            } else {
                None
            };

            let _ = fetch_tx.send(LrcFetchRequest {
                title: update.title,
                artist: update.artist,
                duration_sec,
            });
        }
    }

    pub fn set_lyrics(&mut self, lrc_text: Option<String>) {
        if let Some(text) = lrc_text {
            let parsed = ParsedLrc::parse(&text);
            if parsed.lines.is_empty() {
                self.parsed_lrc = None;
                self.lyrics_status = LyricsStatus::NotFound;
            } else {
                self.parsed_lrc = Some(parsed);
                self.lyrics_status = LyricsStatus::Loaded;
            }
        } else {
            self.parsed_lrc = None;
            self.lyrics_status = LyricsStatus::NotFound;
        }
    }

    pub fn current_position_ms(&self, offset_ms: i64) -> u64 {
        let base_pos = self.reported_progress_ms;

        let interpolated = if self.is_playing {
            if let Some(last_time) = self.last_update {
                let elapsed = last_time.elapsed().as_millis() as u64;
                base_pos.saturating_add(elapsed)
            } else {
                base_pos
            }
        } else {
            base_pos
        };

        let with_offset = (interpolated as i64) + offset_ms;
        let clamped = if with_offset < 0 {
            0
        } else if self.duration_ms > 0 && with_offset > self.duration_ms as i64 {
            self.duration_ms
        } else {
            with_offset as u64
        };

        clamped
    }

    pub fn get_display_lyrics(&self, offset_ms: i64) -> (String, String) {
        if self.current_track.is_empty() {
            return (
                "LyricReme — Waiting for Pear Desktop...".to_string(),
                "Play any track on YouTube Music to begin".to_string(),
            );
        }

        match self.lyrics_status {
            LyricsStatus::Loading => (
                format!("{} - {}", self.current_track, self.current_artist),
                "Fetching timed lyrics from LRCLIB...".to_string(),
            ),
            LyricsStatus::NotFound => (
                String::new(),
                String::new(),
            ),
            LyricsStatus::Loaded => {
                if let Some(ref lrc) = self.parsed_lrc {
                    let pos = self.current_position_ms(offset_ms);
                    let (curr, next) = lrc.get_current_and_next(pos);

                    let line1 = curr
                        .map(|l| l.text.clone())
                        .unwrap_or_else(|| format!("♪ {} - {} ♪", self.current_track, self.current_artist));

                    let line2 = next
                        .map(|l| l.text.clone())
                        .unwrap_or_default();

                    (line1, line2)
                } else {
                    (
                        format!("{} - {}", self.current_track, self.current_artist),
                        String::new(),
                    )
                }
            }
            LyricsStatus::Idle => (
                "LyricReme — Waiting for Pear Desktop...".to_string(),
                String::new(),
            ),
        }
    }
}
