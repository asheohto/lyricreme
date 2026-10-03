<div align="center">

  <img src="assets/logo.png" alt="LyricReme Logo" width="140" />

  # Lyricreme

  Desktop lyrics overlay for Windows with a music-reactive visualizer.

  [![Release](https://img.shields.io/github/v/release/asheohto/lyricreme?style=for-the-badge&logo=github&color=BC96E6)](https://github.com/asheohto/lyricreme/releases/latest)
  [![License](https://img.shields.io/badge/License-MIT-2ecc71?style=for-the-badge)](LICENSE)
  [![Support on Ko-fi](https://img.shields.io/badge/Ko--fi-omoretti-FF5E5B?style=for-the-badge&logo=kofi&logoColor=white)](https://ko-fi.com/omoretti)

</div>

---

## About

LyricReme is a borderless desktop lyrics overlay for Windows. It receives track updates from Pear Desktop's Tuna plugin, fetches synchronized lyrics from LRCLIB, and renders a floating two-line overlay over games, browsers, and desktop apps.

When a track has no timed lyrics available, it displays a music-reactive visualizer scaled to system audio loudness.

---

## Features

- Borderless and transparent window with per-pixel alpha.
- Music-reactive visualizer powered by WASAPI loopback audio when lyrics are not found.
- Synchronized timed lyrics from LRCLIB with local caching in `%LOCALAPPDATA%\LyricReme\cache\`.
- Two-line layout: current active line in bold with drop shadows, next line preview.
- Click-through mode (`WS_EX_TRANSPARENT`) so clicks pass directly to background apps.
- System tray menu for position anchors, text sizing, colors, outline, opacity, and timing offsets.
- Written in Rust using Win32 and Direct2D/DirectWrite.

---

## Preview
<img width="1919" height="1033" alt="image" src="https://github.com/user-attachments/assets/89f37bbd-e4ed-48d8-b8ee-434e0f6a63b7" />
<img width="1919" height="1033" alt="image" src="https://github.com/user-attachments/assets/4aae80a3-a7f9-4b6c-b6ce-a11fa16b9670" />
<img width="1919" height="1029" alt="image" src="https://github.com/user-attachments/assets/967b75ff-3125-4545-bae5-bf6850add03d" />

---

## Getting Started

### 1. Enable Tuna in Pear Desktop

1. Open Pear Desktop.
2. Go to Settings -> Plugins.
3. Turn on Tuna OBS (sends track updates to `127.0.0.1:1608`).
<img width="274" height="206" alt="image" src="https://github.com/user-attachments/assets/dd7f6b55-7814-417e-b561-f96377003686" />

### 2. Run LyricReme

1. Download `lyricreme.exe` from Releases.
2. Run `lyricreme.exe`.
3. Play any track in Pear Desktop.

---

## Building from Source

Requires Windows and Rust with the MSVC toolchain:

```powershell
git clone https://github.com/asheohto/lyricreme.git
cd lyricreme
cargo build --release
.\target\release\lyricreme.exe
```

---

## Tray Menu

Right-click the LyricReme icon in the system tray:

| Option | Description |
| :--- | :--- |
| Position | Snap overlay to 9 screen anchors (Top Center, Top Left, Bottom Center, etc.) |
| Text Size | Font sizes from Small to Huge with automatic window bounds scaling |
| Text Color | Color presets (White, Black, Cream, Sky, Pink, Mint, Gold) |
| Outline | Drop shadow stroke width (None, Thin, Medium, Thick) |
| Opacity | Overlay transparency (40%, 60%, 80%, 100%) |
| Visualizer | Toggle audio-reactive pulse when lyrics are not found |
| Click-Through | Toggle click transparency |
| Lock Position | Unlock to drag the overlay with the mouse |
| Nudge Timing | Adjust sync offset (+0.2s / -0.2s / Reset) |
<img width="465" height="361" alt="image" src="https://github.com/user-attachments/assets/b30180b0-2f08-4d09-91e2-8c630a0c04c9" />

---

## License

MIT - see [LICENSE](LICENSE).

---

## Support

- Issues: https://github.com/asheohto/lyricreme/issues
- Ko-fi: https://ko-fi.com/omoretti
