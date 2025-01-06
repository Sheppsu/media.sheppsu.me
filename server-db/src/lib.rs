use std::future::Future;
use std::task::{Context, Poll, Waker};
use std::pin::Pin;
use std::time::SystemTime;
use std::thread;
use std::sync::{mpsc, mpsc::Receiver, Arc, Mutex};

use rusqlite::Connection;
use rusqlite::params;

macro_rules! handle_result {
    ($result:expr) => {
        $result.map_err(|e| e.to_string())?
    };
}

macro_rules! timestamp {
    () => { SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_secs() };
}


type Result<T> = std::result::Result<T, String>;


fn init_db(conn: &Connection) -> Result<()> {
    handle_result!(conn.execute(
        "CREATE TABLE \"file\" (
            \"code\"	TEXT NOT NULL UNIQUE,
            \"hash\"	TEXT NOT NULL,
            \"views\"	INTEGER NOT NULL DEFAULT 0,
            \"last_viewed\"	INTEGER NOT NULL,
            \"content_type\"	TEXT NOT NULL,
            \"file_ext\" TEXT NOT NULL,
            PRIMARY KEY(\"code\")
        )",
        []
    ));

    Ok(())
}

pub struct Database {
    conn: Connection,
}

impl Database {
    pub fn new(path: &str) -> Result<Self> {
        let conn = handle_result!(Connection::open(path));

        // Check if the db has a table named 'file'
        {
            let mut stmt = handle_result!(conn.prepare(
                "SELECT name FROM sqlite_schema WHERE type = 'table' AND name = 'file'"
            ));
            let mut rows = handle_result!(stmt.query([]));
            if handle_result!(rows.next()).is_none() {
                handle_result!(init_db(&conn));
            }
        }

        Ok(Database { conn })
    }

    pub fn get_file_for(&self, code: &str) -> Result<Option<(String, String, String)>> {
        let mut stmt = handle_result!(self.conn.prepare("SELECT hash, content_type, file_ext FROM file WHERE code = ?1"));
        let mut rows = handle_result!(stmt.query([code]));
        let row = handle_result!(rows.next());
        if let Some(data) = row {
            return Ok(Some((
                handle_result!(data.get(0)),
                handle_result!(data.get(1)),
                handle_result!(data.get(2))
            )));
        }

        Ok(None)
    }

    pub fn update_file_stats(&self, code: &str) -> Result<()> {
        handle_result!(self.conn.execute(
            "UPDATE file SET views = views + 1, last_viewed = ?1 WHERE code = ?2",
            params![timestamp!(), code]
        ));

        Ok(())
    }

    pub fn add_file(&self, code: &str, hash: &str, content_type: &str, file_ext: &str) -> Result<()> {
        handle_result!(self.conn.execute(
            "INSERT INTO file (code, hash, last_viewed, content_type, file_ext) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![code, hash, timestamp!(), content_type, file_ext]
        ));

        Ok(())
    }
}

unsafe impl Send for Database {}

struct DatabaseCommandFuture<T>(Arc<Mutex<DatabaseCommandFutureState<T>>>);
struct DatabaseCommandFutureState<T> {
    result: Option<T>,
    waker: Option<Waker>,
}

impl<T> DatabaseCommandFutureState<T> {
    pub fn new() -> Self {
        DatabaseCommandFutureState { result: None, waker: None }
    }

    pub fn resolve(&mut self, result: T) {
        self.result.replace(result);
        if let Some(waker) = self.waker.take() {
            waker.wake();
        }
    }
}

impl<T> DatabaseCommandFuture<T> {
    pub fn new() -> Self {
        DatabaseCommandFuture(Arc::new(Mutex::new(DatabaseCommandFutureState::new())))
    }
}

impl<T> Future for DatabaseCommandFuture<T> {
    type Output = T;

    fn poll(self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut state = self.0.lock().unwrap();
        match state.result.take() {
            Some(result) => Poll::Ready(result),
            None => {
                state.waker.replace(ctx.waker().clone());
                Poll::Pending
            }
        }
    }
}

// TODO: macro
type GetFileResult = Result<Option<(String, String, String)>>;
type GetFileFuture = DatabaseCommandFuture<GetFileResult>;
type UpdateFileResult = Result<()>;
type UpdateFileFuture = DatabaseCommandFuture<UpdateFileResult>;
type AddFileResult = Result<()>;
type AddFileFuture = DatabaseCommandFuture<AddFileResult>;

type DatabaseCaller = Box<dyn FnOnce(&Database) -> () + Send>;

pub struct DatabaseAsyncWrapper {
    tx: mpsc::Sender<DatabaseCaller>,
}

impl DatabaseAsyncWrapper {
    pub fn new(db: Database) -> Self {
        let (tx, rx) = mpsc::channel();

        thread::spawn(move || Self::__run(db, rx));

        DatabaseAsyncWrapper { tx }
    }

    fn __run(db: Database, rx: Receiver<DatabaseCaller>) {
        loop {
            let func = match rx.recv() {
                Ok(func) => func,
                Err(_) => {
                    // TODO: start the thread again if it should be still running
                    log::info!("DatabaseAsyncWrapper::__run thread is ending due to lost connection");
                    return;
                },
            };
            func(&db);
        }
    }

    pub async fn get_file_for(&self, code: &str) -> Result<Option<(String, String, String)>> {
        let movable_code = String::from(code);

        let mut future = GetFileFuture::new();
        let future = Pin::new(&mut future);
        let state = future.0.clone();

        handle_result!(
            self.tx.send(Box::new(move |db| {
                state.lock().unwrap().resolve(db.get_file_for(&movable_code));
            }))
        );

        future.await
    }

    pub async fn update_file_stats(&self, code: &str) -> UpdateFileResult {
        let movable_code = String::from(code);

        let mut future = UpdateFileFuture::new();
        let future = Pin::new(&mut future);
        let state = future.0.clone();

        handle_result!(
            self.tx.send(Box::new(move |db| {
                state.lock().unwrap().resolve(db.update_file_stats(&movable_code));
            }))
        );

        future.await
    }

    pub async fn add_file(&self, code: &str, hash: &str, content_type: &str, file_ext: &str) -> AddFileResult {
        let movable_code = String::from(code);
        let movable_hash = String::from(hash);
        let movable_content_type = String::from(content_type);
        let movable_file_ext = String::from(file_ext);

        let mut future = AddFileFuture::new();
        let future = Pin::new(&mut future);
        let state = future.0.clone();

        handle_result!(
            self.tx.send(Box::new(move |db| {
                state.lock().unwrap().resolve(db.add_file(&movable_code, &movable_hash, &movable_content_type, &movable_file_ext));
            }))
        );

        future.await
    }
}
