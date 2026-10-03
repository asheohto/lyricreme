pub mod client;
pub mod hiragana;
pub mod parser;

pub use client::LrcLibClient;
#[allow(unused_imports)]
pub use hiragana::{contains_japanese, convert_lrc_to_hiragana, convert_lrc_to_romaji, convert_lrc_to_transliterated};
#[allow(unused_imports)]
pub use parser::{LyricLine, ParsedLrc};
