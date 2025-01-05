use std::str::FromStr;
use std::fmt::{Display, Formatter};

use actix_web as axw;
use actix_files as axf;
use actix_web::{App, HttpRequest, HttpResponse, HttpServer, Responder, ResponseError};
use actix_web::http::StatusCode;
use actix_web::http::header::ContentType;
use actix_web::body::BoxBody;
use actix_multipart::Multipart;
use actix_multipart::form::MultipartForm;
use actix_multipart::form::tempfile::TempFile;
use mime::Mime;
use server_db::Database;

const API_KEY: String = std::env::var("API_KEY").unwrap();

type Result<T, E = AppError> = std::result::Result<T, E>;

struct AppState {
    db: Database,
}

impl AppState {
    pub fn new() -> Result<AppState, String> {
        Ok(AppState { db: Database::new("files.db")? })
    }
}

#[derive(Debug)]
struct AppError(StatusCode, String);

impl Display for AppError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0.as_str())
    }
}

impl ResponseError for AppError {
    fn status_code(&self) -> StatusCode {
        self.0
    }

    fn error_response(&self) -> HttpResponse {
        HttpResponse::build(self.status_code())
            .insert_header(ContentType::html())
            .body(BoxBody::new(
                match self.0 {
                    StatusCode::INTERNAL_SERVER_ERROR => String::new(),
                    _ => self.1.clone()
                }
            ))
    }
}

impl From<String> for AppError {
    fn from(s: String) -> Self {
        AppError(StatusCode::INTERNAL_SERVER_ERROR, s)
    }
}

impl AppError {
    pub fn not_found() -> Self {
        AppError(StatusCode::NOT_FOUND, String::new())
    }

    pub fn forbidden() -> Self {
        AppError(StatusCode::FORBIDDEN, String::new())
    }
}

macro_rules! handle_result {
    ($result:expr) => {
        $result.map_err(|e| AppError::from(e))?
    };
}

#[axw::get("/status")]
async fn status() -> Result<impl Responder> {
    Ok("OK")
}

#[axw::get("/{code}")]
async fn query(state: axw::web::Data<AppState>, path: axw::web::Path<String>) -> Result<impl Responder> {
    let code = path.into_inner();
    if let Some((hash, content_type)) = handle_result!(state.db.get_hash_for(&code)) {
        return Ok(
            axf::NamedFile::open_async(format!("static/files/{}", hash))
                .await
                .map_err(|e| AppError::from(e.to_string()))?
                .set_content_type(Mime::from_str(&content_type).unwrap())
        );
    }

    Err(AppError::not_found())
}

#[derive(MultipartForm)]
struct UploadForm {
    #[multipart(rename = "file")]
    files: Vec<TempFile>
}

#[axw::post("/upload")]
async fn upload(
    state: axw::web::Data<AppState>,
    req: HttpRequest,
    MultipartForm(form): MultipartForm<UploadForm>
) -> Result<impl Responder> {
    let key = req.headers().get("api-key").ok_or_else(|| AppError::forbidden())?;

}

#[axw::main]
async fn main() -> std::io::Result<()> {
    HttpServer::new(|| {
        App::new()
            .wrap(axw::middleware::Logger::default())
            .app_data(axw::web::Data::new(AppState::new().expect("Failed to open database")))
            .service(status)
            .service(axf::Files::new("/static", "./static/").prefer_utf8(true))
            .service(query)
            .service(upload)
    })
        .bind("127.0.0.1:8080")?
        .run()
        .await
}