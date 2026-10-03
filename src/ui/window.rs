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
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    GetDC, GetDeviceCaps, ReleaseDC, HBRUSH, VREFRESH,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::ReleaseCapture;
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DestroyWindow,
    DispatchMessageW, GetCursorPos, GetSystemMetrics, GetWindowLongW, GetWindowRect,
    LoadIconW, PeekMessageW, PostQuitMessage, RegisterClassW, SendMessageW, SetForegroundWindow,
    SetTimer, SetWindowLongW, SetWindowPos, ShowWindow, TrackPopupMenu, TranslateMessage,
    GWL_EXSTYLE, HTCAPTION, HWND_TOPMOST, IDI_APPLICATION,
    MF_CHECKED, MF_GRAYED, MF_POPUP, MF_SEPARATOR, MF_STRING,
    MF_UNCHECKED, MSG, PM_REMOVE, SM_CXSCREEN, SM_CYSCREEN,
    SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW,
    SW_SHOWNOACTIVATE, TPM_BOTTOMALIGN, TPM_RIGHTBUTTON, WM_APP, WM_CLOSE, WM_COMMAND,
    WM_DESTROY, WM_EXITSIZEMOVE, WM_LBUTTONDOWN, WM_NCLBUTTONDOWN, WM_QUIT, WM_RBUTTONUP, WM_TIMER,
    WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
    WS_EX_TRANSPARENT, WS_POPUP, WS_VISIBLE,
};

use crate::config::{AppConfig, LyricsPosition, COLOR_PRESETS};
use crate::player::{LyricsStatus, PlayerState};
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
const IDM_TOGGLE_VISUALIZER: usize = 2008;
const IDM_TOGGLE_HIRAGANA: usize = 2009;
const IDM_TOGGLE_ROMAJI: usize = 2010;
// Position snap commands (one per LyricsPosition variant, 2100–2108)
const IDM_POS_BASE: usize = 2100;
// Text size commands (Small → XL, 2200–2204)
const IDM_SIZE_BASE: usize = 2200;
const SIZE_COUNT: usize = 5;
// Text colour presets (one per COLOR_PRESETS entry, 2300+)
const IDM_COLOR_BASE: usize = 2300;
// Outline presets (None / Thin / Medium / Thick, 2400–2403)
const IDM_OUTLINE_BASE: usize = 2400;
const OUTLINE_COUNT: usize = 4;
// Opacity presets (40% … 100%, 2500–2503)
const IDM_ALPHA_BASE: usize = 2500;
const ALPHA_COUNT: usize = 4;

/// Font size multipliers for the "Text Size" submenu.
const SIZE_PRESETS: [(&str, f32); SIZE_COUNT] = [
    ("Small", 0.80),
    ("Normal", 1.00),
    ("Large", 1.25),
    ("Extra Large", 1.55),
    ("Huge", 1.90),
];

/// Outline width presets for the "Outline" submenu.
const OUTLINE_PRESETS: [(&str, f32); OUTLINE_COUNT] = [
    ("None", 0.0),
    ("Thin", 1.0),
    ("Medium", 2.0),
    ("Thick", 3.5),
];

/// Overlay opacity presets for the "Opacity" submenu (percent).
const ALPHA_PRESETS: [(&str, u8); ALPHA_COUNT] = [
    ("40%", 40),
    ("60%", 60),
    ("80%", 80),
    ("100%", 100),
];

// Base layout metrics at 100% text size. Scaling multiplies these so the two
// lyric lines stay proportionally spaced at any size.
const BASE_FONT_LINE1: f32 = 24.0;
const BASE_FONT_LINE2: f32 = 16.0;
const BASE_WINDOW_HEIGHT: i32 = 170;
const BASE_L1_Y: f32 = 12.0;
const BASE_L2_Y: f32 = 56.0;

/// Visualizer pulse: fraction of the art's full slot held at rest (silence) and
/// the extra fraction added at full volume. The two must sum to 1.0 so the peak
/// still fits the window height (see `OverlayRenderer::draw_visualizer`).
pub const VISUALIZER_REST_SCALE: f32 = 0.55;
pub const VISUALIZER_GAIN: f32 = 0.45;

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
    pub zeta: f32,       // Damping ratio (0.75 for crisp tactile overshoot)
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HoverZone {
    Far,    // Normal full opacity (1.0)
    Near,   // Near the overlay area (0.70)
    Nearer, // Very close or inside overlay area (0.40)
}

impl HoverZone {
    pub fn opacity_factor(&self) -> f32 {
        match self {
            HoverZone::Far => 1.0,
            HoverZone::Near => 0.70,
            HoverZone::Nearer => 0.40,
        }
    }

    /// Evaluates hover zone based on Euclidean distance with hysteresis
    /// to prevent stuttering near boundaries.
    pub fn update(&self, dist: f32) -> Self {
        match self {
            HoverZone::Far => {
                if dist <= 45.0 {
                    HoverZone::Nearer
                } else if dist <= 150.0 {
                    HoverZone::Near
                } else {
                    HoverZone::Far
                }
            }
            HoverZone::Near => {
                if dist <= 45.0 {
                    HoverZone::Nearer
                } else if dist > 165.0 {
                    HoverZone::Far
                } else {
                    HoverZone::Near
                }
            }
            HoverZone::Nearer => {
                if dist > 165.0 {
                    HoverZone::Far
                } else if dist > 55.0 {
                    HoverZone::Near
                } else {
                    HoverZone::Nearer
                }
            }
        }
    }
}

pub fn calculate_cursor_distance(pt: POINT, rect: RECT) -> f32 {
    let dx = if pt.x < rect.left {
        rect.left - pt.x
    } else if pt.x > rect.right {
        pt.x - rect.right
    } else {
        0
    };

    let dy = if pt.y < rect.top {
        rect.top - pt.y
    } else if pt.y > rect.bottom {
        pt.y - rect.bottom
    } else {
        0
    };

    ((dx * dx + dy * dy) as f32).sqrt()
}

pub struct OpacityFader {
    pub current_opacity: f32,
    pub target_opacity: f32,
    pub fade_from: f32,
    pub fade_start: Option<Instant>,
    pub duration_s: f32,
}

impl OpacityFader {
    pub fn new() -> Self {
        Self {
            current_opacity: 1.0,
            target_opacity: 1.0,
            fade_from: 1.0,
            fade_start: None,
            duration_s: 0.22,
        }
    }

    pub fn set_target(&mut self, target: f32, duration_s: f32) -> bool {
        if (target - self.target_opacity).abs() > 0.005 {
            self.fade_from = self.current_opacity;
            self.target_opacity = target;
            self.duration_s = duration_s;
            self.fade_start = Some(Instant::now());
            true
        } else {
            false
        }
    }

    /// Evaluates current opacity and returns true if still actively fading.
    pub fn update(&mut self) -> bool {
        if let Some(start) = self.fade_start {
            let elapsed = start.elapsed().as_secs_f32();
            if elapsed >= self.duration_s {
                self.current_opacity = self.target_opacity;
                self.fade_start = None;
                false
            } else {
                let p = (elapsed / self.duration_s).clamp(0.0, 1.0);
                // Hermite smoothstep ease-in-out: 3p^2 - 2p^3
                let smooth = p * p * (3.0 - 2.0 * p);
                self.current_opacity = self.fade_from + (self.target_opacity - self.fade_from) * smooth;
                true
            }
        } else {
            false
        }
    }

    pub fn is_fading(&self) -> bool {
        self.fade_start.is_some()
    }
}

pub struct AnimationState {
    pub current_line1: String,
    pub current_line2: String,
    pub old_line1: String,
    pub old_line2: String,
    pub transition_start: Option<Instant>,
    pub spring: SpringOscillator,
    pub fader: OpacityFader,
    pub status_target: f32,
    pub hover_zone: HoverZone,
    /// Elapsed wall-clock since the app started, used as the wobble phase clock.
    pub wobble_clock: Instant,
    /// When true, the line that just arrived slides down from above (previous line
    /// scrolled up) instead of up from below. Set from the playback direction.
    pub scroll_down: bool,
    /// Current line-1 font size; drives layout scale (set from config, updated on size change).
    pub font_size_line1: f32,
    /// Current visualizer pulse scale, chased toward the audio level each frame.
    pub visualizer_scale: f32,
}

impl AnimationState {
    pub fn new() -> Self {
        Self {
            current_line1: String::new(),
            current_line2: String::new(),
            old_line1: String::new(),
            old_line2: String::new(),
            transition_start: None,
            spring: SpringOscillator::new(),
            fader: OpacityFader::new(),
            status_target: 1.0,
            hover_zone: HoverZone::Far,
            wobble_clock: Instant::now(),
            scroll_down: false,
            font_size_line1: BASE_FONT_LINE1,
            visualizer_scale: VISUALIZER_REST_SCALE,
        }
    }

    pub fn update(&mut self, next_line1: String, next_line2: String) -> bool {
        if next_line1 == self.current_line1 && next_line2 == self.current_line2 {
            return false;
        }

        // Direction of travel: if the new line was the old preview (line2 → line1),
        // the lyrics advanced and content scrolls upward, so the incoming line
        // should rise from below. Otherwise (seek/offset jump) keep the last direction.
        if !self.current_line2.is_empty() && next_line1 == self.current_line2 {
            self.scroll_down = false;
        } else if !next_line2.is_empty() && self.current_line1 == next_line2 {
            self.scroll_down = true;
        }

        self.old_line1 = std::mem::replace(&mut self.current_line1, next_line1);
        self.old_line2 = std::mem::replace(&mut self.current_line2, next_line2);
        self.transition_start = Some(Instant::now());
        true
    }

    pub fn update_targets(&mut self, status: &LyricsStatus, hover_factor: f32) -> bool {
        match status {
            LyricsStatus::NotFound => self.status_target = 1.0,
            LyricsStatus::Loaded => self.status_target = 1.0,
            LyricsStatus::Idle => self.status_target = 1.0,
            LyricsStatus::Loading => {}
        }

        let combined = self.status_target * hover_factor;
        self.fader.set_target(combined, 0.22)
    }

    pub fn is_animating(&self, wobble_active: bool) -> bool {
        let spring_active = if let Some(start) = self.transition_start {
            start.elapsed().as_secs_f32() < self.spring.duration_s
        } else {
            false
        };
        spring_active || self.fader.is_fading() || wobble_active
    }

    /// Wobble is a subtle idle bob applied to the active lyric while music plays.
    /// Returns the rotation (degrees) for line 1 and line 2 at the current phase.
    pub fn wobble_rotations(&self, wobble_active: bool) -> (f32, f32) {
        if !wobble_active {
            return (0.0, 0.0);
        }
        let t = self.wobble_clock.elapsed().as_secs_f32();
        // Two slightly detuned sines so the pair breathes rather than rotating in lockstep.
        let l1 = 0.9 * (t * 0.55 * std::f32::consts::TAU * 0.5).sin();
        let l2 = 0.6 * (t * 0.42 * std::f32::consts::TAU * 0.5).sin();
        (l1, l2)
    }

    /// Eases the visualizer art toward the level captured from the audio mix.
    ///
    /// The level is a coarse (~20 ms) WASAPI RMS sample, so a light chase (which
    /// a frame-rate independent exponential still gives) turns it into motion:
    /// quick attack while rising (the beat should land) and a slower decay once
    /// the level drops.
    /// Returns the new pulse scale.
    pub fn update_visualizer(&mut self, audio_level: f32, dt: f32) -> f32 {
        let punchy_level = (audio_level * 1.25).clamp(0.0, 1.0);
        let target = VISUALIZER_REST_SCALE + VISUALIZER_GAIN * punchy_level;
        let rate = if target > self.visualizer_scale { 160.0 } else { 28.0 };
        // Frame-rate independent exponential ease (dt = 0 would otherwise freeze it).
        let blend = 1.0 - (-rate * dt.max(0.0)).exp();
        self.visualizer_scale += (target - self.visualizer_scale) * blend;
        self.visualizer_scale
    }
}

pub fn build_render_lines(anim: &AnimationState) -> Vec<RenderLine> {
    // Wobble is driven by the caller through `wobble_rotations`; this function only
    // lays out positions/opacities so it stays a pure, testable transform.
    build_render_lines_rot(anim, 0.0, 0.0)
}

/// Same as [`build_render_lines`] but with explicit wobble rotations (degrees) for
/// the active line and the preview line.
pub fn build_render_lines_rot(anim: &AnimationState, rot1: f32, rot2: f32) -> Vec<RenderLine> {
    let mut lines = Vec::with_capacity(4);

    // Scale line spacing with the configured font size so bigger text doesn't overlap.
    let scale = (anim.font_size_line1 / BASE_FONT_LINE1).max(0.1);
    let l1_y = BASE_L1_Y * scale;
    let l2_y = BASE_L2_Y * scale;
    // Vertical travel of the slide during a transition (px).
    let slide = 22.0 * scale;

    let master_opacity = anim.fader.current_opacity.clamp(0.0, 1.0);

    let elapsed = anim.transition_start.map(|t| t.elapsed().as_secs_f32()).unwrap_or(999.0);

    // If transition finished, render settled resting lines with only the idle wobble.
    if elapsed >= anim.spring.duration_s {
        if !anim.current_line1.is_empty() {
            lines.push(RenderLine {
                text: anim.current_line1.clone(),
                y: l1_y,
                opacity: 1.0 * master_opacity,
                is_active: true,
                rotation_deg: rot1,
            });
        }
        if !anim.current_line2.is_empty() {
            lines.push(RenderLine {
                text: anim.current_line2.clone(),
                y: l2_y,
                opacity: 0.70 * master_opacity,
                is_active: false,
                rotation_deg: rot2,
            });
        }
        return lines;
    }

    // Eased 0..1 progress over the transition duration. Smoothstep removes the
    // velocity kink the old spring-displacement had at both ends, which is what
    // made the slide read as a jerk rather than a glide.
    let p = (elapsed / anim.spring.duration_s).clamp(0.0, 1.0);
    let eased = p * p * (3.0 - 2.0 * p);
    // Overshoot only near the tail, scaled down from the spring so it's tactile,
    // not bouncy: peak displacement of the incoming line ends at ~3px past rest.
    let spring_val = anim.spring.evaluate(elapsed);
    let glide = eased + (spring_val - eased) * 0.35;

    // Lyric scroll: on advance (scroll_down = false) the incoming line rises from
    // below while the outgoing line slides up and out. A backward seek mirrors it:
    // incoming enters from above and the outgoing line slides down.
    let dir = if anim.scroll_down { -1.0 } else { 1.0 };
    let enter_disp = (1.0 - glide) * slide * dir;
    let exit_disp = -glide * slide * dir;

    // Opacities cross-fade on the same eased curve so nothing pops.
    let fade_in = eased;
    let fade_out = 1.0 - eased;

    // Line 1 transition
    if anim.current_line1 == anim.old_line1 {
        if !anim.current_line1.is_empty() {
            lines.push(RenderLine {
                text: anim.current_line1.clone(),
                y: l1_y,
                opacity: 1.0 * master_opacity,
                is_active: true,
                rotation_deg: rot1,
            });
        }
    } else {
        if !anim.old_line1.is_empty() {
            lines.push(RenderLine {
                text: anim.old_line1.clone(),
                y: l1_y + exit_disp,
                opacity: fade_out * master_opacity,
                is_active: true,
                rotation_deg: 0.0,
            });
        }
        if !anim.current_line1.is_empty() {
            lines.push(RenderLine {
                text: anim.current_line1.clone(),
                y: l1_y + enter_disp,
                opacity: fade_in * master_opacity,
                is_active: true,
                rotation_deg: 0.0,
            });
        }
    }

    // Line 2 transition
    if anim.current_line2 == anim.old_line2 {
        if !anim.current_line2.is_empty() {
            lines.push(RenderLine {
                text: anim.current_line2.clone(),
                y: l2_y,
                opacity: 0.70 * master_opacity,
                is_active: false,
                rotation_deg: rot2,
            });
        }
    } else {
        if !anim.old_line2.is_empty() {
            lines.push(RenderLine {
                text: anim.old_line2.clone(),
                y: l2_y + exit_disp * 0.6,
                opacity: fade_out * 0.70 * master_opacity,
                is_active: false,
                rotation_deg: 0.0,
            });
        }
        if !anim.current_line2.is_empty() {
            lines.push(RenderLine {
                text: anim.current_line2.clone(),
                y: l2_y + enter_disp * 0.6,
                opacity: fade_in * 0.70 * master_opacity,
                is_active: false,
                rotation_deg: 0.0,
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
            let mut renderer = OverlayRenderer::new(
                width,
                height,
                &config.font_family,
                config.font_size_line1,
                config.font_size_line2,
                config.text_color,
                config.outline_color,
                config.outline_width,
            ).map_err(|e| format!("Renderer init failed: {}", e))?;
            renderer.set_master_alpha(config.window_alpha);

            let mut anim = AnimationState::new();
            let (l1, l2, _init_status) = {
                let state = player_state.lock().unwrap();
                let (first1, first2) = state.get_display_lyrics(config.time_offset_ms, config.show_romaji, config.show_hiragana);
                (first1, first2, state.lyrics_status.clone())
            };
            anim.current_line1 = l1;
            anim.current_line2 = l2;
            anim.font_size_line1 = config.font_size_line1;

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
            ctx.renderer.render_lines(hwnd, &lines, ctx.is_locked, ctx.config.position.text_alignment());

            let refresh_rate = get_monitor_refresh_rate();
            println!("[LyricReme] Detected native screen refresh rate: {} Hz", refresh_rate);
            let frame_target = Duration::from_secs_f64(1.0 / refresh_rate as f64);

            timeBeginPeriod(1);

            println!("[LyricReme] Entering native presentation loop ({} FPS transitions)...", refresh_rate);
            let mut msg: MSG = zeroed();
            let mut is_running = true;
            let mut next_frame = Instant::now();
            let mut last_frame_time = Instant::now();
            let mut needs_render = true;

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

                let now = Instant::now();
                let frame_dt = now.duration_since(last_frame_time).as_secs_f32().clamp(0.001, 0.1);
                last_frame_time = now;

                let is_animating = if !GLOBAL_CONTEXT.is_null() {
                    let ctx = &mut *GLOBAL_CONTEXT;
                    let (line1, line2, status, is_playing, audio_level) = {
                        let state = ctx.player_state.lock().unwrap();
                        let (l1, l2) = state.get_display_lyrics(ctx.config.time_offset_ms, ctx.config.show_romaji, ctx.config.show_hiragana);
                        (l1, l2, state.lyrics_status.clone(), state.is_playing, state.audio_level)
                    };

                    // --- Cursor proximity hover dimming ---
                    let mut cursor_pt: POINT = zeroed();
                    let _ = GetCursorPos(&mut cursor_pt);
                    let mut win_rect: RECT = zeroed();
                    let _ = GetWindowRect(hwnd, &mut win_rect);
                    let dist = calculate_cursor_distance(cursor_pt, win_rect);
                    let new_zone = ctx.anim.hover_zone.update(dist);
                    let hover_changed = new_zone != ctx.anim.hover_zone;
                    if hover_changed {
                        ctx.anim.hover_zone = new_zone;
                    }
                    let hover_factor = ctx.anim.hover_zone.opacity_factor();

                    let target_changed = ctx.anim.update_targets(&status, hover_factor);
                    let fader_animating = ctx.anim.fader.update();
                    let changed = ctx.anim.update(line1, line2);
                    if changed || target_changed || fader_animating || hover_changed {
                        needs_render = true;
                    }

                    // Wobble only while music is actually playing (idle = rock-solid still).
                    let wobble_active = is_playing && status == LyricsStatus::Loaded;
                    let (rot1, rot2) = ctx.anim.wobble_rotations(wobble_active);

                    // Visualizer pulse: chase the captured audio level
                    let vis_scale = ctx.anim.update_visualizer(audio_level, frame_dt);
                    // Visualizer appears instead of text when no lyrics could be fetched
                    let show_visualizer = ctx.config.visualizer && status == LyricsStatus::NotFound;
                    ctx.renderer.set_visualizer(show_visualizer, vis_scale);

                    let pulsing = show_visualizer && (ctx.anim.visualizer_scale > VISUALIZER_REST_SCALE + 0.005);

                    if ctx.anim.is_animating(wobble_active) || pulsing {
                        let lines = build_render_lines_rot(&ctx.anim, rot1, rot2);
                        ctx.renderer.render_lines(hwnd, &lines, ctx.is_locked, ctx.config.position.text_alignment());
                        needs_render = true;
                        pulsing
                    } else if needs_render {
                        // Render final settled frame
                        let lines = build_render_lines_rot(&ctx.anim, rot1, rot2);
                        ctx.renderer.render_lines(hwnd, &lines, ctx.is_locked, ctx.config.position.text_alignment());
                        needs_render = false;
                        false
                    } else {
                        false
                    }
                } else {
                    false
                };

                // Pacing:
                if is_animating {
                    // During spring/opacity transition: lock to screen refresh rate
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
                } else {
                    // When resting: poll cursor at ~60Hz for proximity detection
                    std::thread::sleep(Duration::from_millis(16));
                    next_frame = Instant::now();
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
                let (line1, line2, status) = {
                    let state = ctx.player_state.lock().unwrap();
                    let (l1, l2) = state.get_display_lyrics(ctx.config.time_offset_ms, ctx.config.show_romaji, ctx.config.show_hiragana);
                    (l1, l2, state.lyrics_status.clone())
                };

                let mut cursor_pt: POINT = zeroed();
                let _ = GetCursorPos(&mut cursor_pt);
                let mut win_rect: RECT = zeroed();
                let _ = GetWindowRect(hwnd, &mut win_rect);
                let dist = calculate_cursor_distance(cursor_pt, win_rect);
                let new_zone = ctx.anim.hover_zone.update(dist);
                ctx.anim.hover_zone = new_zone;
                let hover_factor = ctx.anim.hover_zone.opacity_factor();

                ctx.anim.update_targets(&status, hover_factor);
                ctx.anim.fader.update();
                ctx.anim.update(line1, line2);

                // Keep the pulse ticking while a modal drag blocks the main loop.
                ctx.anim.update_visualizer(0.0, 0.0);
                let show_visualizer = ctx.config.visualizer && status == LyricsStatus::NotFound;
                ctx.renderer.set_visualizer(show_visualizer, ctx.anim.visualizer_scale);
                let lines = build_render_lines(&ctx.anim);
                ctx.renderer.render_lines(hwnd, &lines, ctx.is_locked, ctx.config.position.text_alignment());
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
                let mut rect: RECT = zeroed();
                if GetWindowRect(hwnd, &mut rect).is_ok() {
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
    let instance = GetModuleHandleW(None).unwrap_or_default();

    // Try to load our embedded app icon (resource ID 1 written by build.rs / winres).
    // Falls back to the generic Windows application icon if unavailable.
    let hicon = LoadIconW(instance, windows::core::PCWSTR(1usize as *const u16))
        .or_else(|_| LoadIconW(None, IDI_APPLICATION))
        .unwrap_or_default();

    let mut nid: NOTIFYICONDATAW = zeroed();
    nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
    nid.hWnd = hwnd;
    nid.uID = 1;
    nid.uFlags = NIF_MESSAGE | NIF_TIP | NIF_ICON;
    nid.hIcon = hicon;
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

    let vis_flags = MF_STRING | if ctx.config.visualizer { MF_CHECKED } else { MF_UNCHECKED };
    let _ = AppendMenuW(hmenu, vis_flags, IDM_TOGGLE_VISUALIZER, w!("Visualizer (Music Reactive)"));

    let romaji_flags = MF_STRING | if ctx.config.show_romaji { MF_CHECKED } else { MF_UNCHECKED };
    let _ = AppendMenuW(hmenu, romaji_flags, IDM_TOGGLE_ROMAJI, w!("Show Romanized (Japanese)"));

    let hira_flags = MF_STRING | if ctx.config.show_hiragana { MF_CHECKED } else { MF_UNCHECKED };
    let _ = AppendMenuW(hmenu, hira_flags, IDM_TOGGLE_HIRAGANA, w!("Show Hiragana (Japanese)"));

    let lock_flags = MF_STRING | if ctx.is_locked { MF_CHECKED } else { MF_UNCHECKED };
    let _ = AppendMenuW(hmenu, lock_flags, IDM_TOGGLE_LOCK, w!("Lock Position (Drag to move)"));

    let _ = AppendMenuW(hmenu, MF_SEPARATOR, 0, PCWSTR(null_mut()));

    // --- Position submenu ---
    let hpos = match CreatePopupMenu() {
        Ok(m) => m,
        Err(_) => return,
    };
    for (i, pos) in LyricsPosition::all().iter().enumerate() {
        let is_current = *pos == ctx.config.position;
        let flags = MF_STRING | if is_current { MF_CHECKED } else { MF_UNCHECKED };
        let label: Vec<u16> = format!("{}\0", pos.label()).encode_utf16().collect();
        let _ = AppendMenuW(hpos, flags, IDM_POS_BASE + i, PCWSTR(label.as_ptr()));
    }
    let _ = AppendMenuW(hmenu, MF_POPUP, hpos.0 as usize, w!("Lyrics Position"));

    let _ = AppendMenuW(hmenu, MF_SEPARATOR, 0, PCWSTR(null_mut()));

    // --- Text Size submenu (multiples of the base font size) ---
    let hsize = match CreatePopupMenu() {
        Ok(m) => m,
        Err(_) => return,
    };
    let current_mult = ctx.config.font_size_line1 / BASE_FONT_LINE1;
    for (i, (label, mult)) in SIZE_PRESETS.iter().enumerate() {
        let is_current = (current_mult - mult).abs() < 0.01;
        let flags = MF_STRING | if is_current { MF_CHECKED } else { MF_UNCHECKED };
        let text: Vec<u16> = format!("{}\0", label).encode_utf16().collect();
        let _ = AppendMenuW(hsize, flags, IDM_SIZE_BASE + i, PCWSTR(text.as_ptr()));
    }
    let _ = AppendMenuW(hmenu, MF_POPUP, hsize.0 as usize, w!("Text Size"));

    // --- Text Color submenu ---
    let hcolor = match CreatePopupMenu() {
        Ok(m) => m,
        Err(_) => return,
    };
    for (i, (label, rgb)) in COLOR_PRESETS.iter().enumerate() {
        let is_current = ctx.config.text_color == *rgb;
        let flags = MF_STRING | if is_current { MF_CHECKED } else { MF_UNCHECKED };
        let text: Vec<u16> = format!("{}\0", label).encode_utf16().collect();
        let _ = AppendMenuW(hcolor, flags, IDM_COLOR_BASE + i, PCWSTR(text.as_ptr()));
    }
    let _ = AppendMenuW(hmenu, MF_POPUP, hcolor.0 as usize, w!("Text Color"));

    // --- Outline submenu (drop-shadow thickness) ---
    let houtline = match CreatePopupMenu() {
        Ok(m) => m,
        Err(_) => return,
    };
    for (i, (label, width)) in OUTLINE_PRESETS.iter().enumerate() {
        let is_current = (ctx.config.outline_width - width).abs() < 0.01;
        let flags = MF_STRING | if is_current { MF_CHECKED } else { MF_UNCHECKED };
        let text: Vec<u16> = format!("{}\0", label).encode_utf16().collect();
        let _ = AppendMenuW(houtline, flags, IDM_OUTLINE_BASE + i, PCWSTR(text.as_ptr()));
    }
    let _ = AppendMenuW(hmenu, MF_POPUP, houtline.0 as usize, w!("Outline"));

    // --- Opacity submenu ---
    let hopacity = match CreatePopupMenu() {
        Ok(m) => m,
        Err(_) => return,
    };
    for (i, (label, alpha)) in ALPHA_PRESETS.iter().enumerate() {
        let is_current = ctx.config.window_alpha == *alpha;
        let flags = MF_STRING | if is_current { MF_CHECKED } else { MF_UNCHECKED };
        let text: Vec<u16> = format!("{}\0", label).encode_utf16().collect();
        let _ = AppendMenuW(hopacity, flags, IDM_ALPHA_BASE + i, PCWSTR(text.as_ptr()));
    }
    let _ = AppendMenuW(hmenu, MF_POPUP, hopacity.0 as usize, w!("Opacity"));

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
    let _ = DestroyMenu(hsize);
    let _ = DestroyMenu(hcolor);
    let _ = DestroyMenu(houtline);
    let _ = DestroyMenu(hopacity);
    let _ = DestroyMenu(hpos);
    let _ = DestroyMenu(hmenu);
}

unsafe fn snap_to_position(hwnd: HWND, ctx: &mut WindowContext) {
    let screen_w = GetSystemMetrics(SM_CXSCREEN);
    let screen_h = GetSystemMetrics(SM_CYSCREEN);
    let w = ctx.config.window_width;
    let h = ctx.config.window_height;
    const MARGIN: i32 = 20;

    let x = match ctx.config.position {
        LyricsPosition::TopLeft | LyricsPosition::MiddleLeft | LyricsPosition::BottomLeft => MARGIN,
        LyricsPosition::TopCenter | LyricsPosition::MiddleCenter | LyricsPosition::BottomCenter => (screen_w - w) / 2,
        LyricsPosition::TopRight | LyricsPosition::MiddleRight | LyricsPosition::BottomRight => screen_w - w - MARGIN,
    };
    let y = match ctx.config.position {
        LyricsPosition::TopLeft | LyricsPosition::TopCenter | LyricsPosition::TopRight => 0,
        LyricsPosition::MiddleLeft | LyricsPosition::MiddleCenter | LyricsPosition::MiddleRight => (screen_h - h) / 2,
        LyricsPosition::BottomLeft | LyricsPosition::BottomCenter | LyricsPosition::BottomRight => screen_h - h - MARGIN,
    };

    ctx.config.window_x = Some(x);
    ctx.config.window_y = y;
    // Include size (no SWP_NOSIZE) so the window grows/shrinks when the text-size
    // preset changes the configured height. Width is unchanged by presets.
    let _ = SetWindowPos(hwnd, HWND_TOPMOST, x, y, w, h, SWP_NOACTIVATE);
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
            ctx.renderer.render_lines(hwnd, &lines, ctx.is_locked, ctx.config.position.text_alignment());
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
            ctx.renderer.render_lines(hwnd, &lines, ctx.is_locked, ctx.config.position.text_alignment());
        }
        IDM_OFFSET_PLUS => {
            ctx.config.time_offset_ms += 200;
            let _ = ctx.config.save();
            let (l1, l2) = {
                let state = ctx.player_state.lock().unwrap();
                state.get_display_lyrics(ctx.config.time_offset_ms, ctx.config.show_romaji, ctx.config.show_hiragana)
            };
            ctx.anim.update(l1, l2);
            let lines = build_render_lines(&ctx.anim);
            ctx.renderer.render_lines(hwnd, &lines, ctx.is_locked, ctx.config.position.text_alignment());
        }
        IDM_OFFSET_MINUS => {
            ctx.config.time_offset_ms -= 200;
            let _ = ctx.config.save();
            let (l1, l2) = {
                let state = ctx.player_state.lock().unwrap();
                state.get_display_lyrics(ctx.config.time_offset_ms, ctx.config.show_romaji, ctx.config.show_hiragana)
            };
            ctx.anim.update(l1, l2);
            let lines = build_render_lines(&ctx.anim);
            ctx.renderer.render_lines(hwnd, &lines, ctx.is_locked, ctx.config.position.text_alignment());
        }
        IDM_OFFSET_RESET => {
            ctx.config.time_offset_ms = 0;
            let _ = ctx.config.save();
            let (l1, l2) = {
                let state = ctx.player_state.lock().unwrap();
                state.get_display_lyrics(ctx.config.time_offset_ms, ctx.config.show_romaji, ctx.config.show_hiragana)
            };
            ctx.anim.update(l1, l2);
            let lines = build_render_lines(&ctx.anim);
            ctx.renderer.render_lines(hwnd, &lines, ctx.is_locked, ctx.config.position.text_alignment());
        }
        IDM_EXIT => {
            let _ = DestroyWindow(hwnd);
        }
        IDM_TOGGLE_VISUALIZER => {
            ctx.config.visualizer = !ctx.config.visualizer;
            let _ = ctx.config.save();
            let lines = build_render_lines(&ctx.anim);
            ctx.renderer.render_lines(hwnd, &lines, ctx.is_locked, ctx.config.position.text_alignment());
        }
        IDM_TOGGLE_ROMAJI => {
            ctx.config.show_romaji = !ctx.config.show_romaji;
            if ctx.config.show_romaji {
                ctx.config.show_hiragana = false;
            }
            let _ = ctx.config.save();
            let (l1, l2) = {
                let state = ctx.player_state.lock().unwrap();
                state.get_display_lyrics(ctx.config.time_offset_ms, ctx.config.show_romaji, ctx.config.show_hiragana)
            };
            ctx.anim.update(l1, l2);
            let lines = build_render_lines(&ctx.anim);
            ctx.renderer.render_lines(hwnd, &lines, ctx.is_locked, ctx.config.position.text_alignment());
        }
        IDM_TOGGLE_HIRAGANA => {
            ctx.config.show_hiragana = !ctx.config.show_hiragana;
            if ctx.config.show_hiragana {
                ctx.config.show_romaji = false;
            }
            let _ = ctx.config.save();
            let (l1, l2) = {
                let state = ctx.player_state.lock().unwrap();
                state.get_display_lyrics(ctx.config.time_offset_ms, ctx.config.show_romaji, ctx.config.show_hiragana)
            };
            ctx.anim.update(l1, l2);
            let lines = build_render_lines(&ctx.anim);
            ctx.renderer.render_lines(hwnd, &lines, ctx.is_locked, ctx.config.position.text_alignment());
        }
        cmd if cmd >= IDM_POS_BASE && cmd < IDM_POS_BASE + LyricsPosition::all().len() => {
            let idx = cmd - IDM_POS_BASE;
            let new_pos = LyricsPosition::all()[idx];
            ctx.config.position = new_pos;
            snap_to_position(hwnd, ctx);
            let _ = ctx.config.save();
            let lines = build_render_lines(&ctx.anim);
            ctx.renderer.render_lines(hwnd, &lines, ctx.is_locked, ctx.config.position.text_alignment());
        }
        cmd if cmd >= IDM_SIZE_BASE && cmd < IDM_SIZE_BASE + SIZE_COUNT => {
            let idx = cmd - IDM_SIZE_BASE;
            let mult = SIZE_PRESETS[idx].1;
            ctx.config.font_size_line1 = BASE_FONT_LINE1 * mult;
            ctx.config.font_size_line2 = BASE_FONT_LINE2 * mult;
            ctx.config.window_height = (BASE_WINDOW_HEIGHT as f32 * mult).round() as i32;
            // Keep the current anchor: re-snap so the window stays on-screen at the new height.
            snap_to_position(hwnd, ctx);
            let _ = ctx.config.save();
            ctx.renderer.rebuild(&ctx.config);
            ctx.anim.font_size_line1 = ctx.config.font_size_line1;
            let lines = build_render_lines(&ctx.anim);
            ctx.renderer.render_lines(hwnd, &lines, ctx.is_locked, ctx.config.position.text_alignment());
        }
        cmd if cmd >= IDM_COLOR_BASE && cmd < IDM_COLOR_BASE + COLOR_PRESETS.len() => {
            let idx = cmd - IDM_COLOR_BASE;
            ctx.config.text_color = COLOR_PRESETS[idx].1;
            let _ = ctx.config.save();
            ctx.renderer.set_colors(ctx.config.text_color, ctx.config.outline_color, ctx.config.outline_width);
            let lines = build_render_lines(&ctx.anim);
            ctx.renderer.render_lines(hwnd, &lines, ctx.is_locked, ctx.config.position.text_alignment());
        }
        cmd if cmd >= IDM_OUTLINE_BASE && cmd < IDM_OUTLINE_BASE + OUTLINE_COUNT => {
            let idx = cmd - IDM_OUTLINE_BASE;
            ctx.config.outline_width = OUTLINE_PRESETS[idx].1;
            let _ = ctx.config.save();
            ctx.renderer.set_colors(ctx.config.text_color, ctx.config.outline_color, ctx.config.outline_width);
            let lines = build_render_lines(&ctx.anim);
            ctx.renderer.render_lines(hwnd, &lines, ctx.is_locked, ctx.config.position.text_alignment());
        }
        cmd if cmd >= IDM_ALPHA_BASE && cmd < IDM_ALPHA_BASE + ALPHA_COUNT => {
            let idx = cmd - IDM_ALPHA_BASE;
            ctx.config.window_alpha = ALPHA_PRESETS[idx].1;
            let _ = ctx.config.save();
            ctx.renderer.set_master_alpha(ctx.config.window_alpha);
            let lines = build_render_lines(&ctx.anim);
            ctx.renderer.render_lines(hwnd, &lines, ctx.is_locked, ctx.config.position.text_alignment());
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
    fn test_animation_state() {
        let mut anim = AnimationState::new();
        assert!(!anim.is_animating(false));

        let changed = anim.update("Line 1".into(), "Line 2".into());
        assert!(changed);
        assert!(anim.is_animating(false));

        let lines = build_render_lines(&anim);
        assert!(!lines.is_empty());
    }

    #[test]
    fn test_wobble_is_bounded_and_off_when_inactive() {
        let anim = AnimationState::new();
        // Inactive → no rotation at all.
        assert_eq!(anim.wobble_rotations(false), (0.0, 0.0));
        // Active → small, bounded rotation for both lines (never a wild spin).
        for _ in 0..50 {
            let (r1, r2) = anim.wobble_rotations(true);
            assert!(r1.abs() <= 0.9 + 1e-4, "r1 out of range: {}", r1);
            assert!(r2.abs() <= 0.6 + 1e-4, "r2 out of range: {}", r2);
        }
    }

    #[test]
    fn test_visualizer_pulse_eases_toward_level_and_never_overflows() {
        let mut anim = AnimationState::new();
        assert_eq!(anim.visualizer_scale, VISUALIZER_REST_SCALE);

        // The resting size and the full-volume size must be far enough apart to
        // actually see: this is the regression that made the art look static.
        assert!(
            VISUALIZER_GAIN >= 0.3,
            "pulse range too small to see: {VISUALIZER_REST_SCALE}..{}",
            VISUALIZER_REST_SCALE + VISUALIZER_GAIN
        );

        // Silent: stays parked at the rest scale.
        for _ in 0..60 {
            anim.update_visualizer(0.0, 1.0 / 60.0);
        }
        assert!((anim.visualizer_scale - VISUALIZER_REST_SCALE).abs() < 0.001);

        // Loud music: rises, and the peak still fits the slot (scale <= 1.0).
        for _ in 0..60 {
            let s = anim.update_visualizer(1.0, 1.0 / 60.0);
            assert!((VISUALIZER_REST_SCALE..=1.0).contains(&s), "scale out of slot: {}", s);
        }
        assert!(anim.visualizer_scale > VISUALIZER_REST_SCALE + VISUALIZER_GAIN * 0.9);

        // Music stops: it eases back down instead of snapping.
        let before = anim.visualizer_scale;
        anim.update_visualizer(0.0, 1.0 / 60.0);
        assert!(anim.visualizer_scale < before);
        assert!(anim.visualizer_scale > VISUALIZER_REST_SCALE);

        // Every sample of real music must produce visible motion (a frame is
        // rendered only when `update_visualizer` still reports a gap this big),
        // so a jumping level has to move the scale on the very next frame.
        let mut anim = AnimationState::new();
        for _ in 0..30 {
            anim.update_visualizer(0.0, 1.0 / 60.0);
        }
        let rest = anim.visualizer_scale;
        anim.update_visualizer(1.0, 1.0 / 60.0);
        assert!(
            anim.visualizer_scale - rest > 0.002,
            "single loud frame only moved {}",
            anim.visualizer_scale - rest
        );
    }

    #[test]
    fn test_transition_is_smooth_and_directional() {
        let mut anim = AnimationState::new();
        anim.current_line1 = "first".into();
        anim.current_line2 = "second".into();
        // Advance to the next line: line2 becomes line1 → scrolls up (scroll_down = false).
        anim.update("second".into(), "third".into());
        assert!(!anim.scroll_down);

        let lines = build_render_lines_rot(&anim, 0.0, 0.0);
        // Both the outgoing and incoming line-1 are present during the cross-fade.
        assert!(lines.len() >= 3);

        // Incoming line starts displaced from its rest position, settles by the end.
        let start_y = lines
            .iter()
            .find(|l| l.text == "second")
            .map(|l| l.y)
            .unwrap();
        assert!((start_y - BASE_L1_Y).abs() > 1.0, "incoming line should start displaced");

        // After the transition duration, layout snaps to exact rest positions.
        anim.transition_start = Some(Instant::now() - Duration::from_millis(600));
        let settled = build_render_lines_rot(&anim, 0.0, 0.0);
        assert!((settled[0].y - BASE_L1_Y).abs() < 0.001);
    }

    /// Advancing a line must scroll the lyrics upward: the outgoing line slides up
    /// (negative Y offset) while the incoming line rises from below (positive Y).
    #[test]
    fn test_transition_slides_out_up_on_advance() {
        let mut anim = AnimationState::new();
        anim.current_line1 = "first".into();
        anim.current_line2 = "second".into();
        anim.update("second".into(), "third".into());
        assert!(!anim.scroll_down);
        // Sample partway through the transition, before it settles.
        anim.transition_start = Some(Instant::now() - Duration::from_millis(120));

        let lines = build_render_lines_rot(&anim, 0.0, 0.0);
        let outgoing = lines
            .iter()
            .find(|l| l.text == "first")
            .expect("outgoing line-1 present");
        let incoming = lines
            .iter()
            .find(|l| l.text == "second" && l.is_active)
            .expect("incoming line-1 present");
        assert!(outgoing.y < BASE_L1_Y, "outgoing line should slide up, got y={}", outgoing.y);
        assert!(incoming.y > BASE_L1_Y, "incoming line should enter from below, got y={}", incoming.y);
    }

    #[test]
    fn test_size_change_rescales_line_spacing() {
        let mut anim = AnimationState::new();
        anim.current_line1 = "a".into();
        anim.current_line2 = "b".into();
        anim.transition_start = Some(Instant::now() - Duration::from_millis(600));

        // At 100% the two lines sit at the base Y offsets.
        let base = build_render_lines(&anim);
        assert!((base[0].y - BASE_L1_Y).abs() < 0.001);
        assert!((base[1].y - BASE_L2_Y).abs() < 0.001);

        // At 150% the spacing scales proportionally (no overlap).
        anim.font_size_line1 = BASE_FONT_LINE1 * 1.5;
        let big = build_render_lines(&anim);
        assert!((big[0].y - BASE_L1_Y * 1.5).abs() < 0.001);
        assert!((big[1].y - BASE_L2_Y * 1.5).abs() < 0.001);
    }

    #[test]
    fn test_opacity_fader() {
        let mut fader = OpacityFader::new();
        assert_eq!(fader.current_opacity, 1.0);
        assert!(!fader.is_fading());

        // Start fade to 10%
        let changed = fader.set_target(0.10, 0.22);
        assert!(changed);
        assert!(fader.is_fading());

        // Setting same target again should return false (no-op)
        assert!(!fader.set_target(0.10, 0.22));

        // Advance past duration
        fader.fade_start = Some(Instant::now() - Duration::from_millis(400));
        let still_fading = fader.update();
        assert!(!still_fading);
        assert!(!fader.is_fading());
        assert!((fader.current_opacity - 0.10).abs() < 0.001);

        // Fade back up to 100%
        let changed_up = fader.set_target(1.0, 0.22);
        assert!(changed_up);
        assert!(fader.is_fading());

        // Advance past duration
        fader.fade_start = Some(Instant::now() - Duration::from_millis(400));
        let still_fading_up = fader.update();
        assert!(!still_fading_up);
        assert!(!fader.is_fading());
        assert!((fader.current_opacity - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_render_lines_fader_opacity() {
        let mut anim = AnimationState::new();
        anim.current_line1 = "Track Title".into();
        anim.current_line2 = "(No synced lyrics found on LRCLIB)".into();

        // Settled at 10%
        anim.fader.current_opacity = 0.10;
        let lines_10 = build_render_lines(&anim);
        assert_eq!(lines_10.len(), 2);
        assert!((lines_10[0].opacity - 0.10).abs() < 0.001);
        assert!((lines_10[1].opacity - 0.07).abs() < 0.001);

        // Settled at 100%
        anim.fader.current_opacity = 1.0;
        let lines_100 = build_render_lines(&anim);
        assert_eq!(lines_100.len(), 2);
        assert!((lines_100[0].opacity - 1.0).abs() < 0.001);
        assert!((lines_100[1].opacity - 0.70).abs() < 0.001);
    }

    #[test]
    fn test_hover_zone_transitions() {
        // Far -> Near when cursor is within 150px
        let zone = HoverZone::Far;
        assert_eq!(zone.update(200.0), HoverZone::Far);
        assert_eq!(zone.update(100.0), HoverZone::Near);
        assert_eq!(zone.update(30.0),  HoverZone::Nearer);

        // Near has hysteresis on Far boundary (must be > 165, not > 150)
        let near = HoverZone::Near;
        assert_eq!(near.update(155.0), HoverZone::Near);   // stays Near (< 165)
        assert_eq!(near.update(170.0), HoverZone::Far);    // exits to Far
        assert_eq!(near.update(20.0),  HoverZone::Nearer);

        // Nearer has hysteresis on both edges
        let nearer = HoverZone::Nearer;
        assert_eq!(nearer.update(30.0),  HoverZone::Nearer); // stays Nearer
        assert_eq!(nearer.update(60.0),  HoverZone::Near);   // exits to Near
        assert_eq!(nearer.update(170.0), HoverZone::Far);    // exits all the way

        // Opacity factors are correct
        assert!((HoverZone::Far.opacity_factor()    - 1.00).abs() < 0.001);
        assert!((HoverZone::Near.opacity_factor()   - 0.70).abs() < 0.001);
        assert!((HoverZone::Nearer.opacity_factor() - 0.40).abs() < 0.001);
    }

    #[test]
    fn test_calculate_cursor_distance() {
        let rect = RECT { left: 100, top: 50, right: 500, bottom: 100 };

        // Inside: distance should be 0
        let inside = POINT { x: 300, y: 75 };
        assert_eq!(calculate_cursor_distance(inside, rect), 0.0);

        // Directly above: distance = vertical gap
        let above = POINT { x: 300, y: 20 };
        assert!((calculate_cursor_distance(above, rect) - 30.0).abs() < 0.001);

        // To the left: distance = horizontal gap
        let left = POINT { x: 60, y: 75 };
        assert!((calculate_cursor_distance(left, rect) - 40.0).abs() < 0.001);
    }
}
