use std::path::PathBuf;

use redb::Database;

use crate::{
    TemplateApp,
    app::{EditTrack, METADATA_TABLE},
};
/// BASIC UTILITIES ///
/// Simple functions used everywhere, mostly just conversions
pub fn show_error(app: &mut TemplateApp, error: String) {
    app.error_value = error;
    app.error_show = true;
}

pub fn to_base62(mut n: usize, width: usize) -> String {
    let charset = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
    let mut result = Vec::new();

    if n == 0 {
        result.push(charset[0]);
    } else {
        while n > 0 {
            result.push(charset[n % 62]);
            n /= 62;
        }
    }

    while result.len() < width {
        result.push(charset[0]);
    }

    result.reverse();
    String::from_utf8(result).unwrap_or_else(|_| "0000".to_string())
}

pub fn path_to_string(path: &PathBuf) -> String {
    let stringpath = path.as_path().to_string_lossy().to_string();
    stringpath
}

pub fn path_to_string_name(path: &PathBuf) -> String {
    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "Unknown".to_string());
    file_name
}

pub fn path_to_uri(path: std::path::PathBuf) -> String {
    let abs_path = path.canonicalize().unwrap_or(path.clone());
    let path_str = abs_path.to_string_lossy().to_string();

    let cleaned_path = path_str // this will probably need to be changed for android. God how the hell do you builkd for Android. Rafgh.
        .replace("\\\\?\\", "")
        .replace("\\", "/");

    let uri = format!("file:///{}", cleaned_path).replace("#", "%23");
    uri
}

pub fn init_metadata_cache_redb(db: &Database) {
    println!("Checking for database init");
    let table_exists = {
        let read_txn = db.begin_read().expect("Failed to begin read txn");
        match read_txn.open_table(METADATA_TABLE) {
            Ok(_) => true,
            Err(redb::TableError::TableDoesNotExist(_)) => false,
            Err(e) => panic!("Unexpected database error: {:?}", e),
        }
    };

    if !table_exists {
        println!("Database not initialized. Initializing now");
        let write_txn = db.begin_write().expect("Failed to begin write txn");
        {
            let _ = write_txn
                .open_table(METADATA_TABLE)
                .expect("Failed to initialize table");
        }
        write_txn.commit().expect("Failed to commit table creation");
    }
}

pub fn get_metadata_from_redb(db: &Database, uid: String) -> Option<EditTrack> {
    let read_txn = db.begin_read().ok()?;
    let table = read_txn.open_table(METADATA_TABLE).ok()?;
    let access = table.get(uid).ok()??; // double ? because table.get returns Result<Option<T>>
    let bytes = access.value();
    postcard::from_bytes(bytes).ok()
}

pub fn remove_illegal_characters(input: &str) -> String {
    input
        .chars()
        .filter(|&c| {
            !matches!(
                c,
                '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '\0'..='\u{001F}'
            )
        })
        .collect()
}

/*fn uri_to_path(uri: &str) -> Result<PathBuf, String> {
    Url::parse(uri)
        .map_err(|e| e.to_string())?
        .to_file_path()
        .map_err(|_| "Invalid URI".into())
}*/
//commented out bc nothing uses it rn
