use std::mem;

use winapi::um::wingdi::{
    CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDIBits, PatBlt,
    SelectObject, SetStretchBltMode, StretchBlt, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, BLACKNESS,
    DIB_RGB_COLORS, HALFTONE, SRCCOPY,
};
use winapi::um::winuser::{
    GetDC, GetDesktopWindow, GetSystemMetrics, ReleaseDC, SM_CXSCREEN, SM_CYSCREEN,
};

#[derive(Clone, Copy)]
pub enum FitMode {
    Contain,
    Cover,
}

pub fn screen_size() -> (u32, u32) {
    unsafe {
        (
            GetSystemMetrics(SM_CXSCREEN).max(1) as u32,
            GetSystemMetrics(SM_CYSCREEN).max(1) as u32,
        )
    }
}

/// Captura la pantalla y la ajusta al tamaño de salida.
/// Contain: mantiene todo visible con bandas negras.
/// Cover: llena toda la salida recortando bordes si hace falta.
pub fn capture_jpeg_to_size(
    quality: u8,
    out_w: u32,
    out_h: u32,
    fit_mode: FitMode,
) -> anyhow::Result<Vec<u8>> {
    let (w, h, pixels) = unsafe { capture_gdi_to_size(out_w.max(1), out_h.max(1), fit_mode)? };
    encode_jpeg(&pixels, w, h, quality)
}

unsafe fn capture_gdi_to_size(
    out_w: u32,
    out_h: u32,
    fit_mode: FitMode,
) -> anyhow::Result<(u32, u32, Vec<u8>)> {
    let hwnd = GetDesktopWindow();
    let screen_dc = GetDC(hwnd);
    if screen_dc.is_null() {
        anyhow::bail!("GetDC failed");
    }

    let w = GetSystemMetrics(SM_CXSCREEN).max(1) as u32;
    let h = GetSystemMetrics(SM_CYSCREEN).max(1) as u32;
    if w == 0 || h == 0 {
        ReleaseDC(hwnd, screen_dc);
        anyhow::bail!("invalid screen dimensions {w}x{h}");
    }

    let dst_dc = CreateCompatibleDC(screen_dc);
    let dst_bmp = CreateCompatibleBitmap(screen_dc, out_w as i32, out_h as i32);
    let dst_old_obj = SelectObject(dst_dc, dst_bmp as *mut _);

    PatBlt(dst_dc, 0, 0, out_w as i32, out_h as i32, BLACKNESS);

    let scale = match fit_mode {
        FitMode::Contain => f32::min(out_w as f32 / w as f32, out_h as f32 / h as f32),
        FitMode::Cover => f32::max(out_w as f32 / w as f32, out_h as f32 / h as f32),
    };
    let draw_w = (w as f32 * scale).round().max(1.0) as i32;
    let draw_h = (h as f32 * scale).round().max(1.0) as i32;
    let off_x = ((out_w as i32 - draw_w) / 2).max(0);
    let off_y = ((out_h as i32 - draw_h) / 2).max(0);

    SetStretchBltMode(dst_dc, HALFTONE);
    StretchBlt(
        dst_dc, off_x, off_y, draw_w, draw_h, screen_dc, 0, 0, w as i32, h as i32, SRCCOPY,
    );

    let mut bmi: BITMAPINFO = mem::zeroed();
    bmi.bmiHeader.biSize = mem::size_of::<BITMAPINFOHEADER>() as u32;
    bmi.bmiHeader.biWidth = out_w as i32;
    bmi.bmiHeader.biHeight = -(out_h as i32); // negativo = top-down
    bmi.bmiHeader.biPlanes = 1;
    bmi.bmiHeader.biBitCount = 32;
    bmi.bmiHeader.biCompression = BI_RGB;

    let mut pixels = vec![0u8; (out_w * out_h * 4) as usize];
    GetDIBits(
        dst_dc,
        dst_bmp,
        0,
        out_h,
        pixels.as_mut_ptr() as *mut _,
        &mut bmi,
        DIB_RGB_COLORS,
    );

    SelectObject(dst_dc, dst_old_obj);
    DeleteObject(dst_bmp as *mut _);
    DeleteDC(dst_dc);
    ReleaseDC(hwnd, screen_dc);

    Ok((out_w, out_h, pixels))
}

fn encode_jpeg(
    pixels: &[u8], // BGRA, top-down
    w: u32,
    h: u32,
    quality: u8,
) -> anyhow::Result<Vec<u8>> {
    // Convertir BGRA → RGB
    let mut rgb = Vec::with_capacity((w * h * 3) as usize);
    for y in 0..h {
        let src_y = y as usize;
        for x in 0..w {
            let src_x = x as usize;
            let idx = (src_y * w as usize + src_x) * 4;
            rgb.push(pixels[idx + 2]); // R  (BGRA→RGB: swap B y R)
            rgb.push(pixels[idx + 1]); // G
            rgb.push(pixels[idx]); // B
        }
    }

    let mut jpeg = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, quality).encode(
        &rgb,
        w,
        h,
        image::ExtendedColorType::Rgb8,
    )?;

    Ok(jpeg)
}
