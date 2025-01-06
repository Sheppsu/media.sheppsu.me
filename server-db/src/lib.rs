use rusqlite::Connection;
use rusqlite::params;

use std::time::SystemTime;


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
