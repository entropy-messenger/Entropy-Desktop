#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    
    entropy_lib::run();
}
