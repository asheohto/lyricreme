pub mod client;
pub mod parser;

pub use client::LrcLibClient;
#[allow(unused_imports)]
pub use parser::{LyricLine, ParsedLrc};
