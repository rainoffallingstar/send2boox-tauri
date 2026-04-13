#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod api;
mod app;
mod auth;
mod dashboard;
mod device;
mod diagnostics;
mod models;
mod push;
mod state;
mod util;

fn main() {
    app::run();
}
