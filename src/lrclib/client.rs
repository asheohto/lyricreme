use std::fs;
use std::path::PathBuf;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct LrcLibResponse {
    pub id: Option<u64>,
    pub name: Option<String>,
    #[serde(rename = "trackName")]
    pub track_name: Option<String>,
    #[serde(rename = "artistName")]
    pub artist_name: Option<String>,
    #[serde(rename = "albumName")]
    pub album_name: Option<String>,
    pub duration: Option<f64>,
    pub instrumental: Option<bool>,
    #[serde(rename = "plainLyrics")]
    pub plain_lyrics: Option<String>,
    #[serde(rename = "syncedLyrics")]
    pub synced_lyrics: Option<String>,
}

pub struct LrcLibClient {
    cache_dir: PathBuf,
}

impl LrcLibClient {
    pub fn new() -> Self {
        let cache_dir = dirs_cache_dir().unwrap_or_else(|| PathBuf::from("./lyrics_cache"));
        let _ = fs::create_dir_all(&cache_dir);
        Self { cache_dir }
    }

    fn cache_path(&self, artist: &str, title: &str) -> PathBuf {
        let safe_name = format!("{}_{}", sanitize_filename(artist), sanitize_filename(title));
        self.cache_dir.join(format!("{}.lrc", safe_name))
    }

    pub fn get_lyrics(&self, artist: &str, title: &str, duration_sec: Option<u64>) -> Option<String> {
        let clean_artist = clean_metadata(artist);
        let clean_title = clean_metadata(title);

        let cache_file = self.cache_path(&clean_artist, &clean_title);
        if cache_file.exists() {
            if let Ok(content) = fs::read_to_string(&cache_file) {
                if !content.trim().is_empty() {
                    return Some(content);
                }
            }
        }

        if let Some(lyrics) = self.fetch_exact(&clean_artist, &clean_title, duration_sec) {
            let _ = fs::write(&cache_file, &lyrics);
            return Some(lyrics);
        }

        if let Some(lyrics) = self.fetch_search(&clean_artist, &clean_title) {
            let _ = fs::write(&cache_file, &lyrics);
            return Some(lyrics);
        }

        None
    }

    fn fetch_exact(&self, artist: &str, title: &str, duration_sec: Option<u64>) -> Option<String> {
        let mut url = format!(
            "https://lrclib.net/api/get?track_name={}&artist_name={}",
            urlencoding(title),
            urlencoding(artist)
        );

        if let Some(dur) = duration_sec {
            if dur > 0 {
                url.push_str(&format!("&duration={}", dur));
            }
        }

        let resp = ureq::get(&url)
            .set("User-Agent", "LyricReme/0.1.0 (https://github.com/asheohto/lyricreme)")
            .timeout(std::time::Duration::from_secs(5))
            .call()
            .ok()?;

        if resp.status() == 200 {
            let data: LrcLibResponse = resp.into_json().ok()?;
            data.synced_lyrics.filter(|s| !s.trim().is_empty())
        } else {
            None
        }
    }

    fn fetch_search(&self, artist: &str, title: &str) -> Option<String> {
        let query = format!("{} {}", title, artist);
        let url = format!("https://lrclib.net/api/search?q={}", urlencoding(&query));

        let resp = ureq::get(&url)
            .set("User-Agent", "LyricReme/0.1.0 (https://github.com/asheohto/lyricreme)")
            .timeout(std::time::Duration::from_secs(5))
            .call()
            .ok()?;

        if resp.status() == 200 {
            let list: Vec<LrcLibResponse> = resp.into_json().ok()?;
            for item in list {
                if let Some(synced) = item.synced_lyrics {
                    if !synced.trim().is_empty() {
                        return Some(synced);
                    }
                }
            }
        }

        None
    }
}

fn dirs_cache_dir() -> Option<PathBuf> {
    if let Ok(local_appdata) = std::env::var("LOCALAPPDATA") {
        Some(PathBuf::from(local_appdata).join("LyricReme").join("cache"))
    } else {
        None
    }
}

fn sanitize_filename(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect()
}

pub fn clean_metadata(text: &str) -> String {
    let mut clean = text.trim().to_string();

    let suffixes_to_remove = [
        "(Official Music Video)",
        "(Official Video)",
        "(Official Audio)",
        "(Lyrics)",
        "(Lyric Video)",
        "[Official Music Video]",
        "[Official Video]",
        "[MV]",
        "(MV)",
        "[Audio]",
        "(Audio)",
    ];

    for s in &suffixes_to_remove {
        if let Some(pos) = clean.to_lowercase().find(&s.to_lowercase()) {
            clean.replace_range(pos..pos + s.len(), "");
        }
    }

    clean.trim().to_string()
}

fn urlencoding(s: &str) -> String {
    let mut out = String::new();
    for b in s.as_bytes() {
        match *b {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*b as char);
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clean_metadata() {
        assert_eq!(
            clean_metadata("Blinding Lights (Official Video)"),
            "Blinding Lights"
        );
        assert_eq!(
            clean_metadata("Song Name [MV]"),
            "Song Name"
        );
    }
}
