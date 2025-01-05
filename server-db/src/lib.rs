use rusqlite::Connection;


macro_rules! handle_result {
    ($result:expr) => {
        $result.map_err(|e| e.to_string())?
    };
}


type Result<T> = std::result::Result<T, String>;


fn init_db(conn: &Connection) -> Result<()> {
    handle_result!(conn.execute(
        "CREATE TABLE \"file\" (
            \"code\"	TEXT NOT NULL UNIQUE,
            \"hash\"	TEXT NOT NULL UNIQUE,
            \"views\"	INTEGER NOT NULL DEFAULT 0,
            \"last_viewed\"	INTEGER NOT NULL,
            \"content_type\"	TEXT NOT NULL,
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

    pub fn get_hash_for(&self, code: &str) -> Result<Option<(String, String)>> {
        let mut stmt = handle_result!(self.conn.prepare("SELECT hash, content_type FROM file WHERE code = ?1"));
        let mut rows = handle_result!(stmt.query([code]));
        let row = handle_result!(rows.next());
        if let Some(data) = row {
            return Ok(Some((handle_result!(data.get(0)), handle_result!(data.get(1)))));
        }

        Ok(None)
    }
}
