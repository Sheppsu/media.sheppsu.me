use actix_web as axw;
use actix_files as axf;
use actix_web::{App, HttpRequest, HttpServer, Responder};

use server_db::Database;

type Result<T> = std::result::Result<T, String>;

struct AppState {
    db: Database
}

impl AppState {
    pub fn new() -> Result<AppState> {
        Ok(AppState { db: Database::new("files.db")? })
    }
}

#[axw::get("/status")]
async fn status() -> impl Responder {
    "OK"
}

#[axw::main]
async fn main() -> std::io::Result<()> {
    HttpServer::new(|| {
        App::new()
            .app_data(axw::web::Data::new(AppState::new().expect("Failed to open database")))
            .service(status)
            .service(axf::Files::new("/static", "./static/").prefer_utf8(true))
    })
        .bind("127.0.0.1:8080")?
        .run()
        .await
}