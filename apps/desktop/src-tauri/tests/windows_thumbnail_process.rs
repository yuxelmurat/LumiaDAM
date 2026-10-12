#![cfg(windows)]

#[test]
fn helper_process_generates_preview_without_opening_the_catalog() {
    let dir = std::env::temp_dir().join(format!("lumiadam-thumbnail-process-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let source = dir.join("render & kitchen.jpg");
    let output = dir.join("cache/preview.png");
    image::RgbImage::from_pixel(64, 32, image::Rgb([40, 180, 240])).save(&source).unwrap();
    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_lumiadam-desktop"));
    use std::os::windows::process::CommandExt;
    let result = child.creation_flags(0x08000000).current_dir(&dir)
        .arg("--lumiadam-shell-thumbnail").arg(&source).arg(&output).output().unwrap();
    assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
    std::fs::remove_file(source).unwrap();
    assert!(image::open(output).is_ok());
    std::fs::remove_dir_all(dir).unwrap();
}
