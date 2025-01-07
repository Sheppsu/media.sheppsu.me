use std::str::FromStr;
use std::fmt::{Display, Formatter, Debug};
use std::io::Read;
use std::error::Error;
use std::sync::{Arc, Mutex};
use std::pin::Pin;
use std::task::{Context, Poll};
use std::future::Future;

use actix_web as axw;
use actix_files as axf;
use actix_web::{App, HttpRequest, HttpResponse, HttpServer, Responder, ResponseError};
use actix_web::http::{Method, StatusCode};
use actix_web::body::{BoxBody, SizedStream};
use actix_multipart::form::MultipartForm;
use actix_multipart::form::tempfile::TempFile;
use actix_web::http::header::{ContentDisposition, DispositionParam, DispositionType};
use mime::Mime;
use sha2::{Digest, Sha256};
use rand::distributions::{Alphanumeric, DistString};
use dotenv::dotenv;
use futures_core::stream::Stream;
use bytes::{Bytes, BytesMut};
use futures_core::future::BoxFuture;
use tokio::fs;
use tokio::io::AsyncReadExt;

use server_db::AsyncDatabase;


type Result<T, E = AppError> = std::result::Result<T, E>;

struct AppState {
    db: AsyncDatabase,
    api_key: Arc<String>,
    base_url: Arc<String>
}

impl AppState {
    pub fn new(api_key: Arc<String>, base_url: Arc<String>) -> Result<AppState, String> {
        Ok(AppState {
            db: AsyncDatabase::new("files.db")?,
            api_key,
            base_url
        })
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

    (to_str; $result:expr) => {
        $result.map_err(|e| AppError::from(e.to_string()))?
    }
}

#[axw::get("/status")]
async fn status() -> Result<impl Responder> {
    Ok("OK")
}

struct FileStreamState {
    file_fut: Option<BoxFuture<'static, std::io::Result<fs::File>>>,
    read_fut: Option<BoxFuture<'static, Result<(Bytes, Option<fs::File>), AppError>>>
}

impl FileStreamState {
    pub fn new(path: &str) -> Self {
        let path = String::from(path);
        FileStreamState {
            file_fut: Some(Box::pin(fs::File::open(path))),
            read_fut: None
        }
    }

    pub fn empty() -> Self {
        FileStreamState {
            file_fut: None,
            read_fut: Some(Box::pin(async { Ok((Bytes::new(), None)) }))
        }
    }
}

struct FileStream(Arc<Mutex<FileStreamState>>);

impl FileStream {
    pub fn from_path(path: &str) -> Self {
        FileStream(Arc::new(Mutex::new(FileStreamState::new(path))))
    }

    pub fn empty() -> Self {
        FileStream(Arc::new(Mutex::new(FileStreamState::empty())))
    }

    fn __make_read_fut(mut file: fs::File) -> BoxFuture<'static, Result<(Bytes, Option<fs::File>), AppError>> {
        Box::pin(async move {
            let mut buf = BytesMut::zeroed(2097152);  // 2MiB
            let len = handle_result!(to_str; file.read(&mut buf).await);
            if len == 0 {
                Ok((Bytes::new(), None))
            } else {
                Ok((buf.split_to(len).freeze(), Some(file)))
            }
        })
    }
}

impl Stream for FileStream {
    type Item = Result<Bytes, AppError>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut state = self.0.lock().unwrap();

        // check file future (opening the file)
        if let Some(mut file_fut) = state.file_fut.take() {
            return match Pin::new(&mut file_fut).poll(cx) {
                Poll::Pending => {
                    state.file_fut.replace(file_fut);
                    Poll::Pending
                },
                Poll::Ready(result) => {
                    match result {
                        Ok(file) => {
                            state.read_fut.replace(FileStream::__make_read_fut(file));
                            // immediately wake to start polling the read future
                            cx.waker().clone().wake();
                            Poll::Pending
                        },
                        Err(e) => Poll::Ready(Some(Err(AppError::from(e.to_string()))))
                    }
                }
            };
        }

        // check read future (reading bytes from the file)
        if let Some(mut read_fut) = state.read_fut.take() {
            return match Pin::new(&mut read_fut).poll(cx) {
                Poll::Pending => {
                    state.read_fut.replace(read_fut);
                    Poll::Pending
                },
                Poll::Ready(result) => {
                    match result {
                        Ok((bytes, file)) => {
                            if let Some(file) = file {
                                state.read_fut.replace(FileStream::__make_read_fut(file));
                            }
                            Poll::Ready(Some(Ok(bytes)))
                        },
                        Err(e) => Poll::Ready(Some(Err(e)))
                    }
                }
            }
        }

        // Done reading
        Poll::Ready(None)
    }
}

#[axw::route("/{code}", method = "GET", method = "HEAD")]
async fn query(state: axw::web::Data<AppState>, method: Method, path: axw::web::Path<(String,)>) -> Result<impl Responder> {
    let code = path.into_inner().0;
    if let Some((hash, content_type, file_ext)) = handle_result!(state.db.get_file_for(&code).await) {
        handle_result!(state.db.update_file_stats(&code).await);

        let path = format!("files/{}", hash);
        let file_md = handle_result!(to_str; std::fs::metadata(&path));

        let stream = match method {
            Method::GET => FileStream::from_path(&path),
            Method::HEAD => FileStream::empty(),
            _ => unreachable!()
        };

        return Ok(
            HttpResponse::Ok()
                .content_type(Mime::from_str(&content_type).unwrap())
                .insert_header(("Content-Disposition", ContentDisposition {
                    disposition: DispositionType::Inline,
                    parameters: vec![DispositionParam::Filename(format!("{}.{}", &code, &file_ext))],
                }))
                .body(SizedStream::new(file_md.len(), stream))
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

        state.db.add_file(&code, &hash, &content_type, file_ext).await?;
    }

    Ok(
        HttpResponse::build(StatusCode::OK)
            .body(String::from(format!("{{\"url\": \"{}?v={}\"}}", state.base_url, &code)))
    )
}

#[axw::get("/")]
async fn index() -> Result<axf::NamedFile> {
    Ok(handle_result!(to_str; axf::NamedFile::open("pages/index.html")))
}

#[axw::main]
async fn main() -> std::io::Result<()> {
    dotenv().ok();
    let api_key = Arc::new(std::env::var("API_KEY").unwrap());
    let base_url = Arc::new(std::env::var("BASE_URL").unwrap());
    let port = std::env::var("PORT").unwrap();
    let log_level = std::env::var("LOG_LEVEL").unwrap_or(String::from("info"));
    let workers = std::env::var("WORKERS").unwrap_or(String::from("4"));

    std::env::set_var("RUST_LOG", log_level);
    env_logger::init();

    HttpServer::new(move || {
        App::new()
            .wrap(axw::middleware::Logger::default())
            .app_data(axw::web::Data::new(AppState::new(api_key.clone(), base_url.clone()).expect("Failed to open database")))
            .service(axf::Files::new("/static", "./static/").prefer_utf8(true))
            .service(index)
            .service(status)
            .service(upload)
            .service(query)
    })
        .bind(("127.0.0.1", u16::from_str(&port).expect("Failed to parse 'PORT' env variable")))?
        .workers(usize::from_str(&workers).expect("Failed to parse 'WORKERS' env variable"))
        .run()
        .await
}