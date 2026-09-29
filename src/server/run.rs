//! Development HTTP server: static file routes over `public/`.

use std::path::PathBuf;

use actix_cors::Cors;
use actix_files::NamedFile;
use actix_web::{App, HttpResponse, HttpServer, Responder, get, web};
use anyhow::{Context, Result, bail};
use tokio::select;
use tracing::{info, warn};

use crate::server::{self, get_public_path, render};

/// Render the site once, then serve `public/` while watching for changes.
#[tokio::main(flavor = "multi_thread", worker_threads = 10)]
pub async fn run(port: u16) -> Result<()> {
    let (shutdown_tx, _) = tokio::sync::broadcast::channel(1);

    // Render the whole site before serving
    render::render_all().await?;

    let server = init_server(port, shutdown_tx.clone())?;

    // Run file watchers and the HTTP server until one of them finishes
    select! {
        _ = server::start_watch(shutdown_tx) => {},
        _ = server => {},
    }
    Ok(())
}

/// Build the actix-web server with graceful ctrl-c shutdown.
fn init_server(
    port: u16,
    shutdown_tx: tokio::sync::broadcast::Sender<()>,
) -> Result<actix_web::dev::Server> {
    let server = HttpServer::new(|| {
        let cors = Cors::default()
            .allowed_origin("https://giscus.app")
            .allowed_methods(vec!["GET"]);
        App::new().wrap(cors).service(hi).service(get_static_files)
    })
    .shutdown_signal(async move {
        // Wait ctrl_c for quit gracefully
        tokio::signal::ctrl_c()
            .await
            .expect("Failed to listen for ctrl_c");
        let _ = shutdown_tx.send(());
        info!("Received exit signal, shutting down...");
    })
    .shutdown_timeout(60)
    .bind(("0.0.0.0", port))
    .context(format!("Failed to bind port: {port}"))?
    .run();
    Ok(server)
}

#[get("/hi")]
async fn hi() -> impl Responder {
    HttpResponse::Ok().body("hi")
}

/// Route serving any file under `public/` (posts, pages, assets).
#[get("{path:.*}")]
async fn get_static_files(path: web::Path<String>) -> actix_web::Result<NamedFile> {
    let path = path.into_inner();
    let safe_path = validate_and_get_path(&path).map_err(|e| {
        warn!("BadRequest: request file name: {}, error info: {}", path, e);
        actix_web::error::ErrorBadRequest("Invalid target")
    })?;
    NamedFile::open(safe_path).map_err(actix_web::error::ErrorNotFound)
}

/// Resolve a request path inside `public/`, rejecting traversal attempts and
/// hidden files such as the `.post_hash.json` cache.
#[inline]
fn validate_and_get_path(path: &str) -> Result<PathBuf> {
    // the site root is the home page
    let path = if path.is_empty() { "index.html" } else { path };
    if path.starts_with("//")
        || path.contains('\\')
        || path
            .split('/')
            .any(|seg| seg.is_empty() || seg == ".." || seg.starts_with('.'))
    {
        bail!("Invalid request path format");
    }

    let mut p = get_public_path(".");
    path.split("/").for_each(|seg| p = p.join(seg));
    if p.extension().is_none() {
        p = p.join("index.html");
    }

    if p.exists() {
        Ok(p)
    } else {
        bail!("Invalid request path not exists");
    }
}
