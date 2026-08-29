use std::sync::Mutex;

use actix_web::{App, HttpRequest, HttpResponse, HttpServer, Responder, get, post, web};
use tera::Context;

use crate::{
    result_matcher,
    server::{self, SITE, TERA, get_public_path, render},
};

#[tokio::main(flavor = "multi_thread", worker_threads = 10)]
pub async fn run(port: u16) -> std::io::Result<()> {
    // Start watching file change
    let (shutdown_tx, _) = tokio::sync::broadcast::channel(1);
    server::start_watch(shutdown_tx.clone());

    // Render all posts
    result_matcher!(render::render_all().await, "Failed to render posts");

    // Initialize the server
    let server = init_server(port, shutdown_tx)?;

    // Run the server
    server.await
}

#[derive(Debug)]
struct AppState {
    ak: String,
    allows: Mutex<Vec<String>>,
}

fn init_server(
    port: u16,
    shutdown_tx: tokio::sync::broadcast::Sender<()>,
) -> Result<actix_web::dev::Server, std::io::Error> {
    let server = HttpServer::new(|| {
        App::new()
            .service(hello)
            .service(login)
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
        println!("\nReceived exit signal, shutting down...");
    })
    .shutdown_timeout(60)
    .bind(("0.0.0.0", port))?
    .run();
    Ok(server)
}

#[get("/")]
async fn hello() -> impl Responder {
    HttpResponse::Ok().body("Hello world!")
}

#[get("/{page}")]
async fn get_page(page: web::Path<String>) -> impl Responder {
    let page_name = page.into_inner();
    let mut context = Context::new();
    let site = &SITE.load();
    context.insert("post", &site.posts);
    context.insert("tags", &site.tags);
    context.insert("categories", &site.categories);
    context.insert("page", &page_name);
    match TERA
        .load()
        .render(format!("{}.html", page_name).as_str(), &context)
    {
        Ok(res) => HttpResponse::Ok().body(res),
        Err(_) => HttpResponse::ExpectationFailed().body("Failed to render page"),
    }
}

fn get_real_ip(req: &HttpRequest) -> String {
    req.connection_info()
        .realip_remote_addr()
        .unwrap_or("unknown")
        .to_string()
}

#[post("/login")]
async fn login(
    ak: web::Json<String>,
    req: HttpRequest,
    data: web::Data<AppState>,
) -> impl Responder {
    if data.ak == *ak {
        let real_ip = get_real_ip(&req);
        let mut allows = data.allows.lock().unwrap();
        if !allows.contains(&real_ip) {
            allows.push(real_ip);
        }
        HttpResponse::Ok().body("AK passed")
    } else {
        HttpResponse::Unauthorized().body("AK failed")
    }
}

#[get("/archives/{post}")]
async fn get_archive(
    post: web::Path<String>,
    req: HttpRequest,
    data: web::Data<AppState>,
) -> impl Responder {
    let post_name = post.into_inner();
    let is_private = SITE
        .load()
        .posts
        .iter()
        .any(|p| p.title == post_name && p.prva);
    if is_private {
        let real_ip = get_real_ip(&req);
        let allows = data.allows.lock().unwrap();
        if !allows.contains(&real_ip) {
            return HttpResponse::Forbidden().body("No right to access private post");
        }
    }

    match tokio::fs::read_to_string(get_public_path(&post_name)).await {
        Ok(html) => HttpResponse::Ok()
            .content_type("text/html; charset=utf-8")
            .body(html),
        Err(_) => HttpResponse::NotFound().body("Post not found"),
    }
}

#[get("/categories/{category}")]
async fn get_category(category: web::Path<String>) -> impl Responder {
    let category_name = category.into_inner();
    let mut context = Context::new();
    let site = &SITE.load();
    context.insert("post", &site.posts);
    context.insert("tags", &site.tags);
    context.insert("categories", &site.categories);
    context.insert("name", &category_name);
    HttpResponse::Ok().body(TERA.load().render("category.html", &context).unwrap())
}

#[get("/tags/{tag}")]
async fn get_tag(tag: web::Path<String>) -> impl Responder {
    let tag_name = tag.into_inner();
    let mut context = Context::new();
    let site = &SITE.load();
    context.insert("post", &site.posts);
    context.insert("tags", &site.tags);
    context.insert("categories", &site.categories);
    context.insert("name", &tag_name);
    HttpResponse::Ok().body(TERA.load().render("category.html", &context).unwrap())
}
