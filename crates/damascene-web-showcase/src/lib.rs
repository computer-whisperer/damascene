//! Browser entry point for the Damascene showcase demo.

#[cfg(target_arch = "wasm32")]
mod entry {
    use damascene_fixtures::Showcase;
    use wasm_bindgen::prelude::*;

    #[wasm_bindgen(start)]
    pub fn start_web_showcase() {
        let _ = damascene_web::start_with(damascene_web::VIEWPORT, Showcase::new());
    }
}
