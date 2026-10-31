//! Gitto's desktop app. Everything is in the library, so it can be tested; this only runs it.

// A release build on Windows is a GUI app: started from Explorer or the Start menu, it opens no
// console beside its window.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    app::run();
}
