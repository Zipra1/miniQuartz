use redb::Database;
use std::fs;
use std::fs::File;
use std::io::{BufRead, BufReader, Error, Write};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

use crate::TemplateApp;
use crate::app::{EditTrack, METADATA_TABLE};
use crate::utilities::{
    path_to_string, path_to_string_name, show_error, to_base62,
};

const M3U_HEADER: &'static str = "#EXTM3U";

/// PLAYLIST ///
/// Song management & organization
pub struct Songs {
    pub articles: Vec<SongCardData>,
}

#[derive(Clone, serde::Deserialize, serde::Serialize, PartialEq)] // This is so serde knows wat 2 do. Using serde here to store the last playing song
pub struct SongCardData {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub length_string: String,
    pub cover_path: String,
    pub path: std::path::PathBuf,
    #[serde(skip)]
    pub texture: Option<egui::TextureHandle>,
    pub playing: bool,
    pub metadata_loaded: bool,
    pub display: bool,
}

impl Songs {
    pub fn new(m3u_path: &PathBuf, db: &Database) -> Songs {
        let playlist_entries = match read_m3u(m3u_path) {
            Ok(entries) => entries,
            Err(_) => return Songs { articles: vec![] },
        };
        {
            let write_txn = db.begin_write().expect("Failed to begin write txn");
            let _ = write_txn
                .open_table(METADATA_TABLE)
                .expect("Failed to initialize table");
            write_txn.commit().expect("Failed to commit table creation");
        }
        let read_txn = db.begin_read().expect("Failed to begin read txn");
        let table = read_txn
            .open_table(METADATA_TABLE)
            .expect("Failed to open table");

        let iter = playlist_entries.into_iter().map(|entry| {
            let uid = &entry.path;
            let metadata = table
                .get(uid)
                .ok()
                .flatten()
                .and_then(|val| postcard::from_bytes(val.value()).ok())
                .unwrap_or(EditTrack {
                    playlist_path: "FAILED".to_string(),
                    track_path: "FAILED".to_string(),
                    index: 0,
                    album: "FAILED".to_string(),
                    artist: "FAILED".to_string(),
                    cover: "FAILED".to_string(),
                    title: "FAILED".to_string(),
                    length_string: "FAILED".to_string(),
                });

            SongCardData {
                title: metadata.title,
                artist: metadata.artist,
                album: metadata.album,
                length_string: metadata.length_string,
                cover_path: metadata.cover,
                path: PathBuf::from(&entry.path),
                texture: None,
                playing: false,
                metadata_loaded: false,
                display: true,
            }
        });

        Songs {
            articles: Vec::from_iter(iter),
        }
    }

    pub fn empty() -> Songs {
        Songs {
            articles: Vec::new(),
        }
    }

    pub fn new_from_folder(folder_path: &Path) -> Songs {
        let audio_extensions = ["mp3", "wav", "ogg", "flac", "m4a"];

        let iter = fs::read_dir(folder_path)
            .into_iter() // Handle potential errors reading the folder
            .flatten()
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .filter(|path| {
                path.is_file()
                    && path
                        .extension()
                        .and_then(|ext| ext.to_str())
                        .map(|ext| audio_extensions.contains(&ext.to_lowercase().as_str()))
                        .unwrap_or(false)
            })
            .map(|path| {
                let file_name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| "Unknown Track".to_string()); // unwrap_or_else probably not needed here, every file has a name right?

                SongCardData {
                    title: file_name,
                    artist: "Unknown Artist(2)".to_owned(),
                    album: "Unknown Album".to_owned(),
                    length_string: "--:--".to_owned(),
                    cover_path: "".to_owned(),
                    path: path.clone(),
                    texture: None,
                    playing: false,
                    metadata_loaded: false,
                    display: true,
                }
            });
        Songs {
            articles: Vec::from_iter(iter),
        }
    }
}
impl SongCardData {
    //i must be for real this section is written by ai. im Sorry. but im fuck at rust,, this should be checked later, though.
    pub fn load_texture_if_needed(&mut self, ctx: &egui::Context) {
        if self.texture.is_none() {
            if let Ok(image) = image::open(&self.cover_path) {
                let image = image.to_rgba8();
                let size = [image.width() as usize, image.height() as usize];
                let texture = ctx.load_texture(
                    "ac".to_string(),
                    egui::ColorImage::from_rgba_unmultiplied(size, &image),
                    Default::default(),
                );
                self.texture = Some(texture);
            }
        }
    }
}

pub fn get_playlists(path: &str) -> std::io::Result<Vec<PathBuf>> {
    let entries = fs::read_dir(path)?;
    let playlist_files = entries
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            let path = entry.path();
            path.is_file() && path.extension().and_then(|ext| ext.to_str()) == Some("m3u")
        })
        .map(|entry| entry.path())
        .collect();
    Ok(playlist_files)
}

pub fn print_walkdir() -> Result<(), Box<dyn std::error::Error>> {
    let path = "./playlists";

    if !std::path::Path::new(path).exists() {
        println!("Directory '{}' does not exist!", path);
        return Ok(());
    }

    let walker = WalkDir::new(path).into_iter();
    let mut count = 0;
    for entry in walker {
        println!("{}", entry?.path().display());
        count += 1;
    }
    println!("Total entries: {}", count);
    Ok(())
}

pub fn add_to_playlist(playlist: &mut M3uPlaylist, new_song: &SongCardData) {
    // todo: doesn't need to return error if there is nothing that can have an error here.
    playlist.add_track(&format!("{}", path_to_string(&new_song.path)));
}

pub fn remove_from_playlist(
    playlist: &mut M3uPlaylist,
    index_to_remove: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    if index_to_remove < playlist.entries.len() {
        playlist.entries.remove(index_to_remove);
    } else {
        return Err("Index out of bounds".into());
    }

    Ok(())
}

#[derive(Debug, Clone, PartialEq)]
pub struct PlaylistEntry {
    pub path: String,
    pub extra: Option<Vec<String>>,
}

#[derive(Clone, Default, PartialEq)]
pub struct M3uPlaylist {
    pub title: String,
    pub entries: Vec<PlaylistEntry>,
    pub path: String,
    pub texture: Option<egui::TextureHandle>,
}

impl IntoIterator for M3uPlaylist {
    type Item = PlaylistEntry;
    type IntoIter = std::vec::IntoIter<Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        self.entries.into_iter()
    }
}

impl M3uPlaylist {
    pub fn new() -> Self {
        M3uPlaylist {
            title: String::new(),
            entries: Vec::new(),
            path: String::new(),
            texture: None,
        }
    }

    pub fn add_track(&mut self, path: &str) {
        self.entries.push(PlaylistEntry {
            path: path.to_string(),
            extra: None,
        });
    }
}

pub fn read_m3u<P: AsRef<Path>>(path: P) -> anyhow::Result<M3uPlaylist> {
    let path = path.as_ref();

    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let mut lines = reader.lines();

    let mut playlist = M3uPlaylist::new();

    // Verify that this file is actually an M3U file
    let is_header = lines
        .next()
        .transpose()?
        .map(|header| header == M3U_HEADER)
        .unwrap_or(false);

    if !is_header {
        anyhow::bail!("\"{}\" is not an M3U file.", path.to_string_lossy());
    }
    let mut pending_directives: Vec<String> = Vec::new();
    while let Some(line) = lines.next() {
        let line = line?;
        if line.is_empty() {
            continue;
        }

        if line.starts_with('#') {
            pending_directives.push(line);
        } else {
            playlist.entries.push(PlaylistEntry { 
                path: line, 
                extra: Some(std::mem::take(&mut pending_directives)) // This clears the buffer for the next song
            });
        }
    }

    Ok(playlist)
}

pub fn move_m3u_track(playlist: &mut M3uPlaylist, from: usize, to: usize) -> std::io::Result<()> {
    if playlist.entries.len() <= from {
        eprintln!(
            "move_m3u_track index out of bounds error | from: {} | len: {}",
            from,
            playlist.entries.len()
        );
        return Err(Error::new(
            std::io::ErrorKind::Other,
            format!(
                "move_m3u_track index out of bounds error | from: {} | len: {}",
                from,
                playlist.entries.len()
            ),
        ));
    }
    let insert_at = if from < to { to - 1 } else { to };

    //if from >= playlist.entries.len() || insert_at >= playlist.entries.len(){
    //    return Err(Error::new(std::io::ErrorKind::Other, format!("playlist::move_m3u_track : Index failure. From:{}, To:{}, Len:{}",from,to,playlist.entries.len())));
    //}
    /* im not really sure whats going on that is causing this check to be freaky? can't explain just check it out and try moving songs to/from the very bottom of a playlist.
    i guess it's not really a big deal cus this shouldn't ever trigger.. but: todo: fix this error check */

    let entry = playlist.entries.remove(from); // i wonder if there is a better way of doing this? .remove() has poor performance at huge playlist sizes.
    playlist.entries.insert(insert_at, entry);

    Ok(())
}

pub fn write_m3u<P: AsRef<Path>>(
    path: P,
    playlist: &M3uPlaylist,
    write_header: bool,
    append: bool,
    overwrite: bool,
) -> std::io::Result<()> {
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .append(append)
        .create(true) // if it doesn't exist it'll make it, this is useful for the New Playlist option in the right click menu on song cards
        .open(&path)?;

    if overwrite {
        file = std::fs::File::create(path)?;
    }

    if write_header {
        writeln!(file, "#EXTM3U")?;
    }

    for entry in &playlist.entries {
        if entry.extra.is_some(){
            for directive in entry.extra.clone().unwrap(){
                if let Err(e) = writeln!(file, "{}", directive){
                    println!("Error writing directive to disk: {}", e);
                }
            }
        }
        writeln!(file, "{}", entry.path)?;
    }

    Ok(())
}

pub fn create_empty_m3u<P: AsRef<Path>>(path: P) -> std::io::Result<()> {
    let playlist = M3uPlaylist::new();
    write_m3u(path, &playlist, true, true, true)
}

pub fn get_folders(path: &str) -> std::io::Result<Vec<PathBuf>> {
    let entries = fs::read_dir(path)?; // Read the directory contents
    let folders = entries
        .filter_map(|entry| entry.ok()) // Ignore entries with errors
        .filter(|entry| entry.path().is_dir()) // Keep only directories
        .map(|entry| entry.path()) // Convert DirEntry to PathBuf
        .collect();
    Ok(folders)
}

pub fn reset_playlist_ids(app: &mut TemplateApp) {
    let mut count = 0;
    for mut playlist in app.playlists.clone() {
        let old_path = playlist.clone();
        let selected = &playlist == &app.currently_selected_playlist_path;
        let file_name = path_to_string_name(&playlist);
        let clean_name: String = file_name.chars().skip(4).collect(); // todo: when program more refined, check if you need it like this or if you can just do [4..]
        // ^^ this is done in case a playlist file is ever put into folder that has less than 4 chars. shouldn't happen, but just in case.
        let count62 = to_base62(count, 4); // 14 million playlists gotta be enough.
        playlist.set_file_name(format!("{:04}{}", count62, clean_name));
        app.playlists[count] = playlist.clone(); // this should probably be on a different thread, since a huge amount of playlists will cause a freeze bc disk operations
        if playlist.set_extension("m3utmp") {
            if playlist.file_name()
                != app
                    .currently_selected_playlist_name
                    .as_ref()
                    .map(std::ffi::OsStr::new)
            /*  this check is useless if .file_name returns the extension aswell.
            meant to be a bit of an optimization, so that we do not rename playlists that aren't being rearranged.
            though, i'm not sure if it's working right. i do not think it is, actually! */
            {
                if let Err(error) = fs::rename(&old_path, &playlist) {
                    show_error(
                        app,
                        format!(
                            "err: {} | from: {} | to: {}",
                            error.to_string(),
                            path_to_string(&old_path),
                            path_to_string(&playlist),
                        ),
                    );
                    eprintln!(
                        "reset_playlist_ids: err: {} | from: {} | to: {}",
                        error.to_string(),
                        path_to_string(&old_path),
                        path_to_string(&playlist),
                    );
                }
                if selected {
                    playlist.set_extension("m3u");
                    app.currently_selected_playlist_path = playlist;
                    //show_error(self, "Meow! Selected moved.".to_string());
                }
            }
        } else {
            let err = "reset_playlist_ids set_extension error 1: m3utmp".to_string();
            show_error(app, err.clone());
            eprintln!("{}", err);
        }
        count += 1;
    }
    for mut playlist in app.playlists.clone() {
        let mut old_path = playlist.clone();
        old_path.set_extension("m3utmp");
        if !&playlist.set_extension("m3u") {
            let err = "reset_playlist_ids set_extension error 2: m3u".to_string();
            show_error(app, err.clone());
            eprintln!("{}", err);
        }
        if let Err(error) = fs::rename(&old_path, &playlist) {
            show_error(
                app,
                format!(
                    "err: {} | from: {} | to: {}",
                    error.to_string(),
                    path_to_string(&old_path),
                    path_to_string(&playlist),
                ),
            );
        }
    }
}
