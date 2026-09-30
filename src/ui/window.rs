use std::mem::zeroed;
use std::ptr::null_mut;
use std::sync::{Arc, Mutex};

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::Graphics::Gdi::HBRUSH;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::ReleaseCapture;
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DestroyWindow,
    DispatchMessageW, GetCursorPos, GetMessageW, GetSystemMetrics, GetWindowLongW,
    PostQuitMessage, RegisterClassW, SendMessageW, SetForegroundWindow,
    SetTimer, SetWindowLongW, SetWindowPos, ShowWindow, TrackPopupMenu, TranslateMessage,
    GWL_EXSTYLE, HTCAPTION, HWND_TOPMOST, MF_CHECKED, MF_GRAYED, MF_SEPARATOR, MF_STRING,
    MF_UNCHECKED, MSG, SM_CXSCREEN, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW,
    SW_SHOWNOACTIVATE, TPM_BOTTOMALIGN, TPM_RIGHTBUTTON, WM_APP, WM_CLOSE, WM_COMMAND,
    WM_DESTROY, WM_EXITSIZEMOVE, WM_LBUTTONDOWN, WM_NCLBUTTONDOWN, WM_RBUTTONUP, WM_TIMER,
    WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
    WS_EX_TRANSPARENT, WS_POPUP, WS_VISIBLE,
};

use crate::config::AppConfig;
use crate::player::PlayerState;
use crate::ui::renderer::OverlayRenderer;

const WM_TRAYICON: u32 = WM_APP + 1;
const TIMER_UPDATE_ID: usize = 1001;

// Menu Command IDs
const IDM_TOGGLE_CLICKTHROUGH: usize = 2001;
const IDM_TOGGLE_LOCK: usize = 2002;
const IDM_RESET_POSITION: usize = 2003;
const IDM_OFFSET_PLUS: usize = 2004;
const IDM_OFFSET_MINUS: usize = 2005;
const IDM_OFFSET_RESET: usize = 2006;
const IDM_EXIT: usize = 2007;

pub struct WindowContext {
    pub config: AppConfig,
    pub player_state: Arc<Mutex<PlayerState>>,
    pub renderer: OverlayRenderer,
    pub is_locked: bool,
    pub last_line1: String,
    pub last_line2: String,
}

static mut GLOBAL_CONTEXT: *mut WindowContext = null_mut();

pub struct OverlayWindow;

impl OverlayWindow {
    pub fn run(
        config: AppConfig,
        player_state: Arc<Mutex<PlayerState>>,
    ) -> Result<(), String> {
        unsafe {
            let instance = GetModuleHandleW(None).map_err(|e| format!("GetModuleHandle failed: {}", e))?;
            let class_name = w!("LyricRemeOverlayClass");

            let wc = WNDCLASSW {
                lpfnWndProc: Some(window_proc),
                hInstance: instance.into(),
                lpszClassName: class_name,
                hbrBackground: HBRUSH(null_mut()),
                ..zeroed()
            };

            println!("[LyricReme] Registering window class...");
            let reg = RegisterClassW(&wc);
            println!("[LyricReme] RegisterClass result: {}", reg);

            let screen_width = GetSystemMetrics(SM_CXSCREEN);
            let width = config.window_width;
            let height = config.window_height;
            let x = config.window_x.unwrap_or_else(|| (screen_width - width) / 2);
            let y = config.window_y;
            println!("[LyricReme] Target window pos: x={}, y={}, w={}, h={}", x, y, width, height);

            let mut ex_style = WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE;
            if config.click_through {
                ex_style |= WS_EX_TRANSPARENT;
            }

            println!("[LyricReme] Calling CreateWindowExW...");
            let hwnd = CreateWindowExW(
                ex_style,
                class_name,
                w!("LyricReme"),
                WS_POPUP | WS_VISIBLE,
                x,
                y,
                width,
                height,
                HWND(null_mut()),
                None,
                instance,
                None,
            ).map_err(|e| format!("CreateWindowExW failed: {}", e))?;
            println!("[LyricReme] CreateWindowExW succeeded: {:?}", hwnd);

            let _ = SetWindowPos(
                hwnd,
                HWND_TOPMOST,
                x,
                y,
                width,
                height,
                SWP_SHOWWINDOW | SWP_NOACTIVATE,
            );
            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);

            println!("[LyricReme] Initializing Direct2D renderer...");
            let renderer = OverlayRenderer::new(width, height, &config.font_family)
                .map_err(|e| format!("Renderer init failed: {}", e))?;

            let context = Box::new(WindowContext {
                config,
                player_state,
                renderer,
                is_locked: true,
                last_line1: String::new(),
                last_line2: String::new(),
            });

            GLOBAL_CONTEXT = Box::into_raw(context);

            add_tray_icon(hwnd);
            SetTimer(hwnd, TIMER_UPDATE_ID, 33, None);

            let ctx = &mut *GLOBAL_CONTEXT;
            let (l1, l2) = {
                let state = ctx.player_state.lock().unwrap();
                state.get_display_lyrics(ctx.config.time_offset_ms)
            };
            println!("[LyricReme] Performing initial render (l1: '{}', l2: '{}')...", l1, l2);
            ctx.renderer.render(hwnd, &l1, &l2);
            ctx.last_line1 = l1;
            ctx.last_line2 = l2;

            println!("[LyricReme] Entering Win32 message loop...");
            let mut msg: MSG = zeroed();
            while GetMessageW(&mut msg, HWND(null_mut()), 0, 0).into() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            println!("[LyricReme] Exited message loop.");

            remove_tray_icon(hwnd);
            if !GLOBAL_CONTEXT.is_null() {
                drop(Box::from_raw(GLOBAL_CONTEXT));
                GLOBAL_CONTEXT = null_mut();
            }

            Ok(())
        }
    }
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_TIMER => {
            if wparam.0 == TIMER_UPDATE_ID && !GLOBAL_CONTEXT.is_null() {
                let ctx = &mut *GLOBAL_CONTEXT;
                let (line1, line2) = {
                    let state = ctx.player_state.lock().unwrap();
                    state.get_display_lyrics(ctx.config.time_offset_ms)
                };

                if line1 != ctx.last_line1 || line2 != ctx.last_line2 {
                    ctx.renderer.render(hwnd, &line1, &line2);
                    ctx.last_line1 = line1;
                    ctx.last_line2 = line2;
                }
            }
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            if !GLOBAL_CONTEXT.is_null() {
                let ctx = &*GLOBAL_CONTEXT;
                if !ctx.is_locked {
                    let _ = ReleaseCapture();
                    SendMessageW(hwnd, WM_NCLBUTTONDOWN, WPARAM(HTCAPTION as usize), LPARAM(0));
                }
            }
            LRESULT(0)
        }
        WM_EXITSIZEMOVE => {
            if !GLOBAL_CONTEXT.is_null() {
                let ctx = &mut *GLOBAL_CONTEXT;
                let mut rect = zeroed();
                if windows::Win32::UI::WindowsAndMessaging::GetWindowRect(hwnd, &mut rect).is_ok() {
                    ctx.config.window_x = Some(rect.left);
                    ctx.config.window_y = rect.top;
                    let _ = ctx.config.save();
                }
            }
            LRESULT(0)
        }
        WM_TRAYICON => {
            let event = (lparam.0 & 0xffff) as u32;
            if event == WM_RBUTTONUP {
                show_tray_menu(hwnd);
            }
            LRESULT(0)
        }
        WM_COMMAND => {
            let cmd_id = wparam.0 & 0xffff;
            handle_menu_command(hwnd, cmd_id);
            LRESULT(0)
        }
        WM_CLOSE => {
            let _ = DestroyWindow(hwnd);
            LRESULT(0)
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

unsafe fn add_tray_icon(hwnd: HWND) {
    let mut nid: NOTIFYICONDATAW = zeroed();
    nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
    nid.hWnd = hwnd;
    nid.uID = 1;
    nid.uFlags = NIF_MESSAGE | NIF_TIP;
    nid.uCallbackMessage = WM_TRAYICON;

    let tip = w!("LyricReme - YouTube Music Lyrics Overlay");
    let tip_slice = tip.as_wide();
    let copy_len = tip_slice.len().min(nid.szTip.len() - 1);
    nid.szTip[..copy_len].copy_from_slice(&tip_slice[..copy_len]);

    let _ = Shell_NotifyIconW(NIM_ADD, &nid);
}

unsafe fn remove_tray_icon(hwnd: HWND) {
    let mut nid: NOTIFYICONDATAW = zeroed();
    nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
    nid.hWnd = hwnd;
    nid.uID = 1;
    let _ = Shell_NotifyIconW(NIM_DELETE, &nid);
}

unsafe fn show_tray_menu(hwnd: HWND) {
    if GLOBAL_CONTEXT.is_null() {
        return;
    }
    let ctx = &*GLOBAL_CONTEXT;

    let hmenu = match CreatePopupMenu() {
        Ok(m) => m,
        Err(_) => return,
    };

    let title_str = "LyricReme v0.1.0\0".encode_utf16().collect::<Vec<u16>>();
    let _ = AppendMenuW(hmenu, MF_STRING | MF_GRAYED, 0, PCWSTR(title_str.as_ptr()));
    let _ = AppendMenuW(hmenu, MF_SEPARATOR, 0, PCWSTR(null_mut()));

    let ct_flags = MF_STRING | if ctx.config.click_through { MF_CHECKED } else { MF_UNCHECKED };
    let _ = AppendMenuW(hmenu, ct_flags, IDM_TOGGLE_CLICKTHROUGH, w!("Click-Through (Transparent clicks)"));

    let lock_flags = MF_STRING | if ctx.is_locked { MF_CHECKED } else { MF_UNCHECKED };
    let _ = AppendMenuW(hmenu, lock_flags, IDM_TOGGLE_LOCK, w!("Lock Position (Drag to move)"));

    let _ = AppendMenuW(hmenu, MF_STRING, IDM_RESET_POSITION, w!("Reset to Top Center"));
    let _ = AppendMenuW(hmenu, MF_SEPARATOR, 0, PCWSTR(null_mut()));

    let offset_label = format!("Offset: {:+.1}s\0", ctx.config.time_offset_ms as f64 / 1000.0)
        .encode_utf16()
        .collect::<Vec<u16>>();
    let _ = AppendMenuW(hmenu, MF_STRING | MF_GRAYED, 0, PCWSTR(offset_label.as_ptr()));
    let _ = AppendMenuW(hmenu, MF_STRING, IDM_OFFSET_PLUS, w!("Nudge Forward (+0.2s)"));
    let _ = AppendMenuW(hmenu, MF_STRING, IDM_OFFSET_MINUS, w!("Nudge Backward (-0.2s)"));
    let _ = AppendMenuW(hmenu, MF_STRING, IDM_OFFSET_RESET, w!("Reset Offset (0.0s)"));

    let _ = AppendMenuW(hmenu, MF_SEPARATOR, 0, PCWSTR(null_mut()));
    let _ = AppendMenuW(hmenu, MF_STRING, IDM_EXIT, w!("Exit LyricReme"));

    let mut pt: POINT = zeroed();
    let _ = GetCursorPos(&mut pt);
    let _ = SetForegroundWindow(hwnd);
    let _ = TrackPopupMenu(hmenu, TPM_RIGHTBUTTON | TPM_BOTTOMALIGN, pt.x, pt.y, 0, hwnd, None);
    let _ = DestroyMenu(hmenu);
}

unsafe fn handle_menu_command(hwnd: HWND, cmd_id: usize) {
    if GLOBAL_CONTEXT.is_null() {
        return;
    }
    let ctx = &mut *GLOBAL_CONTEXT;

    match cmd_id {
        IDM_TOGGLE_CLICKTHROUGH => {
            ctx.config.click_through = !ctx.config.click_through;
            let mut style = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
            if ctx.config.click_through {
                style |= WS_EX_TRANSPARENT.0;
            } else {
                style &= !WS_EX_TRANSPARENT.0;
            }
            SetWindowLongW(hwnd, GWL_EXSTYLE, style as i32);
            let _ = SetWindowPos(hwnd, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
            let _ = ctx.config.save();
        }
        IDM_TOGGLE_LOCK => {
            ctx.is_locked = !ctx.is_locked;
            let mut style = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
            if ctx.is_locked && ctx.config.click_through {
                style |= WS_EX_TRANSPARENT.0;
            } else {
                style &= !WS_EX_TRANSPARENT.0;
            }
            SetWindowLongW(hwnd, GWL_EXSTYLE, style as i32);
            let _ = SetWindowPos(hwnd, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
        }
        IDM_RESET_POSITION => {
            let screen_width = GetSystemMetrics(SM_CXSCREEN);
            let x = (screen_width - ctx.config.window_width) / 2;
            let y = 20;
            ctx.config.window_x = Some(x);
            ctx.config.window_y = y;
            let _ = SetWindowPos(
                hwnd,
                HWND_TOPMOST,
                x,
                y,
                ctx.config.window_width,
                ctx.config.window_height,
                SWP_NOACTIVATE,
            );
            let _ = ctx.config.save();
        }
        IDM_OFFSET_PLUS => {
            ctx.config.time_offset_ms += 200;
            let _ = ctx.config.save();
        }
        IDM_OFFSET_MINUS => {
            ctx.config.time_offset_ms -= 200;
            let _ = ctx.config.save();
        }
        IDM_OFFSET_RESET => {
            ctx.config.time_offset_ms = 0;
            let _ = ctx.config.save();
        }
        IDM_EXIT => {
            let _ = DestroyWindow(hwnd);
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_window_creation() {
        unsafe {
            let instance = GetModuleHandleW(None).unwrap();
            let class_name = w!("LyricRemeTestClass");
            let wc = WNDCLASSW {
                lpfnWndProc: Some(window_proc),
                hInstance: instance.into(),
                lpszClassName: class_name,
                hbrBackground: HBRUSH(null_mut()),
                ..zeroed()
            };
            let _ = RegisterClassW(&wc);
            let hwnd = CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                class_name,
                w!("LyricRemeTest"),
                WS_POPUP | windows::Win32::UI::WindowsAndMessaging::WS_VISIBLE,
                100,
                100,
                500,
                80,
                HWND(null_mut()),
                None,
                instance,
                None,
            );
            match hwnd {
                Ok(h) => {
                    println!("Window created successfully: {:?}", h);
                    let _ = DestroyWindow(h);
                }
                Err(e) => panic!("CreateWindowExW failed: {:?}", e),
            }
        }
    }
}
