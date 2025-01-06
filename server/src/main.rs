use std::str::FromStr;
use std::fmt::{Display, Formatter, Debug};
use std::io::Read;
use std::pin::Pin;
use std::error::Error;

use actix_web as axw;
use actix_files as axf;
use actix_web::{App, HttpRequest, HttpResponse, HttpServer, Responder, ResponseError};
use actix_web::http::StatusCode;
use actix_web::body::BoxBody;
use actix_multipart::form::MultipartForm;
use actix_multipart::form::tempfile::TempFile;
use actix_web::http::header::{ContentDisposition, DispositionParam, DispositionType};
use mime::Mime;
use sha2::{Digest, Sha256};
use rand::distributions::{Alphanumeric, DistString};
use dotenv::dotenv;
use server_db::Database;


type Result<T, E = AppError> = std::result::Result<T, E>;

struct AppState {
    db: Database,
    api_key: Pin<String>,
    base_url: Pin<String>
}

impl AppState {
    pub fn new(api_key: Pin<String>, base_url: Pin<String>) -> Result<AppState, String> {
        Ok(AppState { db: Database::new("files.db")?, api_key, base_url })
    }
}

struct AppError(StatusCode, String);

impl AppError {
    pub fn not_found() -> Self {
        AppError(StatusCode::NOT_FOUND, String::new())
    }

    pub fn forbidden() -> Self {
        AppError(StatusCode::FORBIDDEN, String::new())
    }

    pub fn readable_string(&self) -> String {
        format!("HTTP {}: {}", self.0, self.1)
    }
}

impl Display for AppError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.readable_string())
    }
}

impl Debug for AppError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.readable_string())
    }
}

impl Error for AppError {}

impl ResponseError for AppError {
    fn status_code(&self) -> StatusCode {
        self.0
    }

    fn error_response(&self) -> HttpResponse {
        HttpResponse::build(self.status_code())
            .body(BoxBody::new(
                match self.0 {
                    StatusCode::INTERNAL_SERVER_ERROR => {
                        // TODO: this should not be here, but I can't bother implementing a middleware to log this
                        log::error!("{}", &self);
                        String::new()
                    },
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
async fn query(state: axw::web::Data<AppState>, path: axw::web::Path<(String,)>) -> Result<impl Responder> {
    let code = path.into_inner().0;
    if let Some((hash, content_type, file_ext)) = handle_result!(state.db.get_file_for(&code)) {
        return Ok(
            axf::NamedFile::open_async(format!("files/{}", hash))
                .await
                .map_err(|e| AppError::from(e.to_string()))?
                .set_content_type(Mime::from_str(&content_type).unwrap())
                .set_content_disposition(ContentDisposition {
                    disposition: DispositionType::Inline,
                    parameters: vec![DispositionParam::Filename(format!("{}.{}", &code, &file_ext))],
                })
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
    let req_api_key = req.headers().get("api-key").ok_or_else(|| AppError::forbidden())?;
    if !state.api_key.as_bytes().eq(req_api_key.as_bytes()) {
        return Err(AppError::forbidden());
    }

    let mut code = String::new();

    for mut f in form.files {
        let mut hasher = Sha256::new();
        let mut buf: [u8; 4096] = [0; 4096];
        loop {
            let n = f.file.read(&mut buf).map_err(|e| AppError::from(e.to_string()))?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
        }

        // todo: check if hash already exists
        let hash = hex::encode(hasher.finalize().as_slice());
        let path = format!("files/{}", &hash);
        code = Alphanumeric.sample_string(&mut rand::thread_rng(), 16);
        let content_type = f.content_type.map_or(String::from("application/octet-stream"), |m| m.to_string());
        let file_ext = match f.file_name.as_ref() {
            Some(file_name) => file_name.rsplitn(2, '.').next().unwrap_or(""),
            None => ""
        };

        f.file.persist(path).map_err(|e| AppError::from(e.to_string()))?;

        state.db.add_file(&code, &hash, &content_type, file_ext)?;
    }

    Ok(
        HttpResponse::build(StatusCode::OK)
            .body(String::from(format!("{{\"url\": \"{}{}\"}}", state.base_url, &code)))
    )
}

#[axw::main]
async fn main() -> std::io::Result<()> {
    dotenv().ok();
    let api_key = Pin::new(std::env::var("API_KEY").unwrap());
    let base_url = Pin::new(std::env::var("BASE_URL").unwrap());
    let port = std::env::var("PORT").unwrap();
    let log_level = std::env::var("LOG_LEVEL").unwrap_or(String::from("info"));

    std::env::set_var("RUST_LOG", log_level);
    env_logger::init();

    HttpServer::new(move || {
        App::new()
            .wrap(axw::middleware::Logger::default())
            .app_data(axw::web::Data::new(AppState::new(api_key.clone(), base_url.clone()).expect("Failed to open database")))
            .service(axf::Files::new("/static", "./static/").prefer_utf8(true))
            .service(status)
            .service(upload)
            .service(query)
    })
        .bind(("127.0.0.1", u16::from_str(&port).unwrap()))?
        .run()
        .await
}