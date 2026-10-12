//! Windows küçük resim sağlayıcılarını ayrı, süre sınırı olan bir süreçte çalıştırır.
use anyhow::{ensure, Context, Result};
use std::path::Path;
use windows::{core::PCWSTR, Win32::{Foundation::SIZE, Graphics::Gdi::*, System::Com::*, UI::Shell::*}};

/// Yalnız küçük resim ister; dosyanın genel simgesi başarılı önizleme sayılmaz.
pub fn generate(source: &Path, output: &Path) -> Result<()> {
    let source = std::fs::canonicalize(source)?;
    // Shell ayrıştırıcısı Win32 genişletilmiş yol önekini kabul etmeyebilir.
    let source = source.to_string_lossy();
    let source = source.strip_prefix(r"\\?\UNC\").map(|path| format!(r"\\{path}"))
        .unwrap_or_else(|| source.strip_prefix(r"\\?\").unwrap_or(&source).to_string());
    let name: Vec<u16> = source.encode_utf16().chain(Some(0)).collect();
    unsafe {
        CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()?;
        let result = (|| -> Result<()> {
            let factory: IShellItemImageFactory = SHCreateItemFromParsingName(PCWSTR(name.as_ptr()), None).context("Cannot create Shell image factory")?;
            let bitmap = factory.GetImage(SIZE { cx: 512, cy: 512 }, SIIGBF_THUMBNAILONLY).context("Windows provider could not generate a thumbnail")?;
            let dc = CreateCompatibleDC(None);
            let result = (|| -> Result<()> {
                ensure!(!dc.is_invalid(), "Cannot create thumbnail device context");
                let mut info = BITMAP::default();
                ensure!(GetObjectW(HGDIOBJ(bitmap.0), std::mem::size_of::<BITMAP>() as i32,
                    Some((&mut info as *mut BITMAP).cast())) != 0, "Cannot inspect Windows thumbnail");
                let (width, height) = (info.bmWidth, info.bmHeight);
                ensure!((1..=2048).contains(&width) && (1..=2048).contains(&height), "Unexpected thumbnail dimensions");
                let mut dib = BITMAPINFO { bmiHeader: BITMAPINFOHEADER { biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: width, biHeight: -height, biPlanes: 1, biBitCount: 32,
                    biCompression: BI_RGB.0, ..Default::default() }, ..Default::default() };
                let mut bytes = vec![0u8; width as usize * height as usize * 4];
                ensure!(GetDIBits(dc, bitmap, 0, height as u32, Some(bytes.as_mut_ptr().cast()), &mut dib, DIB_RGB_COLORS) == height,
                    "Cannot copy Windows thumbnail pixels");
                let opaque = bytes.as_chunks::<4>().0.iter().all(|pixel| pixel[3] == 0);
                for pixel in bytes.as_chunks_mut::<4>().0 {
                    pixel.swap(0, 2);
                    if opaque { pixel[3] = 255; }
                    else if pixel[3] > 0 {
                        // Shell premultiplied BGRA döndürür; PNG düz RGBA saklar.
                        for channel in 0..3 { pixel[channel] = ((pixel[channel] as u32 * 255) / pixel[3] as u32).min(255) as u8; }
                    }
                }
                let image = image::RgbaImage::from_raw(width as u32, height as u32, bytes)
                    .ok_or_else(|| anyhow::anyhow!("Invalid Windows thumbnail pixels"))?;
                if let Some(parent) = output.parent() { std::fs::create_dir_all(parent)?; }
                image.save_with_format(output, image::ImageFormat::Png)?;
                Ok(())
            })();
            if !dc.is_invalid() { let _ = DeleteDC(dc); }
            let _ = DeleteObject(HGDIOBJ(bitmap.0));
            result
        })();
        CoUninitialize();
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_thumbnail_is_local_and_generic_icons_are_rejected() {
        let dir = std::env::temp_dir().join(format!("lumiadam-shell-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let source = dir.join("render.jpg");
        let output = dir.join("cache/preview.png");
        image::RgbImage::from_pixel(128, 64, image::Rgb([30, 160, 230])).save(&source).unwrap();
        generate(&source, &output).unwrap();
        std::fs::remove_file(&source).unwrap();
        let image = image::open(&output).unwrap();
        assert!(image.width() > image.height());
        let unsupported = dir.join("no-preview.lumiadam-test-unknown");
        std::fs::write(&unsupported, b"No thumbnail provider").unwrap();
        assert!(generate(&unsupported, &dir.join("icon.png")).is_err());
        assert!(!dir.join("icon.png").exists());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
