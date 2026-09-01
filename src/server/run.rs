use std::path::PathBuf;

use actix_web::{App, HttpResponse, HttpServer, Responder, get, web};
use tokio::{fs, select};
use tracing::{info, warn};

use crate::{
    result_matcher,
    server::{self, BASE_DIR, get_public_path, render},
};

#[tokio::main(flavor = "multi_thread", worker_threads = 10)]
pub async fn run(port: u16) -> std::io::Result<()> {
    // Start watching file change
    let (shutdown_tx, _) = tokio::sync::broadcast::channel(1);

    // Render all posts
    result_matcher!(render::render_all().await, "Failed to render posts");

    // Initialize the server
    let server = init_server(port, shutdown_tx.clone())?;

    // Run the server
    select! {
        _ = server::start_watch(shutdown_tx) => {},
        _ = server => {},
    }
    Ok(())
}

fn init_server(
    port: u16,
    shutdown_tx: tokio::sync::broadcast::Sender<()>,
) -> Result<actix_web::dev::Server, std::io::Error> {
    let server = HttpServer::new(|| {
        App::new()
            .service(hi)
            .service(get_page)
            .service(get_archive)
            .service(get_category)
            .service(get_tag)
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

#[get("/hi")]
async fn hi() -> impl Responder {
    HttpResponse::Ok().body("hi")
}

#[get("/{page}")]
async fn get_page(page: web::Path<String>) -> impl Responder {
    let name = page.into_inner();
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
    match fs::read_to_string(safe_path).await {
        Ok(html) => HttpResponse::Ok()
            .content_type("text/html; charset=utf-8")
            .body(html),
        Err(_) => HttpResponse::NotFound().body("Target not found"),
    }
}

fn validate_and_get_path(file_name: &str) -> Result<PathBuf, &'static str> {
    if file_name.contains("..") || file_name.contains('/') || file_name.contains('\\') {
        return Err("Invalid path");
    }

    if !file_name
        .chars()
        .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
    {
        return Err("Invalid characters");
    }

    let full_path = get_public_path(file_name);

    if !full_path.starts_with(BASE_DIR.to_path_buf()) {
        return Err("Path traversal detected");
    }

    Ok(full_path)
}
