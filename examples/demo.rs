#![deny(warnings)]
#![warn(clippy::all)]

use egui_wgpu::{storage::FileStorage, RunMode};

fn main() {
    wgpu_subscriber::initialize_default_subscriber(None);
    let title = "Egui wgpu demo";
    let storage = FileStorage::from_path(".egui_demo_wgpu.json".into());
    let app: egui::DemoApp = egui::app::get_value(&storage, egui::app::APP_KEY).unwrap_or_default();
    egui_wgpu::run(title, RunMode::Reactive, storage, app);
}
