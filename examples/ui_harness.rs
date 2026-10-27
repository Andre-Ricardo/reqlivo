//! Harness exclusivo de validação automatizada da interface.
//!
//! Não faz parte do produto: o aplicativo desktop usa IPC com a WebView e não
//! abre listener. Este exemplo serve os mesmos assets embutidos e o roteador
//! em memória num endereço loopback efêmero para que um navegador headless
//! exercite os fluxos de interface contra arquivos sintéticos.

use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use reqlivo::server::{self, AppState};
use reqlivo::store::CollectionStore;
use std::io::Write;

async fn index() -> Response {
    asset(
        include_str!("../src/assets/index.html"),
        "text/html; charset=utf-8",
    )
}

async fn app_js() -> Response {
    asset(
        include_str!("../src/assets/app.js"),
        "text/javascript; charset=utf-8",
    )
}

async fn styles() -> Response {
    asset(
        include_str!("../src/assets/styles.css"),
        "text/css; charset=utf-8",
    )
}

async fn favicon() -> StatusCode {
    StatusCode::NO_CONTENT
}

fn asset(body: &str, content_type: &str) -> Response {
    ([(header::CONTENT_TYPE, content_type)], body.to_string()).into_response()
}

#[tokio::main]
async fn main() {
    let root = std::env::args()
        .nth(1)
        .expect("uso: ui_harness <diretório-da-coleção>");
    let store = CollectionStore::open(&root).expect("a coleção de teste deve abrir");
    let app = server::router(AppState::new(store))
        .route("/", get(index))
        .route("/app.js", get(app_js))
        .route("/styles.css", get(styles))
        .route("/favicon.ico", get(favicon));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener loopback");
    println!("PORT={}", listener.local_addr().expect("endereço").port());
    std::io::stdout().flush().ok();
    axum::serve(listener, app).await.expect("servir harness");
}
