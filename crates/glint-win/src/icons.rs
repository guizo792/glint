//! Real file and app icons as PNG bytes. Slow (around 20 ms each), so callers run this off the
//! UI path and cache the result.
use crate::wide;
use windows::core::{Error, Result, PCWSTR};
use windows::Win32::Foundation::{E_FAIL, SIZE};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, DeleteDC, DeleteObject, GetDIBits, GetObjectW, BITMAP, BITMAPINFO,
    BITMAPINFOHEADER, DIB_RGB_COLORS, HBITMAP,
};
use windows::Win32::UI::Shell::{
    IShellItemImageFactory, SHCreateItemFromParsingName, SIIGBF_BIGGERSIZEOK, SIIGBF_ICONONLY,
};

/// Icon for a path or `shell:` path as a PNG. Needs COM initialised on this thread.
pub fn png(target: &str, size: i32) -> Result<Vec<u8>> {
    let target = wide(target);
    // SAFETY: the name outlives the calls and the bitmap is deleted exactly once.
    unsafe {
        let factory: IShellItemImageFactory =
            SHCreateItemFromParsingName(PCWSTR(target.as_ptr()), None)?;
        let bitmap = factory.GetImage(
            SIZE { cx: size, cy: size },
            SIIGBF_ICONONLY | SIIGBF_BIGGERSIZEOK,
        )?;
        let encoded = encode(bitmap);
        let _ = DeleteObject(bitmap.into());
        encoded
    }
}

/// # Safety
/// `bitmap` must be a valid bitmap handle.
unsafe fn encode(bitmap: HBITMAP) -> Result<Vec<u8>> {
    let mut info = BITMAP::default();
    let got = GetObjectW(
        bitmap.into(),
        size_of::<BITMAP>() as i32,
        Some(&mut info as *mut _ as *mut _),
    );
    if got == 0 || info.bmWidth <= 0 || info.bmHeight == 0 {
        return Err(Error::from(E_FAIL));
    }
    let (w, h) = (info.bmWidth, info.bmHeight.abs());
    let mut header = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: w,
            biHeight: -h, // negative asks for top-down rows
            biPlanes: 1,
            biBitCount: 32,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut pixels = vec![0u8; (w * h * 4) as usize];
    let dc = CreateCompatibleDC(None);
    let lines = GetDIBits(
        dc,
        bitmap,
        0,
        h as u32,
        Some(pixels.as_mut_ptr() as *mut _),
        &mut header,
        DIB_RGB_COLORS,
    );
    let _ = DeleteDC(dc);
    if lines == 0 {
        return Err(Error::from(E_FAIL));
    }
    for px in pixels.as_chunks_mut::<4>().0 {
        px.swap(0, 2); // BGRA to RGBA
    }
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, w as u32, h as u32);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(|_| Error::from(E_FAIL))?;
    writer
        .write_image_data(&pixels)
        .map_err(|_| Error::from(E_FAIL))?;
    writer.finish().map_err(|_| Error::from(E_FAIL))?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn produces_a_png_for_a_system_file() {
        crate::init_com();
        let bytes = png("C:\\Windows\\explorer.exe", 32).unwrap();
        assert_eq!(&bytes[1..4], b"PNG");
        assert!(bytes.len() > 100);
    }

    #[test]
    fn missing_target_is_an_error() {
        crate::init_com();
        assert!(png("C:\\glint-no-such-file-xyz.exe", 32).is_err());
    }
}
