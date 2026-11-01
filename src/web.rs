//! En la web: avisa a la página cuando el juego ya se ve, para que quite el
//! cartel de carga (ver `web/index.html`).

use bevy::prelude::*;

pub struct WebPlugin;

impl Plugin for WebPlugin {
    fn build(&self, app: &mut App) {
        // El primer fotograma es el que prepara el mundo, lo que más tarda.
        app.add_systems(Last, tell_the_page.run_if(run_once));
    }
}

/// Marca la página con `data-motorust="ready"`.
fn tell_the_page() {
    let page = web_sys::window()
        .and_then(|window| window.document())
        .and_then(|document| document.document_element());
    if let Some(page) = page
        && page.set_attribute("data-motorust", "ready").is_err()
    {
        warn!("No se puede avisar a la página de que el juego ya se ve");
    }
}
