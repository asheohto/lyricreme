#[derive(Debug, Clone, PartialEq)]
pub struct LyricLine {
    pub time_ms: u64,
    pub text: String,
}

#[derive(Debug, Clone, Default)]
pub struct ParsedLrc {
    pub lines: Vec<LyricLine>,
}

impl ParsedLrc {
    pub fn parse(lrc_content: &str) -> Self {
        let mut lines = Vec::new();

        for raw_line in lrc_content.lines() {
            let trimmed = raw_line.trim();
            if trimmed.is_empty() {
                continue;
            }

            let mut remaining = trimmed;
            let mut timestamps = Vec::new();

            while remaining.starts_with('[') {
                if let Some(close_bracket) = remaining.find(']') {
                    let tag = &remaining[1..close_bracket];
                    if let Some(ms) = parse_timestamp(tag) {
                        timestamps.push(ms);
                    }
                    remaining = &remaining[close_bracket + 1..];
                } else {
                    break;
                }
            }

            let text = remaining.trim().to_string();
            for time_ms in timestamps {
                lines.push(LyricLine {
                    time_ms,
                    text: text.clone(),
                });
            }
        }

        lines.sort_by_key(|l| l.time_ms);

        Self { lines }
    }

    pub fn get_current_and_next(&self, current_ms: u64) -> (Option<&LyricLine>, Option<&LyricLine>) {
        if self.lines.is_empty() {
            return (None, None);
        }

        if current_ms < self.lines[0].time_ms {
            return (None, self.lines.first());
        }

        let idx = match self.lines.binary_search_by_key(&current_ms, |l| l.time_ms) {
            Ok(exact) => exact,
            Err(insert_idx) => {
                if insert_idx > 0 {
                    insert_idx - 1
                } else {
                    0
                }
            }
        };

        let current = self.lines.get(idx);
        let next = self.lines.get(idx + 1);

        (current, next)
    }
}

fn parse_timestamp(tag: &str) -> Option<u64> {
    let parts: Vec<&str> = tag.split(':').collect();
    if parts.len() != 2 {
        return None;
    }

    let minutes: u64 = parts[0].trim().parse().ok()?;
    let sec_parts: Vec<&str> = parts[1].split('.').collect();
    if sec_parts.is_empty() || sec_parts.len() > 2 {
        return None;
    }

    let seconds: u64 = sec_parts[0].trim().parse().ok()?;
    let millis: u64 = if sec_parts.len() == 2 {
        let frac = sec_parts[1].trim();
        match frac.len() {
            1 => frac.parse::<u64>().ok()? * 100,
            2 => frac.parse::<u64>().ok()? * 10,
            3 => frac.parse::<u64>().ok()?,
            _ => {
                let truncated = &frac[..3];
                truncated.parse::<u64>().ok()?
            }
        }
    } else {
        0
    };

    Some(minutes * 60_000 + seconds * 1_000 + millis)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_timestamps() {
        assert_eq!(parse_timestamp("01:23.45"), Some(83_450));
        assert_eq!(parse_timestamp("00:05.123"), Some(5_123));
        assert_eq!(parse_timestamp("02:10"), Some(130_000));
        assert_eq!(parse_timestamp("invalid"), None);
    }

    #[test]
    fn test_parse_lrc_content() {
        let sample = r#"
[ti:Sample Song]
[ar:Artist]
[00:04.50]Line one of lyrics
[00:09.80]Line two of lyrics
[00:15.00]Line three of lyrics
"#;
        let parsed = ParsedLrc::parse(sample);
        assert_eq!(parsed.lines.len(), 3);
        assert_eq!(parsed.lines[0].time_ms, 4_500);
        assert_eq!(parsed.lines[0].text, "Line one of lyrics");
        assert_eq!(parsed.lines[1].time_ms, 9_800);
        assert_eq!(parsed.lines[2].time_ms, 15_000);

        let (curr, next) = parsed.get_current_and_next(2_000);
        assert!(curr.is_none());
        assert_eq!(next.unwrap().text, "Line one of lyrics");

        let (curr, next) = parsed.get_current_and_next(6_000);
        assert_eq!(curr.unwrap().text, "Line one of lyrics");
        assert_eq!(next.unwrap().text, "Line two of lyrics");

        let (curr, next) = parsed.get_current_and_next(9_800);
        assert_eq!(curr.unwrap().text, "Line two of lyrics");
        assert_eq!(next.unwrap().text, "Line three of lyrics");

        let (curr, next) = parsed.get_current_and_next(25_000);
        assert_eq!(curr.unwrap().text, "Line three of lyrics");
        assert!(next.is_none());
    }
}
