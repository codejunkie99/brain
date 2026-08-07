// Suppress the extra console window on Windows in release. macOS doesn't
// care, but the convention is harmless and keeps cross-platform builds
// clean if anyone tries them.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    brain_ide_lib::run();
}
