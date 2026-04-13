// CarryTalk — Tauri entry point
// Thin main: delegates everything to lib.rs

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    carry_talk_lib::run();
}
