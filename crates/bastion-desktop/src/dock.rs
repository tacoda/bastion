//! The castle in the Dock (macOS). A binary run outside an app bundle shows
//! the Dock's generic icon, and the window's own icon does not reach the
//! Dock, so AppKit is told the image directly.

/// The castle, as a PNG, from `assets/icon/bastion.svg`.
#[cfg(target_os = "macos")]
const PNG: &[u8] = include_bytes!("../../../assets/icon/bastion-512.png");

/// Set the Dock's icon. Only on the main thread, after the app is running.
#[cfg(target_os = "macos")]
pub fn show_icon() {
    use objc2::AllocAnyThread;
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSApplication, NSImage};
    use objc2_foundation::NSData;
    let Some(main) = MainThreadMarker::new() else {
        return;
    };
    let data = NSData::with_bytes(PNG);
    let Some(image) = NSImage::initWithData(NSImage::alloc(), &data) else {
        return;
    };
    let app = NSApplication::sharedApplication(main);
    unsafe { app.setApplicationIconImage(Some(&image)) };
}

/// Elsewhere the window's icon is the app's.
#[cfg(not(target_os = "macos"))]
pub fn show_icon() {}
