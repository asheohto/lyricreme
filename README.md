# LyricReme (Crème Lyric)

An ultra-low-resource, high-performance desktop lyrics overlay for Windows. Designed specifically for **[Pear Desktop](https://github.com/pear-devs/pear-desktop)** (YouTube Music) with synchronized timed lyrics powered by **[LRCLIB](https://lrclib.net/)**.

---

## ✨ Features

- **Borderless & Transparent**: Floats cleanly at the top of your screen with zero background box or ugly borders.
- **Two-Line Display**:
  - **Top line**: Current active lyric in crisp bold text with drop shadows for high legibility over games, browsers, or dark/light wallpapers.
  - **Bottom line**: Upcoming lyric preview in subtle semi-transparent text.
- **Click-Through Transparency (`WS_EX_TRANSPARENT`)**: All mouse clicks pass directly through to windows, games, and browser tabs underneath.
- **Draggable Repositioning**: Right-click the system tray icon to unlock and freely drag the overlay anywhere on your screen.
- **Ultra-Low Memory Footprint**: Built with **Rust + Win32 + Direct2D/DirectWrite**. Uses negligible CPU (<0.1%) and compiles down to a single self-contained ~2.7 MB `.exe`.
- **LRCLIB Integration**: Automatically queries synchronized `.lrc` lyrics from LRCLIB and caches them locally in `%LOCALAPPDATA%\LyricReme\cache\`.
- **Nudge Timing**: On-the-fly timing delay adjustment (`+0.2s` / `-0.2s`) directly from the system tray menu.

---

## 🚀 Getting Started

### 1. Enable Tuna OBS in Pear Desktop

1. Open **Pear Desktop**.
2. Click the gear icon or navigate to **Settings** -> **Plugins**.
3. Locate **Tuna OBS** and toggle it **ON**.
4. *(Optional)* If prompted to restart Pear Desktop, restart it.

> Pear Desktop's Tuna OBS plugin automatically pushes millisecond-accurate track updates to `http://127.0.0.1:1608/` on track changes, play/pause, and seeking.

---

### 2. Run LyricReme

#### From Prebuilt Release:
Run `target\release\lyricreme.exe` or compile it yourself:

```powershell
cargo build --release
.\target\release\lyricreme.exe
```

When started, LyricReme will float at the top center of your main monitor. Play any track in Pear Desktop, and the lyrics will automatically fetch and start scrolling in sync!

---

## ⚙️ Controls & System Tray Menu

Look for the **LyricReme** icon in your Windows notification tray (bottom-right near your clock):

- **Click-Through (Transparent clicks)**: Toggle whether mouse clicks pass through the overlay.
- **Lock Position (Drag to move)**: Uncheck this to temporarily enable window dragging so you can position the overlay wherever you like.
- **Reset to Top Center**: Returns the overlay to the default top-center position.
- **Nudge Forward (+0.2s) / Nudge Backward (-0.2s)**: Adjust lyric lead/lag time.
- **Reset Offset (0.0s)**: Resets sync offset to zero.
- **Exit LyricReme**: Closes the application.

---

## 🛠️ Configuration

Configuration is automatically saved to:
```
%APPDATA%\LyricReme\config.json
```

Example `config.json`:
```json
{
  "window_x": null,
  "window_y": 20,
  "window_width": 1100,
  "window_height": 80,
  "font_family": "Segoe UI",
  "font_size_line1": 24.0,
  "font_size_line2": 16.0,
  "click_through": true,
  "time_offset_ms": 0,
  "tuna_port": 1608
}
```

---

## 🧪 Running Tests

To verify LRC parsing, timestamp calculation, and metadata filtering:

```powershell
cargo test
```