//! LumiaDAM'ın yerel PDF çalışma alanı; kaynak ve kayıt köprüsü ana uygulamaya aittir.
#[cfg(target_arch = "wasm32")]
mod lumiadam_host;
#[cfg(target_arch = "wasm32")]
type Inbox = pdfcraft_ui_egui::Inbox;
#[cfg(target_arch = "wasm32")]
struct LibraryPdf(pdfcraft_ui_egui::PdfCraftApp);
#[cfg(target_arch = "wasm32")]
impl eframe::App for LibraryPdf {
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        if !raw.dropped_files.is_empty() { raw.dropped_files.clear(); lumiadam_host::open_notice(); }
        self.0.raw_input_hook(ctx, raw);
    }
    fn logic(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        self.0.language = lumiadam_host::language();
        self.0.logic(ctx, frame);
    }
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) { self.0.ui(ui, frame); }
    fn save(&mut self, storage: &mut dyn eframe::Storage) { self.0.save(storage); }
}
#[cfg(target_arch = "wasm32")]
fn main() {
    use wasm_bindgen::JsCast;
    wasm_bindgen_futures::spawn_local(async {
        let Some(canvas) = web_sys::window().and_then(|w| w.document())
            .and_then(|d| d.get_element_by_id("pdfcraft"))
            .and_then(|e| e.dyn_into::<web_sys::HtmlCanvasElement>().ok()) else { return };
        let options = eframe::WebOptions { renderer: eframe::Renderer::Glow, ..Default::default() };
        let result = eframe::WebRunner::new().start(canvas, options, Box::new(|cc| {
            let mut app = pdfcraft_ui_egui::PdfCraftApp::new();
            app.language = lumiadam_host::language();
            app.host_save = Some(Box::new(|name, bytes, _| lumiadam_host::write(name, bytes)));
            lumiadam_host::install(app.startup_inbox.clone(), cc.egui_ctx.clone());
            Ok(Box::new(LibraryPdf(app)))
        })).await;
        if let Err(e) = result { web_sys::console::error_1(&e); }
    });
}
#[cfg(not(target_arch = "wasm32"))]
fn main() { eprintln!("Build the LumiaDAM PDF editor with Trunk for wasm32."); }
