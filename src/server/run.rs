use std::path::{Path, PathBuf};

use actix_web::{App, HttpResponse, HttpServer, Responder, get, web};
use anyhow::{Context, Result};
use tokio::{fs, select};
use tracing::{info, warn};

use crate::server::{self, get_public_path, render};

#[tokio::main(flavor = "multi_thread", worker_threads = 10)]
pub async fn run(port: u16) -> Result<()> {
    let (shutdown_tx, _) = tokio::sync::broadcast::channel(1);

    // Render all posts
    render::render_all().await?;

    let server = init_server(port, shutdown_tx.clone())?;

    // Run file watchers and the HTTP server until one of them finishes
    select! {
        _ = server::start_watch(shutdown_tx) => {},
        _ = server => {},
    }
    Ok(())
}

fn init_server(
    port: u16,
    shutdown_tx: tokio::sync::broadcast::Sender<()>,
) -> Result<actix_web::dev::Server> {
    let server = HttpServer::new(|| App::new().service(hi).service(get_static_files))
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
async fn get_static_files(path: web::Path<String>) -> impl Responder {
    let path = path.into_inner();
    get_static_file(path).await
}

async fn get_static_file(path: String) -> impl Responder {
    let safe_path = match validate_and_get_path(&path) {
        Ok(path) => path,
        Err(e) => {
            warn!("BadRequest: request file name: {}, error info: {}", path, e);
            return HttpResponse::BadRequest().body("Invalid target");
        }
    };
    match fs::read(safe_path).await {
        Ok(bytes) => HttpResponse::Ok()
            .content_type(content_type(&path))
            .body(bytes),
        Err(_) => HttpResponse::NotFound().body("Target can not be read"),
    }
}

/// Resolve a request path inside `public/`, rejecting traversal attempts and
/// hidden files such as the `.post_hash.json` cache.
#[inline]
fn validate_and_get_path(path: &str) -> Result<PathBuf, &'static str> {
    // the site root is the home page
    let path = if path.is_empty() { "index.html" } else { path };
    if path.starts_with("//")
        || path.contains('\\')
        || path
            .split('/')
            .any(|seg| seg.is_empty() || seg == ".." || seg.starts_with('.'))
    {
        return Err("Invalid path");
    }

    let mut p = get_public_path(".");
    path.split("/").for_each(|seg| p = p.join(seg));
    if p.extension().is_none() {
        p = p.join("index.html");
    }

    if p.exists() {
        Ok(p)
    } else {
        Err("Invalid path")
    }
}

#[inline]
fn content_type(path: &str) -> &'static str {
    match Path::new(path).extension().and_then(|e| e.to_str()) {
        // extensionless files are rendered HTML pages
        None | Some("html") => "text/html; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("js") => "text/javascript; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("ico") => "image/x-icon",
        Some("json") => "application/json",
        Some("xml") => "application/xml",
        Some("woff2") => "font/woff2",
        _ => "application/octet-stream",
    }
}
