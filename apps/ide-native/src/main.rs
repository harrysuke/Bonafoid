use anyhow::Result;
use eframe::egui;
use nocode_ide::IdeApp;

/// eframe wrapper that drives the egui IDE shell.
struct NativeIde {
    app: IdeApp,
}

impl Default for NativeIde {
    fn default() -> Self {
        Self { app: IdeApp::new() }
    }
}

impl eframe::App for NativeIde {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.app.update(ctx);
    }
}

fn main() -> Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("No-Code IDE")
            .with_inner_size([1400.0, 900.0])
            .with_min_inner_size([1024.0, 768.0]),
        ..Default::default()
    };

    eframe::run_native(
        "No-Code IDE",
        options,
        Box::new(|_cc| Ok(Box::<NativeIde>::default())),
    )
    .map_err(|err| anyhow::anyhow!("failed to run native IDE: {err}"))?;

    Ok(())
}
