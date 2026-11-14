//! tpt-usb-rescue GUI.
//!
//! A plain Win32 window with no third-party dependencies. It runs the same operations as the
//! command-line tool (`tpt_usb_rescue::ops`), on worker threads, and reports back through
//! window messages.

// Release builds run without a console window. Debug builds keep the console for log output.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(windows)]
mod ui;
#[cfg(windows)]
mod win32;

#[cfg(windows)]
fn main() {
    ui::run();
}

#[cfg(not(windows))]
fn main() {
    eprintln!(
        "tpt-usb-rescue-gui only runs on Windows. Use the tpt-usb-rescue CLI on this platform."
    );
    std::process::exit(1);
}
