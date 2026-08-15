#![allow(dead_code, unused_imports, unused_variables, unused_must_use)]
// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    space_toggle_os_lib::run()
}
