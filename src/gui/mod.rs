pub mod app;

use anyhow::Result;
use eframe::egui;

use crate::config::Config;
use self::app::ConfigApp;

pub fn configure_fallback_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    let candidate_paths = [
        // Fedora / RHEL
        "/usr/share/fonts/google-noto-emoji-fonts/NotoEmoji-Regular.ttf",
        "/usr/share/fonts/gdouros-symbola/Symbola.ttf",
        "/usr/share/fonts/dejavu-sans-fonts/DejaVuSans.ttf",
        "/usr/share/fonts/google-noto/NotoSansSymbols-Regular.ttf",
        "/usr/share/fonts/google-noto/NotoSansSymbols2-Regular.ttf",
        // Debian / Ubuntu
        "/usr/share/fonts/truetype/noto/NotoEmoji-Regular.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
        "/usr/share/fonts/truetype/ancient-scripts/Symbola.ttf",
        // Arch Linux
        "/usr/share/fonts/noto/NotoEmoji-Regular.ttf",
        "/usr/share/fonts/TTF/DejaVuSans.ttf",
    ];

    let mut loaded_any = false;
    for path in candidate_paths {
        if let Ok(data) = std::fs::read(path) {
            let name = std::path::Path::new(path)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("fallback_font")
                .to_string();

            if fonts.font_data.contains_key(&name) {
                continue;
            }

            fonts.font_data.insert(
                name.clone(),
                std::sync::Arc::new(egui::FontData::from_owned(data)),
            );

            fonts
                .families
                .entry(egui::FontFamily::Proportional)
                .or_default()
                .push(name.clone());

            fonts
                .families
                .entry(egui::FontFamily::Monospace)
                .or_default()
                .push(name);

            loaded_any = true;
        }
    }

    if loaded_any {
        ctx.set_fonts(fonts);
    }
}

pub fn run_gui(config: Config) -> Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([580.0, 720.0])
            .with_min_inner_size([460.0, 520.0])
            .with_title("OpenWhisper Settings")
            .with_app_id("net.local.openwhisper.settings")
            .with_transparent(false),
        ..Default::default()
    };

    eframe::run_native(
        "OpenWhisper Settings",
        options,
        Box::new(|cc| {
            configure_fallback_fonts(&cc.egui_ctx);
            Ok(Box::new(ConfigApp::new(cc, config)))
        }),
    )
    .map_err(|e| anyhow::anyhow!("GUI error: {e}"))?;

    Ok(())
}
