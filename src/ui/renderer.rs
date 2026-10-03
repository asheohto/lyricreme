use std::mem::zeroed;
use std::ptr::null_mut;
use std::sync::OnceLock;
use windows::core::{w, Interface, PCWSTR};
use windows::Win32::Foundation::{HWND, RECT};
use windows::Foundation::Numerics::Matrix3x2;
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_PIXEL_FORMAT, D2D_RECT_F, D2D_SIZE_U,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1CreateFactory, ID2D1Bitmap, ID2D1DCRenderTarget, ID2D1Factory, ID2D1RenderTarget,
    ID2D1SolidColorBrush, D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, D2D1_BITMAP_PROPERTIES,
    D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_RENDER_TARGET_PROPERTIES,
    D2D1_RENDER_TARGET_TYPE_DEFAULT, D2D1_RENDER_TARGET_USAGE_GDI_COMPATIBLE,
    D2D1_ROUNDED_RECT, D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE,
};
use windows::Win32::Graphics::DirectWrite::{
    DWriteCreateFactory, IDWriteFactory, IDWriteTextFormat, DWRITE_FACTORY_TYPE_SHARED,
    DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT_BOLD,
    DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_TEXT_ALIGNMENT_CENTER,
    DWRITE_TEXT_ALIGNMENT_LEADING, DWRITE_TEXT_ALIGNMENT_TRAILING,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC, ReleaseDC, SelectObject,
    BITMAPINFO, BITMAPINFOHEADER, BI_RGB, BLENDFUNCTION, DIB_RGB_COLORS, HDC, HGDIOBJ,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetWindowRect, UpdateLayeredWindow, ULW_ALPHA,
};

use crate::config::TextAlign;

/// The visualizer art is embedded in the binary (like the tray icon in
/// `build.rs`) so a stray working directory can't break it.
const VIBE_PNG: &[u8] = include_bytes!("../../assets/vibe.png");

/// Decoded visualizer art, cached for the process.
static VIBE_PIXELS: OnceLock<Option<(u32, u32, Vec<u8>)>> = OnceLock::new();

/// Decodes the embedded PNG to premultiplied BGRA with WIC.
/// Returns None if WIC fails to initialise or the image is malformed.
unsafe fn decode_vibe_png() -> Option<(u32, u32, Vec<u8>)> {
    use windows::Win32::Graphics::Imaging::{
        IWICFormatConverter, IWICImagingFactory, IWICPalette, CLSID_WICImagingFactory,
        GUID_WICPixelFormat32bppPBGRA, WICBitmapDitherTypeNone, WICBitmapPaletteTypeCustom,
        WICDecodeMetadataCacheOnLoad,
    };
    use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};
    use windows::Win32::UI::Shell::SHCreateMemStream;

    let factory: IWICImagingFactory =
        CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).ok()?;
    let stream = SHCreateMemStream(Some(VIBE_PNG))?;
    let decoder = factory
        .CreateDecoderFromStream(&stream, std::ptr::null(), WICDecodeMetadataCacheOnLoad)
        .ok()?;
    let frame = decoder.GetFrame(0).ok()?;
    let converter: IWICFormatConverter = factory.CreateFormatConverter().ok()?;
    converter
        .Initialize(
            &frame,
            &GUID_WICPixelFormat32bppPBGRA,
            WICBitmapDitherTypeNone,
            None::<&IWICPalette>,
            0.0,
            WICBitmapPaletteTypeCustom,
        )
        .ok()?;

    let mut width = 0u32;
    let mut height = 0u32;
    converter.GetSize(&mut width, &mut height).ok()?;
    if width == 0 || height == 0 {
        return None;
    }

    let mut pixels = vec![0u8; width as usize * height as usize * 4];
    converter
        .CopyPixels(std::ptr::null(), width * 4, &mut pixels)
        .ok()?;

    Some((width, height, pixels))
}

fn vibe_art() -> Option<&'static (u32, u32, Vec<u8>)> {
    VIBE_PIXELS
        .get_or_init(|| unsafe {
            let res = decode_vibe_png();
            if res.is_none() {
                eprintln!("[LyricReme] Visualizer art failed to decode; rendering without it.");
            }
            res
        })
        .as_ref()
}

/// Music-reactive art drawn in place of lyrics.
struct Visualizer {
    bitmap: ID2D1Bitmap,
    scale: f32,
    visible: bool,
}

#[derive(Debug, Clone)]
pub struct RenderLine {
    pub text: String,
    pub y: f32,
    pub opacity: f32,
    pub is_active: bool,
    /// Rotation in degrees around the line's own horizontal centre (wobble).
    pub rotation_deg: f32,
}

pub struct OverlayRenderer {
    width: i32,
    height: i32,
    hdc_mem: HDC,
    hbitmap: HGDIOBJ,
    old_bitmap: HGDIOBJ,
    #[allow(dead_code)]
    bits: *mut u8,
    #[allow(dead_code)]
    d2d_factory: ID2D1Factory,
    #[allow(dead_code)]
    dwrite_factory: IDWriteFactory,
    dc_render_target: ID2D1DCRenderTarget,
    render_target: ID2D1RenderTarget,
    brush_drag_bg: ID2D1SolidColorBrush,
    brush_drag_border: ID2D1SolidColorBrush,
    brush_glow: ID2D1SolidColorBrush,
    brush_active: ID2D1SolidColorBrush,
    brush_next: ID2D1SolidColorBrush,
    text_format_line1: IDWriteTextFormat,
    text_format_line2: IDWriteTextFormat,
    outline_width: f32,
    /// Global overlay opacity 0–255, applied via BLENDFUNCTION.SourceConstantAlpha
    /// (UpdateLayeredWindow and SetLayeredWindowAttributes are mutually exclusive).
    master_alpha: u8,
    /// Music-reactive art. `None` if the embedded PNG failed to decode.
    visualizer: Option<Visualizer>,
}

impl OverlayRenderer {
    pub unsafe fn new(
        width: i32,
        height: i32,
        font_family: &str,
        font_size_line1: f32,
        font_size_line2: f32,
        text_color: [u8; 3],
        outline_color: [u8; 3],
        outline_width: f32,
    ) -> Result<Self, windows::core::Error> {
        let screen_dc = GetDC(HWND(null_mut()));
        let hdc_mem = CreateCompatibleDC(screen_dc);

        let mut bmi: BITMAPINFO = zeroed();
        bmi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
        bmi.bmiHeader.biWidth = width;
        bmi.bmiHeader.biHeight = -height; // Top-down DIB
        bmi.bmiHeader.biPlanes = 1;
        bmi.bmiHeader.biBitCount = 32;
        bmi.bmiHeader.biCompression = BI_RGB.0;

        let mut bits: *mut std::ffi::c_void = null_mut();
        let hbitmap = CreateDIBSection(
            hdc_mem,
            &bmi,
            DIB_RGB_COLORS,
            &mut bits,
            None,
            0,
        )?;
        let old_bitmap = SelectObject(hdc_mem, hbitmap);
        ReleaseDC(HWND(null_mut()), screen_dc);

        let d2d_factory: ID2D1Factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
        let dwrite_factory: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;

        let rt_props = D2D1_RENDER_TARGET_PROPERTIES {
            r#type: D2D1_RENDER_TARGET_TYPE_DEFAULT,
            pixelFormat: D2D1_PIXEL_FORMAT {
                format: DXGI_FORMAT_B8G8R8A8_UNORM,
                alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
            },
            dpiX: 96.0,
            dpiY: 96.0,
            usage: D2D1_RENDER_TARGET_USAGE_GDI_COMPATIBLE,
            minLevel: Default::default(),
        };

        let dc_render_target = d2d_factory.CreateDCRenderTarget(&rt_props)?;
        let render_target: ID2D1RenderTarget = dc_render_target.cast()?;

        // Grayscale antialiasing is required for transparent layered windows so Direct2D writes true per-pixel alpha
        render_target.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);

        let font_wide: Vec<u16> = font_family.encode_utf16().chain(std::iter::once(0)).collect();
        let font_pcwstr = PCWSTR(font_wide.as_ptr());

        let text_format_line1 = dwrite_factory.CreateTextFormat(
            font_pcwstr,
            None,
            DWRITE_FONT_WEIGHT_BOLD,
            DWRITE_FONT_STYLE_NORMAL,
            DWRITE_FONT_STRETCH_NORMAL,
            font_size_line1,
            w!("en-US"),
        )?;
        text_format_line1.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER)?;

        let text_format_line2 = dwrite_factory.CreateTextFormat(
            font_pcwstr,
            None,
            DWRITE_FONT_WEIGHT_SEMI_BOLD,
            DWRITE_FONT_STYLE_NORMAL,
            DWRITE_FONT_STRETCH_NORMAL,
            font_size_line2,
            w!("en-US"),
        )?;
        text_format_line2.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER)?;

        // Repositioning guide brushes (only visible when unlocked for dragging)
        let brush_drag_bg = render_target.CreateSolidColorBrush(
            &D2D1_COLOR_F { r: 0.10, g: 0.20, b: 0.40, a: 0.25 },
            None,
        )?;
        let brush_drag_border = render_target.CreateSolidColorBrush(
            &D2D1_COLOR_F { r: 0.45, g: 0.70, b: 1.0, a: 0.60 },
            None,
        )?;

        // Outline / drop shadow brush for maximum text readability without background box
        let brush_glow = render_target.CreateSolidColorBrush(
            &D2D1_COLOR_F {
                r: outline_color[0] as f32 / 255.0,
                g: outline_color[1] as f32 / 255.0,
                b: outline_color[2] as f32 / 255.0,
                a: 0.90,
            },
            None,
        )?;

        // Active primary lyric (Line 1) - user-configurable colour
        let brush_active = render_target.CreateSolidColorBrush(
            &D2D1_COLOR_F {
                r: text_color[0] as f32 / 255.0,
                g: text_color[1] as f32 / 255.0,
                b: text_color[2] as f32 / 255.0,
                a: 1.0,
            },
            None,
        )?;

        // Upcoming preview lyric (Line 2) - same colour, dimmed via opacity at draw time
        let brush_next = render_target.CreateSolidColorBrush(
            &D2D1_COLOR_F {
                r: text_color[0] as f32 / 255.0,
                g: text_color[1] as f32 / 255.0,
                b: text_color[2] as f32 / 255.0,
                a: 0.70,
            },
            None,
        )?;

        // Visualizer art: decoded once per process, then uploaded to this target
        let visualizer = if let Some((w, h, pixels)) = vibe_art() {
            let props = D2D1_BITMAP_PROPERTIES {
                pixelFormat: D2D1_PIXEL_FORMAT {
                    format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
                },
                dpiX: 96.0,
                dpiY: 96.0,
            };
            let size = D2D_SIZE_U {
                width: *w,
                height: *h,
            };
            let pitch = w * 4;
            render_target
                .CreateBitmap(size, Some(pixels.as_ptr() as *const core::ffi::c_void), pitch, &props)
                .ok()
                .map(|bitmap| Visualizer {
                    bitmap,
                    scale: 1.0,
                    visible: false,
                })
        } else {
            None
        };

        Ok(Self {
            width,
            height,
            hdc_mem,
            hbitmap: hbitmap.into(),
            old_bitmap,
            bits: bits as *mut u8,
            d2d_factory,
            dwrite_factory,
            dc_render_target,
            render_target,
            brush_drag_bg,
            brush_drag_border,
            brush_glow,
            brush_active,
            brush_next,
            text_format_line1,
            text_format_line2,
            outline_width,
            master_alpha: 255,
            visualizer,
        })
    }

    pub unsafe fn render_lines(
        &mut self,
        hwnd: HWND,
        lines: &[RenderLine],
        is_locked: bool,
        text_align: TextAlign,
    ) {
        let rect = RECT {
            left: 0,
            top: 0,
            right: self.width,
            bottom: self.height,
        };

        if self.dc_render_target.BindDC(self.hdc_mem, &rect).is_err() {
            return;
        }

        // Apply text alignment (DirectWrite alignment is per-format, update every frame)
        let dwrite_align = match text_align {
            TextAlign::Left   => DWRITE_TEXT_ALIGNMENT_LEADING,
            TextAlign::Center => DWRITE_TEXT_ALIGNMENT_CENTER,
            TextAlign::Right  => DWRITE_TEXT_ALIGNMENT_TRAILING,
        };
        let _ = self.text_format_line1.SetTextAlignment(dwrite_align);
        let _ = self.text_format_line2.SetTextAlignment(dwrite_align);

        self.render_target.BeginDraw();
        self.render_target.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);

        // Completely clear target to 100% transparent (no background rectangle!)
        self.render_target.Clear(Some(&D2D1_COLOR_F {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 0.0,
        }));

        // When unlocked for dragging, display a subtle helper box so user knows where to drag
        if !is_locked {
            let guide_rect = D2D1_ROUNDED_RECT {
                rect: windows::Win32::Graphics::Direct2D::Common::D2D_RECT_F {
                    left: 4.0,
                    top: 2.0,
                    right: self.width as f32 - 4.0,
                    bottom: self.height as f32 - 2.0,
                },
                radiusX: 12.0,
                radiusY: 12.0,
            };
            self.render_target.FillRoundedRectangle(&guide_rect, &self.brush_drag_bg);
            self.render_target.DrawRoundedRectangle(&guide_rect, &self.brush_drag_border, 1.5, None);
        }

        // Visualizer art sits in place of the text when no lyrics are found.
        self.draw_visualizer(text_align);

        // Render each active line
        for line in lines {
            if line.text.is_empty() || line.opacity <= 0.005 {
                continue;
            }

            let text_wide: Vec<u16> = line.text.encode_utf16().collect();
            let format = if line.is_active {
                &self.text_format_line1
            } else {
                &self.text_format_line2
            };
            // Line box height scales with the actual font size so larger text isn't clipped.
            // Times ~1.75 leaves room for ascenders/descenders; the outline extends a
            // little beyond that, which DrawText does not clip.
            let font_size = if line.is_active {
                self.text_format_line1.GetFontSize()
            } else {
                self.text_format_line2.GetFontSize()
            };
            let height = (font_size * 1.75).max(if line.is_active { 42.0 } else { 32.0 });
            let alpha = line.opacity.clamp(0.0, 1.0);

            // Wobble: rotate the whole line (outline + fill) about its own centre.
            // Identity transform is restored after the line so layout stays untouched.
            let wobbling = line.rotation_deg.abs() > 0.0005;
            if wobbling {
                let cx = self.width as f32 / 2.0;
                let cy = line.y + height / 2.0;
                let m = Matrix3x2::rotation(line.rotation_deg.to_radians(), cx, cy);
                self.render_target.SetTransform(&m);
            }

            // 1. 8-directional shadow / outline for crisp contrast against any background
            self.brush_glow.SetOpacity(0.90 * alpha);
            let ow = self.outline_width;
            if ow > 0.01 {
                let shadow_offsets = [
                    (-ow, 0.0), (ow, 0.0), (0.0, -ow), (0.0, ow),
                    (-ow * 0.8, -ow * 0.8), (ow * 0.8, -ow * 0.8),
                    (-ow * 0.8, ow * 0.8), (ow * 0.8, ow * 0.8),
                    (0.0, ow * 1.3),
                ];

                for (dx, dy) in shadow_offsets {
                    let s_rect = windows::Win32::Graphics::Direct2D::Common::D2D_RECT_F {
                        left: dx,
                        top: line.y + dy,
                        right: self.width as f32 + dx,
                        bottom: line.y + height + dy,
                    };
                    self.render_target.DrawText(
                        &text_wide,
                        format,
                        &s_rect,
                        &self.brush_glow,
                        windows::Win32::Graphics::Direct2D::D2D1_DRAW_TEXT_OPTIONS_NONE,
                        windows::Win32::Graphics::DirectWrite::DWRITE_MEASURING_MODE_NATURAL,
                    );
                }
            }

            // 2. Foreground text
            let fg_brush = if line.is_active {
                self.brush_active.SetOpacity(1.0 * alpha);
                &self.brush_active
            } else {
                self.brush_next.SetOpacity(0.70 * alpha);
                &self.brush_next
            };

            let fg_rect = windows::Win32::Graphics::Direct2D::Common::D2D_RECT_F {
                left: 0.0,
                top: line.y,
                right: self.width as f32,
                bottom: line.y + height,
            };

            self.render_target.DrawText(
                &text_wide,
                format,
                &fg_rect,
                fg_brush,
                windows::Win32::Graphics::Direct2D::D2D1_DRAW_TEXT_OPTIONS_NONE,
                windows::Win32::Graphics::DirectWrite::DWRITE_MEASURING_MODE_NATURAL,
            );

            if wobbling {
                self.render_target.SetTransform(&Matrix3x2::identity());
            }
        }

        let _ = self.render_target.EndDraw(None, None);

        let mut pt_src = windows::Win32::Foundation::POINT { x: 0, y: 0 };
        let mut size = windows::Win32::Foundation::SIZE {
            cx: self.width,
            cy: self.height,
        };
        let blend = BLENDFUNCTION {
            BlendOp: 0,
            BlendFlags: 0,
            SourceConstantAlpha: self.master_alpha,
            AlphaFormat: 1,
        };

        let mut win_rect = zeroed();
        let _ = GetWindowRect(hwnd, &mut win_rect);
        let mut pt_dst = windows::Win32::Foundation::POINT {
            x: win_rect.left,
            y: win_rect.top,
        };

        let screen_dc = GetDC(HWND(null_mut()));
        let res = UpdateLayeredWindow(
            hwnd,
            screen_dc,
            Some(&mut pt_dst),
            Some(&mut size),
            self.hdc_mem,
            Some(&mut pt_src),
            windows::Win32::Foundation::COLORREF(0),
            Some(&blend),
            ULW_ALPHA,
        );
        if let Err(err) = res {
            eprintln!("[LyricReme] UpdateLayeredWindow failed: {:?}", err);
        }
        ReleaseDC(HWND(null_mut()), screen_dc);
    }

    /// Recolour text/outline brushes at runtime (tray colour picker) without recreating the target.
    pub fn set_colors(&mut self, text_color: [u8; 3], outline_color: [u8; 3], outline_width: f32) {
        unsafe {
            self.brush_active.SetColor(&D2D1_COLOR_F {
                r: text_color[0] as f32 / 255.0,
                g: text_color[1] as f32 / 255.0,
                b: text_color[2] as f32 / 255.0,
                a: 1.0,
            });
            self.brush_next.SetColor(&D2D1_COLOR_F {
                r: text_color[0] as f32 / 255.0,
                g: text_color[1] as f32 / 255.0,
                b: text_color[2] as f32 / 255.0,
                a: 0.70,
            });
            self.brush_glow.SetColor(&D2D1_COLOR_F {
                r: outline_color[0] as f32 / 255.0,
                g: outline_color[1] as f32 / 255.0,
                b: outline_color[2] as f32 / 255.0,
                a: 0.90,
            });
        }
        self.outline_width = outline_width;
    }

    /// Set the global overlay opacity (0–100 → 0–255 alpha). Applied on the next frame
    /// through BLENDFUNCTION.SourceConstantAlpha, which multiplies the per-pixel alpha.
    pub fn set_master_alpha(&mut self, percent: u8) {
        let clamped = percent.clamp(0, 100);
        self.master_alpha = ((clamped as u32 * 255) / 100) as u8;
    }

    /// Shows/hides the visualizer and sets its pulse scale (1.0 = full slot).
    /// Called every frame; a no-op when the art failed to decode.
    pub fn set_visualizer(&mut self, visible: bool, scale: f32) {
        if let Some(vis) = self.visualizer.as_mut() {
            vis.visible = visible;
            vis.scale = scale;
        }
    }

    /// Draws the visualizer art in place of the text, scaled by the pulse.
    unsafe fn draw_visualizer(&self, text_align: TextAlign) {
        let Some(vis) = self.visualizer.as_ref() else {
            return;
        };
        if !vis.visible {
            return;
        }

        let slot = (self.height as f32 - 20.0).clamp(60.0, 160.0);
        let size = slot * vis.scale;
        let centre_x = match text_align {
            TextAlign::Left => 12.0 + slot / 2.0,
            TextAlign::Center => self.width as f32 / 2.0,
            TextAlign::Right => self.width as f32 - 12.0 - slot / 2.0,
        };
        let centre_y = 4.0 + slot / 2.0;
        let dest = D2D_RECT_F {
            left: centre_x - size / 2.0,
            top: centre_y - size / 2.0,
            right: centre_x + size / 2.0,
            bottom: centre_y + size / 2.0,
        };

        let _ = self.render_target.DrawBitmap(
            &vis.bitmap,
            Some(&dest),
            1.0,
            D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
            None,
        );
    }

    /// Recreate the DIB/text formats at a new size or font size (tray "Text Size").
    /// Cheap enough to do on a menu click; reuses the whole construction path.
    pub unsafe fn rebuild(&mut self, config: &crate::config::AppConfig) {
        if let Ok(mut fresh) = Self::new(
            config.window_width,
            config.window_height,
            &config.font_family,
            config.font_size_line1,
            config.font_size_line2,
            config.text_color,
            config.outline_color,
            config.outline_width,
        ) {
            fresh.master_alpha = self.master_alpha;
            // Pulse state is per-frame UI state, not config: keep it across rebuilds
            // (the bitmap itself is re-uploaded by `new`).
            if let (Some(new_vis), Some(old_vis)) = (fresh.visualizer.as_mut(), self.visualizer.as_ref())
            {
                new_vis.scale = old_vis.scale;
                new_vis.visible = old_vis.visible;
            }
            // Release the old DIB/DC before swapping; text formats are COM-managed.
            let old = std::mem::replace(self, fresh);
            drop(old);
        }
    }

    #[allow(dead_code)]
    pub unsafe fn render(
        &mut self,
        hwnd: HWND,
        line1: &str,
        line2: &str,
        is_locked: bool,
    ) {
        let mut lines = Vec::with_capacity(2);
        if !line1.is_empty() {
            lines.push(RenderLine {
                text: line1.to_string(),
                y: 6.0,
                opacity: 1.0,
                is_active: true,
                rotation_deg: 0.0,
            });
        }
        if !line2.is_empty() {
            lines.push(RenderLine {
                text: line2.to_string(),
                y: 48.0,
                opacity: 0.70,
                is_active: false,
                rotation_deg: 0.0,
            });
        }
        self.render_lines(hwnd, &lines, is_locked, TextAlign::Center);
    }
}

impl Drop for OverlayRenderer {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.hdc_mem, self.old_bitmap);
            let _ = DeleteObject(self.hbitmap);
            let _ = DeleteDC(self.hdc_mem);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_renderer_creation() {
        unsafe {
            let res = OverlayRenderer::new(1000, 90, "Segoe UI", 24.0, 16.0, [255, 255, 255], [0, 0, 0], 1.5);
            match res {
                Ok(_) => println!("OverlayRenderer initialized successfully!"),
                Err(e) => panic!("OverlayRenderer failed to initialize: {:?}", e),
            }
        }
    }

    /// Proves the visualizer actually paints into the layered DIB — bitmap upload,
    /// premultiplied format and slot scaling in one shot. The render target is
    /// bound to the renderer's own DIB, so no window is needed (the final
    /// `UpdateLayeredWindow` fails on a null HWND, which is harmless).
    #[test]
    fn test_visualizer_paints_into_the_dib() {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(
                None,
                windows::Win32::System::Com::COINIT_APARTMENTTHREADED,
            );
        }

        let mut renderer = match unsafe {
            OverlayRenderer::new(600, 90, "Segoe UI", 24.0, 16.0, [255, 255, 255], [0, 0, 0], 1.5)
        } {
            Ok(r) => r,
            Err(e) => panic!("renderer init failed: {:?}", e),
        };
        assert!(renderer.visualizer.is_some(), "embedded art must be uploaded");

        fn painted_pixels(renderer: &OverlayRenderer) -> usize {
            let len = renderer.width as usize * renderer.height as usize * 4;
            let bits = unsafe { std::slice::from_raw_parts(renderer.bits, len) };
            bits.chunks_exact(4).filter(|px| px[3] != 0).count()
        }

        let hwnd = HWND(std::ptr::null_mut());
        let draw = |renderer: &mut OverlayRenderer, visible: bool, scale: f32| {
            renderer.set_visualizer(visible, scale);
            unsafe { renderer.render_lines(hwnd, &[], true, TextAlign::Center) };
            painted_pixels(renderer)
        };

        assert_eq!(draw(&mut renderer, false, 1.0), 0, "hidden visualizer must not paint");

        // On transparent vibe.gif, non-zero alpha pixels exist and scale dynamically.
        // Slot is 74px on a 90px-tall surface, and the art is fully opaque.
        let full = draw(&mut renderer, true, 1.0);
        assert!(full > 500, "visualizer painted only {} pixels", full);
        assert!(full > 1000, "visualizer painted only {} pixels", full);

        // Halving the pulse scale must reduce the covered area.
        let half = draw(&mut renderer, true, 0.5);
        assert!(half < full, "scale ignored: {} px vs {} px", half, full);
    }

    /// The visualizer art is an embedded asset, so this must decode on any
    /// Windows box — a failure here means the binary or the WIC path is broken.
    #[test]
    fn test_visualizer_asset_decodes_to_premultiplied_bgra() {
        unsafe {
            // WIC is COM; the app's UI thread is initialised by `main`, tests are not.
            let _ = windows::Win32::System::Com::CoInitializeEx(
                None,
                windows::Win32::System::Com::COINIT_APARTMENTTHREADED,
            );
        }

        let (w, h, pixels) = vibe_art().expect("assets/vibe.png must decode");
        assert!(*w > 0 && *h > 0);
        assert_eq!(pixels.len(), (*w * *h * 4) as usize);
        assert!(pixels.iter().any(|b| *b != 0), "decoded art is blank");
    }
}
