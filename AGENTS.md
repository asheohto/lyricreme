# AGENTS.md

LyricReme: a Windows-only Rust desktop lyrics overlay (Win32 + Direct2D/DirectWrite). It listens for track updates from Pear Desktop's Tuna OBS plugin and fetches synced `.lrc` lyrics from LRCLIB. Single binary crate — no workspace.

## Commands

- Build: `cargo build --release` → `target\release\lyricreme.exe`
- Test: `cargo test` (unit tests only, in-module `#[cfg(test)]`; no integration suite)
- Single test: `cargo test <name>` (e.g. `cargo test test_parse_lrc_content`)

Windows target only: the `windows` crate imports and Win32 entrypoints do not compile on other platforms. There is no lint/format/CI config; `cargo fmt`/`cargo clippy` are not enforced.

## Runtime shape (not obvious from filenames)

`main.rs` wires four threads:
1. `listener::tuna_server` — HTTP server on `127.0.0.1:1608` (configurable) receiving POSTed JSON from Tuna OBS; sends `TunaUpdate` over an mpsc channel. Hardcoded to loopback; CORS `*`.
2. `lrclib_worker` — receives `LrcFetchRequest`, hits LRCLIB, writes back into shared `PlayerState`.
3. `audio` — WASAPI **loopback** level meter (see below), publishing into `PlayerState`.
4. `ui::window` — owns the Win32 message loop and renders the overlay.

Shared state is `Arc<Mutex<PlayerState>>` (`player/state.rs`). `PlayerState::update_from_tuna` interpolates playback position from the last update's `Instant`, so position is time-based, not polled.

- `PlayerState::get_display_lyrics` is the single source of truth for what the two lines show (idle / loading / not-found / loaded states).
- `ui/window.rs` uses a `static mut GLOBAL_CONTEXT: *mut WindowContext` raw pointer — the whole UI is `unsafe` and single-threaded. Do not "fix" this to a safe abstraction without understanding the message-loop ownership; `Drop`-based cleanup happens only after the loop exits.
- Rendering is layered-window alpha via `UpdateLayeredWindow` over a DIB section. Grayscale text antialiasing is **required** for correct per-pixel alpha — do not switch to ClearType.
- Frame pacing is adaptive: locked to the monitor refresh rate while animating, ~60 Hz polling when resting. `get_monitor_refresh_rate` reads `VREFRESH`. While a track is playing (and lyrics are loaded) the idle **wobble** keeps the loop in the animating branch, so it renders continuously; idle/paused is rock-solid still. The **visualizer pulse** joins that branch only while it is still easing toward the current audio level, so a silent/steady state falls back to resting.
- Line transitions in `build_render_lines_rot` use a smoothstep-eased cross-fade + directional slide (direction from `AnimationState::scroll_down`). `build_render_lines` is the zero-wobble wrapper used by one-off renders and tests.

## Gotchas

- **The visualizer's "music" signal is WASAPI loopback, not Tuna.** Tuna only POSTs metadata (`title`/`artists`/`duration`/`progress`/`status`) — never audio levels or spectrum. `audio/mod.rs` therefore opens the *default render endpoint* in shared loopback mode, RMS-sums the mix, and publishes 0–1 into `PlayerState::audio_level` (~50 Hz). It needs no new crate (just the `Win32_Media_Audio` feature) and no FFT — the pulse only needs loudness. It assumes 32-bit samples are float (`sample_format`, marked `ponytail:`); a 32-bit *integer* mix would read quiet, not crash. The thread retries every 2 s so a device change (or a cold boot with no endpoint) recovers.
- `assets/vibe.gif` is `include_bytes!`-embedded in `ui/renderer.rs` and decoded frame-by-frame to premultiplied BGRA with WIC (`IWICBitmapDecoder::GetFrameCount()` / `GetFrame()`), cached in a `OnceLock` so only the D2D uploads repeat per `rebuild`. In `draw_visualizer`, frames cycle smoothly based on elapsed time (~50fps / 20ms frame delay). **`include_bytes!` is outside `cargo`'s change tracking** — touching the GIF forces a *rebuild*, not just a relink; `build.rs` still only declares `logo.ico`.
- `assets/vibe.png` is `include_bytes!`-embedded in `ui/renderer.rs` and decoded to premultiplied BGRA with WIC (`Win32_Graphics_Imaging` + `SHCreateMemStream`), cached in a `OnceLock` so only the D2D upload repeats per `rebuild`. **`include_bytes!` is outside `cargo`'s change tracking** — touching the PNG forces a *rebuild*, not just a relink; `build.rs` still only declares `logo.ico`.
- The visualizer is drawn by `OverlayRenderer::draw_visualizer` in place of lyrics text when `LyricsStatus` is `NotFound` (aligned with the configured text position: center, left, or right). The pulse scale stays within `VISUALIZER_REST_SCALE..=VISUALIZER_REST_SCALE + VISUALIZER_GAIN` (0.52–1.00, `ui/window.rs`).
- `build.rs` embeds `assets/logo.ico` as icon resource ID 1 via `winres` (declared in `[build-dependencies]`). `LoadIconW(instance, PCWSTR(1))` in `ui/window.rs` picks it up for the tray icon, falling back to `IDI_APPLICATION` if absent. `winres` only runs its embedding path on Windows targets, which is fine since the crate is Windows-only.
- `assets/logo.ico` must stay **multi-resolution** (16/24/32/48/64 + 256). A single 256×256 PNG-compressed entry renders as a blank/blurry tray icon because the shell has no small size to pick. Regenerate with `powershell -File scripts/make-icon.ps1` (reads `assets/logo.png`); the script writes uncompressed 32bpp BMP entries for the small sizes plus a PNG entry for 256.
- `main.rs` uses `#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]`: release builds are windowless, debug builds keep the console for `println!`/`eprintln!` logs. In release, console output is discarded — the panic hook's `crash.log` and the `MessageBoxW` error dialog are the only diagnostics.
- Cache and config are read/written outside the repo: `%LOCALAPPDATA%\LyricReme\cache\*.lrc` and `%APPDATA%\LyricReme\config.json`. Delete these to test cold paths. Panics also write `crash.log` (cwd + `%LOCALAPPDATA%`).
- `AppConfig` appearance fields (`window_alpha`, `text_color`, `outline_color`, `outline_width`) and `position` all carry `#[serde(default)]`/`#[serde(default = "…")]` so old configs load without resetting. Any new field needs the same, or existing configs fail to deserialize and silently reset to defaults (covered by `config.rs` tests).
- `config.rs` position enum, tray menu command IDs, and `snap_to_position` are coupled: menu IDs run `IDM_POS_BASE..IDM_POS_BASE + LyricsPosition::all().len()`. The size/colour/outline/opacity submenus use `IDM_*_BASE` + index into the `SIZE_PRESETS` / `COLOR_PRESETS` / `OUTLINE_PRESETS` / `ALPHA_PRESETS` arrays — keep the array lengths in sync with the `*_COUNT` consts. Update `LyricsPosition::all()` and `label()` together.
- Global overlay opacity is applied via `BLENDFUNCTION.SourceConstantAlpha` in `renderer.rs`, **not** `SetLayeredWindowAttributes` — the two are mutually exclusive with `UpdateLayeredWindow`.
- `snap_to_position` resizes the window (no `SWP_NOSIZE`) so the text-size presets, which change `window_height`, actually apply.
- LRCLIB metadata is normalized by `lrclib::client::clean_metadata` (strips "(Official Video)", "[MV]", etc.) before the exact fetch, then falls back to `/api/search`. Cached files are keyed by sanitized artist_title; empty/whitespace cache entries are treated as misses.

## Conventions

- Tests assert on parser/timing/rendering math (`lrclib/parser.rs`, `lrclib/client.rs`, `ui/window.rs`, `ui/renderer.rs`). The renderer test creates a real Direct2D target, so it only passes on Windows with a display. `audio/mod.rs` tests are pure math over synthetic sample buffers; the renderer's `test_visualizer_asset_decodes_to_premultiplied_bgra` is what proves the embedded GIF + WIC path still works.
- Tests assert on parser/timing/rendering math (`lrclib/parser.rs`, `lrclib/client.rs`, `ui/window.rs`, `ui/renderer.rs`). The renderer test creates a real Direct2D target, so it only passes on Windows with a display. `audio/mod.rs` tests are pure math over synthetic sample buffers; the renderer's `test_visualizer_asset_decodes_to_premultiplied_bgra` is what proves the embedded PNG + WIC path still works.
- The repo has no `AGENTS.md` history; keep this file updated when wiring changes (threads, state flow, config schema).
- The `learn-from-mistakes` Cline skill (`.cline/skills/`) implements the Reflection loop: read `.agent-memory/lessons.jsonl` (gitignored) via `scripts/lessons.ps1` before non-trivial tasks, add one specific lesson after an objectively-verified failure (build/test error), reap stale lessons once covered by a passing test. See the skill's `docs/techniques.md` for why RL/self-play were rejected.