use std::mem::zeroed;
use std::ptr::null_mut;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[link(name = "winmm")]
extern "system" {
    fn timeBeginPeriod(u_period: u32) -> u32;
    fn timeEndPeriod(u_period: u32) -> u32;
}

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    GetDC, GetDeviceCaps, ReleaseDC, HBRUSH, VREFRESH,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::ReleaseCapture;
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DestroyWindow,
    DispatchMessageW, GetCursorPos, GetSystemMetrics, GetWindowLongW,
    PeekMessageW, PostQuitMessage, RegisterClassW, SendMessageW, SetForegroundWindow,
    SetTimer, SetWindowLongW, SetWindowPos, ShowWindow, TrackPopupMenu, TranslateMessage,
    GWL_EXSTYLE, HTCAPTION, HWND_TOPMOST, MF_CHECKED, MF_GRAYED, MF_SEPARATOR, MF_STRING,
    MF_UNCHECKED, MSG, PM_REMOVE, SM_CXSCREEN, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW,
    SW_SHOWNOACTIVATE, TPM_BOTTOMALIGN, TPM_RIGHTBUTTON, WM_APP, WM_CLOSE, WM_COMMAND,
    WM_DESTROY, WM_EXITSIZEMOVE, WM_LBUTTONDOWN, WM_NCLBUTTONDOWN, WM_QUIT, WM_RBUTTONUP, WM_TIMER,
    WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
    WS_EX_TRANSPARENT, WS_POPUP, WS_VISIBLE,
};

use crate::config::AppConfig;
use crate::player::PlayerState;
use crate::ui::renderer::{OverlayRenderer, RenderLine};

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

pub fn get_monitor_refresh_rate() -> u32 {
    unsafe {
        let screen_dc = GetDC(HWND(null_mut()));
        let hz = GetDeviceCaps(screen_dc, VREFRESH);
        ReleaseDC(HWND(null_mut()), screen_dc);
        if hz >= 30 && hz <= 500 {
            hz as u32
        } else {
            60
        }
    }
}

/// Damped Harmonic Oscillator (Spring Physics)
/// Analytical underdamped formulation:
/// x(t) = 1.0 - exp(-zeta * omega_n * t) * (cos(omega_d * t) + (zeta * omega_n / omega_d) * sin(omega_d * t))
pub struct SpringOscillator {
    pub zeta: f32,       // Damping ratio (0.75 for crisp, tactile overshoot)
    pub omega_n: f32,    // Natural angular frequency (12.5 rad/s)
    pub duration_s: f32, // Settling duration (0.48s)
}

impl SpringOscillator {
    pub fn new() -> Self {
        Self {
            zeta: 0.75,
            omega_n: 12.5,
            duration_s: 0.48,
        }
    }

    /// Evaluates the spring progress factor at elapsed time `t` (seconds).
    /// Starts at 0.0 at t=0, oscillates with subtle physical overshoot, and settles at 1.0.
    pub fn evaluate(&self, t: f32) -> f32 {
        if t <= 0.0 {
            return 0.0;
        }
        if t >= self.duration_s {
            return 1.0;
        }
        let omega_d = self.omega_n * (1.0 - self.zeta * self.zeta).max(0.001).sqrt();
        let decay = (-self.zeta * self.omega_n * t).exp();
        let cos_term = (omega_d * t).cos();
        let sin_term = (omega_d * t).sin();
        let factor = self.zeta * self.omega_n / omega_d;

        1.0 - decay * (cos_term + factor * sin_term)
    }
}

/// Computes organic ambient floating offsets using dual-harmonic sinusoids.
/// Produces a calm, zero-gravity hovering drift.
pub fn organic_float_offsets(app_start: &Instant) -> (f32, f32) {
    let t = app_start.elapsed().as_secs_f64();
    // Line 1 floating: dual harmonic breathing (~3.6s cycle)
    let l1 = (t * 1.75).sin() * 1.5 + (t * 0.85).cos() * 0.5;
    // Line 2 floating: phase-shifted subtle drift (~4.2s cycle)
    let l2 = (t * 1.50 + 1.25).sin() * 1.2 + (t * 0.70 + 0.40).cos() * 0.4;
    (l1 as f32, l2 as f32)
}

pub struct AnimationState {
    pub current_line1: String,
    pub current_line2: String,
    pub old_line1: String,
    pub old_line2: String,
    pub transition_start: Option<Instant>,
    pub app_start: Instant,
    pub spring: SpringOscillator,
}

impl AnimationState {
    pub fn new() -> Self {
        let now = Instant::now();
        Self {
            current_line1: String::new(),
            current_line2: String::new(),
            old_line1: String::new(),
            old_line2: String::new(),
            transition_start: None,
            app_start: now,
            spring: SpringOscillator::new(),
        }
    }

    pub fn update(&mut self, next_line1: String, next_line2: String) -> bool {
        if next_line1 == self.current_line1 && next_line2 == self.current_line2 {
            return false;
        }

        self.old_line1 = std::mem::replace(&mut self.current_line1, next_line1);
        self.old_line2 = std::mem::replace(&mut self.current_line2, next_line2);
        self.transition_start = Some(Instant::now());
        true
    }

    #[allow(dead_code)]
    pub fn is_transitioning(&self) -> bool {
        if let Some(start) = self.transition_start {
            start.elapsed().as_secs_f32() < self.spring.duration_s
        } else {
            false
        }
    }
}

pub fn build_render_lines(anim: &AnimationState) -> Vec<RenderLine> {
    let mut lines = Vec::with_capacity(4);

    let (float1, float2) = organic_float_offsets(&anim.app_start);
    let l1_base_y = 6.0 + float1;
    let l2_base_y = 48.0 + float2;

    let elapsed = anim.transition_start.map(|t| t.elapsed().as_secs_f32()).unwrap_or(999.0);

    // If transition finished, render settled resting lines with ambient hover
    if elapsed >= anim.spring.duration_s {
        if !anim.current_line1.is_empty() {
            lines.push(RenderLine {
                text: anim.current_line1.clone(),
                y: l1_base_y,
                opacity: 1.0,
                is_active: true,
            });
        }
        if !anim.current_line2.is_empty() {
            lines.push(RenderLine {
                text: anim.current_line2.clone(),
                y: l2_base_y,
                opacity: 0.70,
                is_active: false,
            });
        }
        return lines;
    }

    let spring_val = anim.spring.evaluate(elapsed);
    // Smooth monotonic opacity fades (no oscillation in opacity)
    let fade_in = (elapsed / 0.22).min(1.0);
    let fade_out = (1.0 - elapsed / 0.20).max(0.0);

    // Line 1 transition with spring physics
    if anim.current_line1 == anim.old_line1 {
        if !anim.current_line1.is_empty() {
            lines.push(RenderLine {
                text: anim.current_line1.clone(),
                y: l1_base_y,
                opacity: 1.0,
                is_active: true,
            });
        }
    } else {
        // Outgoing Line 1 floats upwards and gently dissolves
        if !anim.old_line1.is_empty() {
            lines.push(RenderLine {
                text: anim.old_line1.clone(),
                y: l1_base_y - 18.0 * spring_val,
                opacity: fade_out,
                is_active: true,
            });
        }
        // Incoming Line 1 springs into place with tactile damped overshoot
        if !anim.current_line1.is_empty() {
            let spring_disp = (1.0 - spring_val) * 20.0;
            lines.push(RenderLine {
                text: anim.current_line1.clone(),
                y: l1_base_y + spring_disp,
                opacity: fade_in,
                is_active: true,
            });
        }
    }

    // Line 2 transition with spring physics
    if anim.current_line2 == anim.old_line2 {
        if !anim.current_line2.is_empty() {
            lines.push(RenderLine {
                text: anim.current_line2.clone(),
                y: l2_base_y,
                opacity: 0.70,
                is_active: false,
            });
        }
    } else {
        // Outgoing Line 2 fades away smoothly
        if !anim.old_line2.is_empty() {
            lines.push(RenderLine {
                text: anim.old_line2.clone(),
                y: l2_base_y - 10.0 * spring_val,
                opacity: fade_out * 0.70,
                is_active: false,
            });
        }
        // Incoming Line 2 springs into preview position from below
        if !anim.current_line2.is_empty() {
            let spring_disp2 = (1.0 - spring_val) * 14.0;
            lines.push(RenderLine {
                text: anim.current_line2.clone(),
                y: l2_base_y + spring_disp2,
                opacity: fade_in * 0.70,
                is_active: false,
            });
        }
    }

    lines
}

pub struct WindowContext {
    pub config: AppConfig,
    pub player_state: Arc<Mutex<PlayerState>>,
    pub renderer: OverlayRenderer,
    pub is_locked: bool,
    pub anim: AnimationState,
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
            let renderer = OverlayRenderer::new(
                width,
                height,
                &config.font_family,
                config.font_size_line1,
                config.font_size_line2,
            ).map_err(|e| format!("Renderer init failed: {}", e))?;

            let mut anim = AnimationState::new();
            let (l1, l2) = {
                let state = player_state.lock().unwrap();
                state.get_display_lyrics(config.time_offset_ms)
            };
            anim.current_line1 = l1;
            anim.current_line2 = l2;

            let context = Box::new(WindowContext {
                config,
                player_state,
                renderer,
                is_locked: true,
                anim,
            });

            GLOBAL_CONTEXT = Box::into_raw(context);

            add_tray_icon(hwnd);
            // Backup timer for modal dragging
            SetTimer(hwnd, TIMER_UPDATE_ID, 16, None);

            let ctx = &mut *GLOBAL_CONTEXT;
            let lines = build_render_lines(&ctx.anim);
            ctx.renderer.render_lines(hwnd, &lines, ctx.is_locked);

            let refresh_rate = get_monitor_refresh_rate();
            println!("[LyricReme] Detected native screen refresh rate: {} Hz", refresh_rate);
            let frame_target = Duration::from_secs_f64(1.0 / refresh_rate as f64);

            timeBeginPeriod(1);

            println!("[LyricReme] Entering high-refresh presentation loop ({} FPS)...", refresh_rate);
            let mut msg: MSG = zeroed();
            let mut is_running = true;
            let mut next_frame = Instant::now();

            while is_running {
                // Drain any pending Win32 messages (mouse events, tray menu, close) with zero latency
                while PeekMessageW(&mut msg, HWND(null_mut()), 0, 0, PM_REMOVE).as_bool() {
                    if msg.message == WM_QUIT {
                        is_running = false;
                        break;
                    }
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }

                if !is_running {
                    break;
                }

                if !GLOBAL_CONTEXT.is_null() {
                    let ctx = &mut *GLOBAL_CONTEXT;
                    let (line1, line2) = {
                        let state = ctx.player_state.lock().unwrap();
                        state.get_display_lyrics(ctx.config.time_offset_ms)
                    };

                    ctx.anim.update(line1, line2);

                    let lines = build_render_lines(&ctx.anim);
                    ctx.renderer.render_lines(hwnd, &lines, ctx.is_locked);
                }

                // Sub-millisecond precision pacing locked to exact display refresh rate
                next_frame += frame_target;
                let now = Instant::now();
                if next_frame > now {
                    let sleep_dur = next_frame - now;
                    if sleep_dur > Duration::from_millis(2) {
                        std::thread::sleep(sleep_dur - Duration::from_millis(1));
                    }
                    while Instant::now() < next_frame {
                        std::hint::spin_loop();
                    }
                } else if now - next_frame > frame_target {
                    next_frame = now;
                }
            }
            println!("[LyricReme] Exited presentation loop.");

            timeEndPeriod(1);
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
            // Backup update during modal window dragging
            if wparam.0 == TIMER_UPDATE_ID && !GLOBAL_CONTEXT.is_null() {
                let ctx = &mut *GLOBAL_CONTEXT;
                let (line1, line2) = {
                    let state = ctx.player_state.lock().unwrap();
                    state.get_display_lyrics(ctx.config.time_offset_ms)
                };

                ctx.anim.update(line1, line2);

                let lines = build_render_lines(&ctx.anim);
                ctx.renderer.render_lines(hwnd, &lines, ctx.is_locked);
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
            let lines = build_render_lines(&ctx.anim);
            ctx.renderer.render_lines(hwnd, &lines, ctx.is_locked);
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
            let lines = build_render_lines(&ctx.anim);
            ctx.renderer.render_lines(hwnd, &lines, ctx.is_locked);
        }
        IDM_OFFSET_PLUS => {
            ctx.config.time_offset_ms += 200;
            let _ = ctx.config.save();
            let (l1, l2) = {
                let state = ctx.player_state.lock().unwrap();
                state.get_display_lyrics(ctx.config.time_offset_ms)
            };
            ctx.anim.update(l1, l2);
            let lines = build_render_lines(&ctx.anim);
            ctx.renderer.render_lines(hwnd, &lines, ctx.is_locked);
        }
        IDM_OFFSET_MINUS => {
            ctx.config.time_offset_ms -= 200;
            let _ = ctx.config.save();
            let (l1, l2) = {
                let state = ctx.player_state.lock().unwrap();
                state.get_display_lyrics(ctx.config.time_offset_ms)
            };
            ctx.anim.update(l1, l2);
            let lines = build_render_lines(&ctx.anim);
            ctx.renderer.render_lines(hwnd, &lines, ctx.is_locked);
        }
        IDM_OFFSET_RESET => {
            ctx.config.time_offset_ms = 0;
            let _ = ctx.config.save();
            let (l1, l2) = {
                let state = ctx.player_state.lock().unwrap();
                state.get_display_lyrics(ctx.config.time_offset_ms)
            };
            ctx.anim.update(l1, l2);
            let lines = build_render_lines(&ctx.anim);
            ctx.renderer.render_lines(hwnd, &lines, ctx.is_locked);
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
    fn test_spring_oscillator() {
        let spring = SpringOscillator::new();
        let at_0 = spring.evaluate(0.0);
        assert_eq!(at_0, 0.0);

        let at_settle = spring.evaluate(0.60);
        assert_eq!(at_settle, 1.0);

        let at_overshoot = spring.evaluate(0.30);
        assert!(at_overshoot > 0.99);
    }

    #[test]
    fn test_organic_floating() {
        let start = Instant::now();
        let (f1, f2) = organic_float_offsets(&start);
        assert!(f1.abs() <= 2.5);
        assert!(f2.abs() <= 2.0);
    }

    #[test]
    fn test_animation_state() {
        let mut anim = AnimationState::new();
        assert!(!anim.is_transitioning());

        let changed = anim.update("Line 1".into(), "Line 2".into());
        assert!(changed);
        assert!(anim.is_transitioning());

        let lines = build_render_lines(&anim);
        assert!(!lines.is_empty());
    }
}
