pub mod app;

use anyhow::Result;
use eframe::egui;

use crate::config::Config;
use self::app::ConfigApp;

pub fn run_gui(config: Config) -> Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([580.0, 720.0])
            .with_min_inner_size([460.0, 520.0])
            .with_title("OpenWhisper Settings"),
        ..Default::default()
    };

    eframe::run_native(
        "OpenWhisper Settings",
        options,
        Box::new(|cc| Ok(Box::new(ConfigApp::new(cc, config)))),
    )
    .map_err(|e| anyhow::anyhow!("GUI error: {e}"))?;

    Ok(())
}
