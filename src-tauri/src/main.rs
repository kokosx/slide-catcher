// Bez konsoli w wydaniu na Windows
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    slide_catcher_lib::run()
}
