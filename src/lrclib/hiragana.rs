use wana_kana::ConvertJapanese;

use crate::lrclib::parser::{LyricLine, ParsedLrc};

/// Checks if a string contains Kanji or Katakana characters that can be converted to Hiragana.
pub fn contains_japanese(text: &str) -> bool {
    text.chars().any(|c| {
        ('\u{4E00}'..='\u{9FFF}').contains(&c) // Kanji (CJK Unified Ideographs)
            || ('\u{30A0}'..='\u{30FF}').contains(&c) // Katakana
    })
}

/// Converts a romanized or Japanese string into Hiragana.
pub fn to_hiragana(text: &str) -> String {
    text.to_hiragana()
}

/// Fetches the romanized transliteration of a Japanese text chunk from Google's translation service.
pub fn fetch_transliteration(japanese_text: &str) -> Option<String> {
    let encoded = urlencode(japanese_text);
    let url = format!(
        "https://translate.googleapis.com/translate_a/single?client=gtx&sl=ja&tl=ja&dt=rm&q={}",
        encoded
    );

    let resp = ureq::get(&url)
        .set("User-Agent", "Mozilla/5.0")
        .timeout(std::time::Duration::from_secs(6))
        .call()
        .ok()?;

    if resp.status() != 200 {
        return None;
    }

    let val: serde_json::Value = resp.into_json().ok()?;
    extract_transliteration(&val)
}

/// Extracts transliterated text from Google's response structure.
pub fn extract_transliteration(val: &serde_json::Value) -> Option<String> {
    let arr = val.get(0)?.as_array()?;
    let mut result = String::new();

    for item in arr {
        if let Some(sub) = item.as_array() {
            // Google puts transliteration at index 3 or index 2 depending on segment count
            if let Some(s) = sub.get(3).and_then(|v| v.as_str()) {
                result.push_str(s);
            } else if let Some(s) = sub.get(2).and_then(|v| v.as_str()) {
                result.push_str(s);
            }
        }
    }

    if result.trim().is_empty() {
        None
    } else {
        Some(result)
    }
}

/// Converts an entire synchronized LRC string into Hiragana.
/// Preserves all timestamps and non-Japanese lines.
pub fn convert_lrc_to_hiragana(lrc_content: &str) -> Option<String> {
    if !contains_japanese(lrc_content) {
        return None;
    }

    let parsed = ParsedLrc::parse(lrc_content);
    if parsed.lines.is_empty() {
        return None;
    }

    // Process in batches of up to 20 lines separated by " ~~~ "
    const BATCH_SIZE: usize = 20;
    const DELIM: &str = " ~~~ ";

    let mut converted_lines: Vec<LyricLine> = Vec::with_capacity(parsed.lines.len());

    for chunk in parsed.lines.chunks(BATCH_SIZE) {
        // Collect indices of lines in this chunk that actually need Japanese transliteration
        let need_conv: Vec<bool> = chunk.iter().map(|l| contains_japanese(&l.text)).collect();

        if need_conv.iter().any(|&b| b) {
            let combined: String = chunk
                .iter()
                .map(|l| l.text.trim())
                .collect::<Vec<_>>()
                .join(DELIM);

            if let Some(translit) = fetch_transliteration(&combined) {
                // Split transliteration by delimiter
                let parts: Vec<&str> = translit.split("~~~").collect();

                if parts.len() == chunk.len() {
                    for (i, line) in chunk.iter().enumerate() {
                        let text = if need_conv[i] {
                            let hira = to_hiragana(parts[i].trim());
                            if hira.trim().is_empty() {
                                line.text.clone()
                            } else {
                                hira
                            }
                        } else {
                            line.text.clone()
                        };
                        converted_lines.push(LyricLine {
                            time_ms: line.time_ms,
                            text,
                        });
                    }
                    continue;
                }
            }
        }

        // Fallback for this chunk: keep original lines
        for line in chunk {
            converted_lines.push(line.clone());
        }
    }

    // Serialize back into standard LRC format
    let mut out = String::new();
    for line in converted_lines {
        let total_sec = line.time_ms / 1000;
        let ms_rem = (line.time_ms % 1000) / 10;
        let min = total_sec / 60;
        let sec = total_sec % 60;
        out.push_str(&format!("[{:02}:{:02}.{:02}] {}\n", min, sec, ms_rem, line.text));
    }

    Some(out)
}

fn urlencode(s: &str) -> String {
    let mut out = String::new();
    for b in s.as_bytes() {
        match *b {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*b as char);
            }
            _ => {
                out.push_str(&format!("%{:02X}", b));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_contains_japanese() {
        assert!(contains_japanese("無敵の笑顔"));
        assert!(contains_japanese("アイドル"));
        assert!(!contains_japanese("Hello World 123!"));
        assert!(!contains_japanese("Kyo wa i tenki desu ne"));
    }

    #[test]
    fn test_to_hiragana_conversion() {
        let hira = to_hiragana("Muteki no egao de arasu media");
        assert_eq!(hira, "むてき の えがお で あらす めぢあ");

        let hira2 = to_hiragana("shiritai sono himitsu");
        assert_eq!(hira2, "しりたい その ひみつ");
    }

    #[test]
    fn test_extract_transliteration() {
        let json: serde_json::Value = serde_json::json!([
            [
                [null, null, null, "Muteki no egao ~~~ Shiritai"]
            ]
        ]);
        let extracted = extract_transliteration(&json);
        assert_eq!(extracted.as_deref(), Some("Muteki no egao ~~~ Shiritai"));
    }
}
