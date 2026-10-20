// Editör yalnız seçilen kaynağın baytlarını alır; çıktılar LumiaDAM kayıt köprüsüne gider.
use wasm_bindgen::prelude::*;
#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_name = lumiadamLanguage)] pub fn language() -> String;
    #[wasm_bindgen(js_name = lumiadamReady)] fn ready();
    #[wasm_bindgen(js_name = lumiadamNotice)] pub fn open_notice();
    #[wasm_bindgen(catch, js_name = lumiadamExport)] fn export(name: &str, bytes: &[u8]) -> Result<(), JsValue>;
}
pub fn install(inbox: super::Inbox, ctx: egui::Context) {
    let repaint_ctx = ctx.clone();
    let repaint = Closure::<dyn FnMut()>::new(move || repaint_ctx.request_repaint());
    let open = Closure::<dyn FnMut(String, js_sys::Uint8Array)>::new(move |name: String, bytes: js_sys::Uint8Array| {
        inbox.lock().unwrap_or_else(|e| e.into_inner()).push((name, bytes.to_vec()));
        ctx.request_repaint();
    });
    if let Some(window) = web_sys::window() {
        if js_sys::Reflect::set(&window, &JsValue::from_str("lumiadamRepaint"), repaint.as_ref()).is_ok() {
            repaint.forget();
        }
        if js_sys::Reflect::set(&window, &JsValue::from_str("lumiadamOpen"), open.as_ref()).is_ok() {
            open.forget(); ready();
        }
    }
}
pub fn write(name: &str, bytes: &[u8]) -> Result<(), String> {
    export(name, bytes).map_err(|e| if language() == "tr" {
        format!("LumiaDAM dışa aktarımı alamadı: {e:?}")
    } else { format!("LumiaDAM could not receive the export: {e:?}") })
}
