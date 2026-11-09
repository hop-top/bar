//! wasm-bindgen exports over `hop_bar::web`; JSON results are strings.

use wasm_bindgen::prelude::wasm_bindgen;

/// The status line as ANSI text; `now` in Unix seconds.
#[wasm_bindgen]
pub fn render(config_text: &str, session_json: &str, columns: usize, now: f64) -> String {
    hop_bar::web::render(config_text, session_json, columns, now as i64)
}

/// `bar check` issues, JSON.
#[wasm_bindgen]
pub fn check(config_text: &str) -> String {
    hop_bar::web::check(config_text).to_string()
}

/// Lines and settings of a config, JSON.
#[wasm_bindgen]
pub fn inspect(config_text: &str) -> String {
    hop_bar::web::inspect(config_text).to_string()
}

/// One group's tokens, JSON.
#[wasm_bindgen]
pub fn tokens(src: &str) -> String {
    hop_bar::web::tokens(src).to_string()
}

/// Placeholders, options, presets and settings, JSON.
#[wasm_bindgen]
pub fn schema() -> String {
    hop_bar::web::schema().to_string()
}

/// A preset's text; `undefined` for an unknown name.
#[wasm_bindgen]
pub fn preset(name: &str) -> Option<String> {
    hop_bar::web::preset(name).map(str::to_owned)
}
