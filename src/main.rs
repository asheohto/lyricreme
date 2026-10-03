#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod audio;
mod config;
mod listener;
mod lrclib;
mod player;
mod ui;

use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};
use std::thread;

use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};

use crate::config::AppConfig;
use crate::listener::{TunaServer, TunaUpdate};
use crate::lrclib::LrcLibClient;
use crate::player::{LrcFetchRequest, PlayerState};
use crate::ui::OverlayWindow;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("==================================================");
    println!("  LyricReme - YouTube Music Desktop Lyrics Overlay ");
    println!("==================================================");

    std::panic::set_hook(Box::new(|info| {
        let msg = format!("LyricReme panicked: {}\n", info);
        eprintln!("{}", msg);
        if let Ok(appdata) = std::env::var("LOCALAPPDATA") {
            let path = std::path::PathBuf::from(appdata).join("LyricReme").join("crash.log");
            let _ = std::fs::write(path, &msg);
        }
        let _ = std::fs::write("crash.log", &msg);
    }));

    // Initialize COM for DirectWrite & Direct2D
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }

    let config = AppConfig::load_or_default();
    let player_state = Arc::new(Mutex::new(PlayerState::default()));

    // Channel for Tuna updates (from HTTP server)
    let (tuna_tx, tuna_rx) = channel::<TunaUpdate>();

    // Channel for LRCLIB fetch requests (to worker thread)
    let (fetch_tx, fetch_rx) = channel::<LrcFetchRequest>();

    // Start Tuna HTTP Server on port (default 1608)
    if let Err(e) = TunaServer::start(tuna_tx.clone(), config.tuna_port) {
        eprintln!("[LyricReme] Warning: {}", e);
        eprintln!("[LyricReme] If port 1608 is already in use, verify if another instance is running.");
    }

    // Start Windows System Media listener (GSMTC) for Spotify, Chrome/Edge/Firefox YouTube, Apple Music, etc.
    if let Err(e) = crate::listener::GsmtcListener::start(tuna_tx.clone()) {
        eprintln!("[LyricReme] Warning (GSMTC): {}", e);
    }

    // Spawn LRCLIB worker thread
    let player_state_for_lrc = Arc::clone(&player_state);
    thread::Builder::new()
        .name("lrclib_worker".into())
        .spawn(move || {
            let client = LrcLibClient::new();
            while let Ok(req) = fetch_rx.recv() {
                println!("[LyricReme] Fetching lyrics for: {} - {}", req.title, req.artist);
                let lyrics = client.get_lyrics(&req.artist, &req.title, req.duration_sec);
                if lyrics.is_some() {
                    println!("[LyricReme] Successfully loaded synced lyrics for '{}'", req.title);
                } else {
                    println!("[LyricReme] No lyrics found on LRCLIB for '{}'", req.title);
                }
                let mut state = player_state_for_lrc.lock().unwrap();
                // Ensure lyrics belong to the current song
                if state.current_track == req.title {
                    state.set_lyrics(lyrics.clone());
                }
                drop(state);

                // If lyrics were found and contain Japanese, fetch/convert Hiragana in background
                if let Some(ref lrc_text) = lyrics {
                    if crate::lrclib::hiragana::contains_japanese(lrc_text) {
                        println!("[LyricReme] Japanese lyrics detected for '{}', processing Hiragana...", req.title);
                        let hira_lyrics = client.get_hiragana_lyrics(&req.artist, &req.title, lrc_text);
                        if hira_lyrics.is_some() {
                            println!("[LyricReme] Successfully loaded Hiragana lyrics for '{}'", req.title);
                        }
                        let mut state = player_state_for_lrc.lock().unwrap();
                        if state.current_track == req.title {
                            state.set_hiragana_lyrics(hira_lyrics);
                        }
                    }
                }
            }
        })?;

    // Spawn Tuna updates handler thread
    let player_state_for_tuna = Arc::clone(&player_state);
    let fetch_tx_for_tuna = fetch_tx.clone();
    thread::Builder::new()
        .name("tuna_consumer".into())
        .spawn(move || {
            while let Ok(update) = tuna_rx.recv() {
                let mut state = player_state_for_tuna.lock().unwrap();
                state.update_from_tuna(update, &fetch_tx_for_tuna);
            }
        })?;

    // Spawn the WASAPI loopback level meter that drives the visualizer pulse.
    if let Err(e) = audio::spawn_level_capture(Arc::clone(&player_state)) {
        eprintln!("[LyricReme] Warning: {}", e);
    }

    println!("[LyricReme] Launching Overlay Window...");
    let ui_res = OverlayWindow::run(config, Arc::clone(&player_state));

    unsafe {
        CoUninitialize();
    }

    if let Err(e) = ui_res {
        eprintln!("[LyricReme] UI Error: {}", e);
        unsafe {
            use windows::core::{w, PCWSTR};
            use windows::Win32::Foundation::HWND;
            use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};
            let wide_msg = format!("LyricReme UI Error:\n{}\0", e).encode_utf16().collect::<Vec<u16>>();
            let _ = MessageBoxW(HWND(std::ptr::null_mut()), PCWSTR(wide_msg.as_ptr()), w!("LyricReme Error"), MB_ICONERROR | MB_OK);
        }
    }

    std::process::exit(0);
}
