<div align="center">

  <img src="assets/logo.png" alt="LyricReme Logo" width="140" />

  # LyricReme

  A lightweight, borderless desktop lyrics overlay for Windows with a music-reactive visualizer.

  [![Release](https://img.shields.io/github/v/release/asheohto/lyricreme?style=for-the-badge&logo=github&color=BC96E6)](https://github.com/asheohto/lyricreme/releases/latest)
  [![License](https://img.shields.io/badge/License-MIT-2ecc71?style=for-the-badge)](LICENSE)
  [![Support on Ko-fi](https://img.shields.io/badge/Ko--fi-omoretti-FF5E5B?style=for-the-badge&logo=kofi&logoColor=white)](https://ko-fi.com/omoretti)

</div>

---

## ✨ What it does

- ✅ **Borderless & Transparent**: Floats cleanly at the top of your screen with per-pixel alpha and zero background box.
- ✅ **Music-Reactive Visualizer**: Pulses dynamically to WASAPI loopback audio loudness when lyrics are not found.
- ✅ **Synchronized Timed Lyrics**: Fetches `.lrc` lyrics from [LRCLIB](https://lrclib.net/) and auto-caches them locally.
- ✅ **Two-Line Dynamic Layout**: Active line with crisp drop-shadow legibility plus upcoming preview line with directional spring sliding.
- ✅ **Click-Through Transparency (`WS_EX_TRANSPARENT`)**: Mouse clicks pass through cleanly to underlying games and apps.
- ✅ **System Tray Controls**: Adjust text size, font color, outline thickness, opacity, screen anchor, and timing nudge (`±0.2s`).
- ✅ **Ultra-Low Overhead**: Native Win32 + Direct2D/DirectWrite in Rust (<0.1% CPU, ~60MB RAM, zero webview bloat).

---

## 🚀 Getting Started

### 1. Enable Tuna in Pear Desktop

1. Open **[Pear Desktop](https://github.com/pear-devs/pear-desktop)** (YouTube Music).
2. Go to **Settings** -> **Plugins**.
3. Toggle **Tuna OBS** to **ON** (broadcasts track info to `127.0.0.1:1608`).

### 2. Run LyricReme

1. Download the latest **`lyricreme.exe`** from [**Releases**](https://github.com/asheohto/lyricreme/releases/latest).
2. Run `lyricreme.exe`.
3. Play music — lyrics synchronize automatically!

---

## 🛠️ Building from Source (Developers)

Requires Windows 10/11 and Rust (MSVC toolchain):

```powershell
# Clone the repository
git clone https://github.com/asheohto/lyricreme.git
cd lyricreme

# Build release binary
cargo build --release

# Run
.\target\release\lyricreme.exe
```

---

## ⚙️ System Tray Menu

Right-click the **LyricReme** tray icon near your clock:

| Option | Description |
| :--- | :--- |
| **Position** | Snap to 9 screen anchors (Top Center, Top Left, Bottom Center, etc.) |
| **Text Size** | Switch font sizes from Small to Huge with auto-scaled window bounds |
| **Text Color** | Pick presets (White, Black, Cream, Sky, Pink, Mint, Gold) |
| **Outline** | Adjust drop-shadow stroke (None, Thin, Medium, Thick) |
| **Opacity** | Set overlay master transparency (40%, 60%, 80%, 100%) |
| **Music Visualizer** | Toggle music-reactive beat pulse when lyrics are not found |
| **Click-Through** | Enable or disable click transparency |
| **Lock Position** | Unlock to drag and freely reposition the overlay anywhere |
| **Nudge Timing** | Lead/lag offset adjustments (`+0.2s` / `-0.2s` / Reset) |

---

## 📜 License

MIT – see the [LICENSE](LICENSE) file.

---

## 📧 Contact & Support

- GitHub Issues: https://github.com/asheohto/lyricreme/issues
- Support / Donate: https://ko-fi.com/omoretti