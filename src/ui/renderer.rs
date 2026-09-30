use std::mem::zeroed;
use std::ptr::null_mut;
use windows::core::{w, Interface, PCWSTR};
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_PIXEL_FORMAT,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1CreateFactory, ID2D1DCRenderTarget, ID2D1Factory, ID2D1RenderTarget, ID2D1SolidColorBrush,
    D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_RENDER_TARGET_PROPERTIES,
    D2D1_RENDER_TARGET_TYPE_DEFAULT, D2D1_RENDER_TARGET_USAGE_GDI_COMPATIBLE,
    D2D1_ROUNDED_RECT, D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE,
};
use windows::Win32::Graphics::DirectWrite::{
    DWriteCreateFactory, IDWriteFactory, IDWriteTextFormat, DWRITE_FACTORY_TYPE_SHARED,
    DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT_BOLD,
    DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_TEXT_ALIGNMENT_CENTER,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC, ReleaseDC, SelectObject,
    BITMAPINFO, BITMAPINFOHEADER, BI_RGB, BLENDFUNCTION, DIB_RGB_COLORS, HDC, HGDIOBJ,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetWindowRect, UpdateLayeredWindow, ULW_ALPHA,
};

#[derive(Debug, Clone)]
pub struct RenderLine {
    pub text: String,
    pub y: f32,
    pub opacity: f32,
    pub is_active: bool,
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
}

impl OverlayRenderer {
    pub unsafe fn new(
        width: i32,
        height: i32,
        font_family: &str,
        font_size_line1: f32,
        font_size_line2: f32,
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
            &D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 0.90 },
            None,
        )?;

        // Active primary lyric (Line 1)
        let brush_active = render_target.CreateSolidColorBrush(
            &D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 1.0 },
            None,
        )?;

        // Upcoming preview lyric (Line 2)
        let brush_next = render_target.CreateSolidColorBrush(
            &D2D1_COLOR_F { r: 0.88, g: 0.92, b: 0.96, a: 0.70 },
            None,
        )?;

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
        })
    }

    pub unsafe fn render_lines(
        &mut self,
        hwnd: HWND,
        lines: &[RenderLine],
        is_locked: bool,
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

        // Render each active line with 8-directional drop-shadow for crisp legibility
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
            let height = if line.is_active { 42.0 } else { 32.0 };
            let alpha = line.opacity.clamp(0.0, 1.0);

            // 8-directional outline/shadow to ensure text is clear across light and dark backgrounds
            self.brush_glow.SetOpacity(0.90 * alpha);
            let shadow_offsets = [
                (-1.5, 0.0), (1.5, 0.0), (0.0, -1.5), (0.0, 1.5),
                (-1.2, -1.2), (1.2, -1.2), (-1.2, 1.2), (1.2, 1.2),
                (0.0, 2.0),
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

            // Foreground text
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
            SourceConstantAlpha: 255,
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
            });
        }
        if !line2.is_empty() {
            lines.push(RenderLine {
                text: line2.to_string(),
                y: 48.0,
                opacity: 0.70,
                is_active: false,
            });
        }
        self.render_lines(hwnd, &lines, is_locked);
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
            let res = OverlayRenderer::new(1000, 90, "Segoe UI", 24.0, 16.0);
            match res {
                Ok(_) => println!("OverlayRenderer initialized successfully!"),
                Err(e) => panic!("OverlayRenderer failed to initialize: {:?}", e),
            }
        }
    }
}
