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
    render::render_all()
        .await
        .context("Failed to render posts")?;

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
) -> std::io::Result<actix_web::dev::Server> {
    let server = HttpServer::new(|| {
        App::new()
            .service(hi)
            .service(home)
            .service(get_archive)
            .service(get_category)
            .service(get_tag)
            .service(get_static_files)
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
    .bind(("0.0.0.0", port))?
    .run();
    Ok(server)
}

#[get("/")]
async fn home() -> impl Responder {
    get_static_file(String::from("index.html")).await
}

#[get("/hi")]
async fn hi() -> impl Responder {
    HttpResponse::Ok().body("hi")
}

/// Fallback route serving any file under `public/` (posts, pages, assets).
#[get("/{path:.*}")]
async fn get_static_files(path: web::Path<String>) -> impl Responder {
    let name = path.into_inner();
    get_static_file(name).await
}

#[get("/post/{post}")]
async fn get_archive(post: web::Path<String>) -> impl Responder {
    let name = post.into_inner();
    get_static_file(name).await
}

#[get("/categories/{category}")]
async fn get_category(category: web::Path<String>) -> impl Responder {
    let name = category.into_inner();
    get_static_file(name).await
}

#[get("/tags/{tag}")]
async fn get_tag(tag: web::Path<String>) -> impl Responder {
    let name = tag.into_inner();
    get_static_file(name).await
}

async fn get_static_file(name: String) -> impl Responder {
    let safe_path = match validate_and_get_path(&name) {
        Ok(path) => path,
        Err(e) => {
            warn!("BadRequest: request name: {}, error info: {}", name, e);
            return HttpResponse::BadRequest().body("Invalid target");
        }
    };
    match fs::read(safe_path).await {
        Ok(bytes) => HttpResponse::Ok()
            .content_type(content_type(&name))
            .body(bytes),
        Err(_) => HttpResponse::NotFound().body("Target not found"),
    }
}

/// Resolve a request path inside `public/`, rejecting traversal attempts and
/// hidden files such as the `.post_hash.json` cache.
fn validate_and_get_path(file_name: &str) -> Result<PathBuf, &'static str> {
    if file_name.is_empty()
        || file_name.starts_with('/')
        || file_name.contains('\\')
        || file_name
            .split('/')
            .any(|seg| seg.is_empty() || seg == ".." || seg.starts_with('.'))
    {
        return Err("Invalid path");
    }
    Ok(get_public_path(file_name))
}

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
