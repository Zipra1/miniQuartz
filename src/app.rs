//use std::collections::{binary_heap::{IntoIter, Iter}, hash_map::Iter};
use anyhow;
use egui::{Id, Modal, ScrollArea};
use gstreamer::prelude::*; // $env:PKG_CONFIG_PATH="C:\Program Files\gstreamer\1.0\msvc_x86_64\lib\pkgconfig"
use gstreamer::tags;
use image::imageops::FilterType;
use redb::{Database, TableDefinition};
use rustc_hash::FxHasher;
use serde::Deserialize;
use serde::Serialize;
use std::collections::HashMap;
use std::collections::HashSet;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender};
use std::time::Duration;

use crate::playback::*;
use crate::playlist::*;
use crate::song_ui::*;
use crate::utilities::*;

const _PLAYLIST_PAGE: usize = 0;
const _SETTINGS_PAGE: usize = 1;
// Later on, these constants should be used as a way to switch what's displayed in the central panel.
/// We derive Deserialize/Serialize so we can persist app state on shutdown.
#[derive(serde::Deserialize, serde::Serialize)]
#[serde(default)] // if we add new fields, give them default values when deserializing old state
pub struct TemplateApp {
    // probably not everything needs to be pub, but is there a reason not to?

    // TODO: A lot of these things can be grouped into structs!!!
    #[serde(skip)]
    pub songs: Songs,

    #[serde(skip)]
    pub queue: Songs,

    #[serde(skip)]
    pub row_height: Option<f32>,

    #[serde(skip)]
    pub col1_width: Option<f32>,

    pub col2_width: Option<f32>,

    #[serde(skip)]
    pub playbin: gstreamer::Element,

    #[serde(skip)]
    pub position_ms: u64,

    #[serde(skip)]
    pub duration_ms: u64,

    #[serde(skip)]
    pub last_update: std::time::Instant,

    #[serde(skip)]
    pub error_show: bool,

    #[serde(skip)]
    pub error_value: String,

    pub volume: f32,

    #[serde(skip)]
    pub folders: Vec<PathBuf>,

    #[serde(skip)]
    pub playlists: Vec<PathBuf>,

    pub currently_selected_playlist_name: Option<String>,
    pub currently_selected_playlist_path: PathBuf,

    pub now_playing: Option<PathBuf>,

    pub now_playing_song: Option<SongCardData>,

    #[serde(skip)]
    pub selected_songs: Vec<usize>,
    #[serde(skip)]
    pub selected_songs_origin: usize,

    #[serde(skip)]
    pub metadata_receiver: Receiver<MetadataResult>,
    #[serde(skip)]
    pub metadata_sender: Sender<MetadataRequest>,

    pub title_header_width: f32,
    pub total_header_width: f32,

    pub dragged_song_index: Option<usize>,
    pub swap_request: Option<(usize, usize)>,
    pub drag_origin: Option<egui::Pos2>,
    pub test_thing: Option<f32>,
    pub dragging_song: Option<usize>,

    #[serde(skip)]
    pub warning_show: bool,
    #[serde(skip)]
    pub playlist_to_delete: Option<PathBuf>,

    #[serde(skip)]
    pub rename_playlist_show: bool,
    #[serde(skip)]
    pub playlist_to_rename: Option<PathBuf>,
    pub rename_to: String,
    #[serde(skip)]
    pub m3u_sender: Sender<M3uEditTask>,

    #[serde(skip)]
    pub thread_info: ThreadInfo,

    //popup
    pub align4: egui::RectAlign,
    pub gap: f32,
    #[serde(skip)]
    pub close_behavior: egui::PopupCloseBehavior,
    pub popup_open: bool,
    pub checked: bool,
    pub color: egui::Color32,

    #[serde(skip)]
    pub loaded_paths: HashSet<PathBuf>,

    #[serde(skip)]
    pub redb_metadata_cache: Arc<Database>,
}

pub struct ThreadInfo {
    pub time_since_task_added: std::time::Instant,
    pub queue_size: usize,
}

pub const METADATA_TABLE: TableDefinition<String, &[u8]> = TableDefinition::new("metadata_table");

impl Default for TemplateApp {
    fn default() -> Self {
        gstreamer::init().expect("Failed to init GStreamer"); // todo: expect should be an error message

        let (result_tx, metadata_receiver) = std::sync::mpsc::channel::<MetadataResult>();
        let (metadata_sender, req_rx) = std::sync::mpsc::channel::<MetadataRequest>();

        std::thread::spawn(move || {
            // [[metadata-thread]]
            // add things to this using load_metadata_if_needed: [[@load-metadata-fn]]
            // metadata is retrieved from metadata-receiver at [[@processing-metadata]]

            let discoverer =
                gstreamer_pbutils::Discoverer::new(gstreamer::ClockTime::from_seconds(5))
                    .expect("Failed to create discoverer"); // todo: proper error

            while let Ok(request) = req_rx.recv() {
                println!(
                    "Received metadata request: {}",
                    path_to_string(&request.path)
                );
                if let Ok(metadata) = get_metadata(&discoverer, request.path.clone()) {
                    if let Err(e) = result_tx.send(MetadataResult {
                        path: request.path,
                        data: metadata,
                    }) {
                        println!("Metadata Request Error: {}", e);
                    }
                }
            }
        });

        let (sender_m3u, receiver_m3u) = std::sync::mpsc::channel::<M3uEditTask>();

        let db = Database::create("cache/metadata.redb").expect("Failed to create redb database");
        let shared_db = Arc::new(db);
        let db_for_worker = shared_db.clone();

        std::thread::spawn(move || {
            let mut time_since_task_added = std::time::Instant::now();
            let mut need_write = false;
            let mut urgent = false;
            let mut pending_updates: HashMap<std::path::PathBuf, M3uPlaylist> = HashMap::new();

            loop {
                match receiver_m3u.recv_timeout(Duration::from_millis(300)) {
                    Ok(task) => {
                        time_since_task_added = std::time::Instant::now();
                        need_write = false;
                        urgent = false;
                        let path = match &task {
                            M3uEditTask::Edit(data) => data.playlist_path.clone(),
                            M3uEditTask::Add(data) => data.file_path.clone(),
                            M3uEditTask::Remove(data) => data.file_path.clone(),
                            M3uEditTask::Move(data) => data.file_path.clone(),
                            M3uEditTask::RemovePlaylist(_data) => "NOREAD".to_string(),
                        };
                        let playlist = pending_updates.entry(PathBuf::from(path.clone())).or_insert_with(|| {
                            /* note cus this is kinda weird to read, this is setting playlist to the read M3uPlaylist. If the M3uPlaylist hasn't
                            been read yet, then it reads it and adds it to pending_updates. That way it can write all changes to a playlist at once */
                            println!("Loading into cache: {:?}", path);
                            read_m3u(&path).unwrap_or_else(|_| M3uPlaylist {
                                title: String::new(),
                                entries: vec![PlaylistEntry{path:"playlists/playlist-1/color bars.mp3".to_string(), extra: None}],
                                path: path,
                                texture: None,
                            })
                        });
                        match task {
                            M3uEditTask::Edit(data) => {
                                let uid = data.track_path.clone();
                                //println!("{} = {}",uid,path_to_uri(PathBuf::from(data.track_path.clone())));

                                if let Ok(encoded_bytes) = postcard::to_allocvec(&data) {
                                    if let Ok(write_txn) = db_for_worker.begin_write() {
                                        if let Ok(mut table) = write_txn.open_table(METADATA_TABLE)
                                        {
                                            let _ = table.insert(uid.clone(), encoded_bytes.as_slice());
                                        }
                                        let _ = write_txn.commit();
                                    }
                                }
                                // Database writes are done one at a time instead of batched and written all at once. This should be changed.
                                // println!(
                                //     "Retrieved value: {}",
                                //     get_metadata_from_redb(&db_for_worker, uid)
                                //         .unwrap_or(EditTrack {
                                //             playlist_path: "FAILED2".to_string(),
                                //             track_path: "FAILED2".to_string(),
                                //             index: 0,
                                //             album: "FAILED2".to_string(),
                                //             artist: "FAILED2".to_string(),
                                //             cover: "FAILED2".to_string(),
                                //             title: "FAILED2".to_string(),
                                //             length_string: "FAILED2".to_string()
                                //         })
                                //         .title
                                // );
                            }
                            M3uEditTask::Add(data) => {
                                println!("{}", "Queued: Adding m3u track");
                                add_to_playlist(playlist, &data.new_song);
                                time_since_task_added = std::time::Instant::now();
                                need_write = true;
                                urgent = true;
                            }
                            M3uEditTask::Remove(data) => {
                                println!("{}", "Queued: Removing m3u track");
                                if let Err(e) = remove_from_playlist(playlist, data.index_to_remove)
                                {
                                    eprintln!("Error removing m3u track: {}", e);
                                } else {
                                    time_since_task_added = std::time::Instant::now();
                                    need_write = true;
                                    urgent = true;
                                }
                            }
                            M3uEditTask::Move(data) => {
                                println!("{}", "Queued: Moving m3u track");
                                if let Err(e) = move_m3u_track(playlist, data.from, data.to) {
                                    eprintln!("Error moving m3u track: {}", e);
                                } else {
                                    time_since_task_added = std::time::Instant::now();
                                    need_write = true;
                                    urgent = true;
                                }
                            }
                            M3uEditTask::RemovePlaylist(data) => {
                                // auummm
                            }
                        }
                    }
                    Err(_) => {
                        // timeout. the check to write to m3u might belong here
                    }
                }

                let write_delay_time = 1000 - (urgent as u64 * 950);
                if time_since_task_added.elapsed()
                    >= std::time::Duration::from_millis(write_delay_time)
                    && need_write
                {
                    need_write = false;
                    urgent = false;
                    for (path, playlist) in pending_updates.drain() {
                        // todo: path should be part of M3uPlaylist
                        if let Err(e) = write_m3u(path, &playlist, true, false, true) {
                            eprintln!("M3uEditTasks: Error writing m3u: {}", e);
                        } else {
                            println!("M3uEditTasks: Successfully wrote m3u");
                        }
                    }
                }
            }
        });

        let pb = gstreamer::ElementFactory::make("playbin")
            .build()
            .expect("Could not create playbin");

        Self {
            songs: Songs::new(&std::path::PathBuf::from("./playlists/"), &shared_db),
            queue: Songs::new(&std::path::PathBuf::from("queue.m3u"), &shared_db),
            row_height: None,
            col1_width: None,
            col2_width: None,
            playbin: pb,
            position_ms: 0,
            duration_ms: 0,
            last_update: std::time::Instant::now(),

            error_show: false,
            error_value: "No error message".to_owned(),

            volume: 1.0,

            folders: get_folders("./playlists/").unwrap_or_default(),
            playlists: get_playlists("./playlists/").unwrap_or_default(),

            currently_selected_playlist_name: None,
            currently_selected_playlist_path: std::path::PathBuf::from(""),

            now_playing: None,

            now_playing_song: Some(SongCardData {
                title: "".to_owned(),
                artist: "none".to_owned(),
                length_string: "".to_owned(), // todo: parse
                album: "none".to_owned(),
                cover_path: "".to_owned(),
                path: std::path::PathBuf::from(""),
                texture: None,
                playing: false,
                metadata_loaded: false,
                display: true,
            }),

            selected_songs: vec![],
            selected_songs_origin: 0,

            metadata_receiver,
            metadata_sender,

            title_header_width: 250.0,
            total_header_width: 0.0,

            dragged_song_index: None,
            swap_request: None,
            drag_origin: None,
            test_thing: None,
            dragging_song: None,

            warning_show: false,
            playlist_to_delete: None,

            rename_playlist_show: false,
            playlist_to_rename: None,
            rename_to: "Playlist Name".to_string(),

            m3u_sender: sender_m3u, // I love shitty naming schemes (>w< )↗

            thread_info: ThreadInfo {
                time_since_task_added: std::time::Instant::now(),
                queue_size: 0,
            },

            //popup demo
            align4: egui::RectAlign::default(),
            gap: 4.0,
            close_behavior: egui::PopupCloseBehavior::CloseOnClick,
            popup_open: false,
            checked: true,
            color: egui::Color32::RED,

            loaded_paths: HashSet::new(),

            redb_metadata_cache: shared_db,
        }
    }
}

impl TemplateApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        cc.egui_ctx
            .style_mut(|s| s.interaction.selectable_labels = false);
        let mut app: Self = if let Some(storage) = cc.storage {
            eframe::get_value(storage, eframe::APP_KEY).unwrap_or_default()
        } else {
            Default::default()
        };

        initialize_gstreamer(&mut app);

        if app.currently_selected_playlist_path.exists() {
            // Check bc user may have deleted folder
            app.songs = Songs::new(
                &app.currently_selected_playlist_path,
                &app.redb_metadata_cache,
            );
        } else {
            app.currently_selected_playlist_name = Some("Playlist not found".to_owned());
        }

        app
    }

    pub fn apply_options<'a>(&self, popup: egui::Popup<'a>) -> egui::Popup<'a> {
        popup
            .align(self.align4)
            .gap(self.gap)
            .close_behavior(self.close_behavior)
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct DefaultPlaylistData {
    song_locations: Vec<String>,
}

pub struct Metadata {
    title: String,
    artist: String,
    album: String,
    cover_path: String,
    length_string: String,
}

#[derive(Serialize, Deserialize)]
pub struct EditTrack {
    pub playlist_path: String,
    pub track_path: String,
    pub index: usize,
    pub album: String,
    pub artist: String,
    pub cover: String,
    pub title: String,
    pub length_string: String,
}

pub struct AddTrack {
    pub file_path: String,
    pub new_song: SongCardData,
}

pub struct RemoveTrack {
    pub file_path: String,
    pub index_to_remove: usize,
}

pub struct MoveTrack {
    file_path: String,
    from: usize,
    to: usize,
}

pub struct RemovePlaylist {
    pub file_path: Option<PathBuf>,
}

pub enum M3uEditTask {
    Edit(EditTrack),
    Add(AddTrack),
    Remove(RemoveTrack),
    Move(MoveTrack),
    RemovePlaylist(RemovePlaylist),
}

// scared to move the multithreaded stuff to another file (～￣▽￣)～ but metadata stuff Should go somewhere else.
pub fn get_metadata(
    discoverer: &gstreamer_pbutils::Discoverer,
    path: std::path::PathBuf,
) -> Result<Metadata, anyhow::Error> {
    let uri = path_to_uri(path);
    let info = discoverer.discover_uri(&uri)?;

    let tags = info.tags();

    let title = tags
        .as_ref()
        .and_then(|t| t.get::<tags::Title>())
        .map(|t| t.get().to_string())
        .unwrap_or_else(|| "".to_string());

    let artist = tags
        .as_ref()
        .and_then(|t| t.get::<tags::Artist>())
        .map(|a| a.get().to_string())
        .unwrap_or_else(|| "Unknown Artist".to_string());

    let album = tags
        .as_ref()
        .and_then(|t| t.get::<tags::Album>())
        .map(|al| al.get().to_string())
        .unwrap_or_else(|| "Unknown Album".to_string());

    let cover_data = tags
        .as_ref()
        .and_then(|t| {
            t.get::<tags::Image>()
                .or_else(|| t.get::<tags::Attachment>())
        })
        .and_then(|sample_obj| {
            let sample = sample_obj.get();
            let buffer = sample.buffer()?;
            let map = buffer.map_readable().ok()?;
            Some(map.as_slice().to_vec())
        });

    let length = info.duration();
    //let length_usize = length.unwrap().mseconds() as usize;
    let length_secs = length.unwrap().seconds();
    let minutes = length_secs / 60;
    let seconds = length_secs % 60;
    let length_string = format!("{:02}:{:02}", minutes, seconds);

    let mut hasher = FxHasher::default();
    if album != "Unknown Album" && artist != "Unknown Artist" {
        format!("{}{}", album, artist).hash(&mut hasher);
        // if just the file path is used, then the same cover would be cached multiple times
    } else {
        uri.hash(&mut hasher);
    }

    let unique_id = hasher.finish();
    let output_path_str = format!("cache/cover_{}.jpg", unique_id);
    let output_path = PathBuf::from(output_path_str.clone());

    if let Some(parent) = output_path.parent() {
        // checking if cache folder exists
        std::fs::create_dir_all(parent).unwrap_or_default();
    }

    if let Some(data) = cover_data {
        if !output_path.exists() {
            match image::load_from_memory(&data) {
                Ok(img) => {
                    let resized = img.resize(96, 96, FilterType::Lanczos3);
                    if let Err(_) = resized.save(output_path) {
                        return Ok(Metadata {
                            title: "⚠ Album cover save error".to_owned(),
                            artist: " ".to_owned(),
                            album,
                            cover_path: "assets/icon-256.png".to_owned(),
                            length_string,
                        });
                    }
                }
                Err(e) => eprintln!("error: {}", e),
            }
        }
        return Ok(Metadata {
            title,
            artist,
            album,
            cover_path: output_path_str,
            length_string,
        });
    }
    //std::thread::sleep(std::time::Duration::from_millis(10));
    Ok(Metadata {
        title,
        artist,
        album,
        cover_path: "assets/icon-256.png".to_owned(),
        length_string,
    })
}

pub struct MetadataRequest {
    path: std::path::PathBuf,
}

pub struct MetadataResult {
    path: std::path::PathBuf,
    data: Metadata,
}

pub fn load_metadata_if_needed(
    song: &mut SongCardData,
    sender: std::sync::mpsc::Sender<MetadataRequest>,
) {
    if !song.metadata_loaded {
        if let Err(e) = sender.send(MetadataRequest {
            // metadata requests processed at [[@metadata-thread]] [[load-metadata-fn]]
            path: song.path.clone(),
        }) {
            println!("load_metadata_if_needed Metadata Request error: {}", e);
        }
        println!("{}", "Sent metadata request");
        song.metadata_loaded = true;
    }
}

//（︶^︶）（︶^︶）//
//    UI STUFF    //

impl eframe::App for TemplateApp {
    /// Called by the framework to save state before shutdown.
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, eframe::APP_KEY, self);
    } //todo: deciper how the example app does this stuff; how do you add something to be saved on reboot?

    /// Called each time the UI needs repainting, which may be many times per second.
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Inside update(&mut self, ctx, frame)
        let bus = self.playbin.bus().unwrap();
        for msg in bus.iter_timed(gstreamer::ClockTime::ZERO) {
            match msg.view() {
                gstreamer::MessageView::Eos(..) => {
                    if let Err(e) = self.playbin.set_state(gstreamer::State::Ready) {
                        println!("GStreamer Error update() set_state: {}", e);
                    }
                    self.now_playing = None; // todo: set this to first song playlist at end of queue
                }
                gstreamer::MessageView::Error(err) => {
                    show_error(
                        self,
                        format!("GStreamer Error update() Error: {}", err.error().to_owned()),
                    );
                    println!("GStreamer Error: {}", err.error().to_owned());
                }
                _ => {}
            }
        }

        // Put your widgets into a `SidePanel`, `TopBottomPanel`, `CentralPanel`, `Window` or `Area`.
        // For inspiration and more examples, go to https://emilk.github.io/egui
        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            // The top panel is often a good place for a menu bar:

            egui::MenuBar::new().ui(ui, |ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::TOP), |ui| {
                    ui.menu_button("File", |ui| {
                        if ui.button("Quit").clicked() {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                        if (ui.button("Meoooww")).clicked() {
                            show_error(self, "Meow Button Pressed".to_owned());
                            if let Err(e) = print_walkdir() {
                                eprintln!("Walkdir error: {}", e);
                            }
                            println!("{}", "Meow Button Pressed".to_string());
                        }
                    });
                    ui.add_space(16.0);
                });

                egui::widgets::global_theme_preference_buttons(ui); // where ddid they go...
            });
        });

        //--\(￣︶￣*\))---\(￣︶￣*\))---\(￣︶￣*\))---\(￣︶￣*\))--//
        //    Bottom bar to display track info and track controls    //

        egui::TopBottomPanel::bottom("status")
            .resizable(true)
            .min_height(50.0)
            .show(ctx, |ui| {
                ui.set_min_height(ui.available_height());
                ui.horizontal(|ui| {
                    ui.group(|ui| {
                        // song info
                        ui.set_width(150.0);
                        ui.horizontal(|ui| {
                            if let Some(tex) = &self.now_playing_song.as_ref().unwrap().texture {
                                ui.add(
                                    egui::Image::new(&*tex)
                                        .max_height(ui.available_height())
                                        .corner_radius(3), // todo: get this directly from metadata! this is using the small cover from cache
                                );
                            }
                            ui.vertical(|ui| {
                                ui.label(self.now_playing_song.as_ref().unwrap().title.clone());
                                ui.label(self.now_playing_song.as_ref().unwrap().artist.clone());
                            });
                        });
                    });

                    ui.add_space(ui.available_width() / 2.0 - 192.0); // there is probably a better way to center things, since this requires some arbitrary numbers.

                    ui.group(|ui| {
                        // main controls
                        /////
                        // Play/pause button
                        ui.set_width(200.0);
                        ui.vertical_centered(|ui| {
                            if ui.button("Play/Pause").clicked() {
                                let (_success, current, _pending) =
                                    self.playbin.state(gstreamer::ClockTime::NONE);
                                if current == gstreamer::State::Playing {
                                    if let Err(err) =
                                        self.playbin.set_state(gstreamer::State::Paused)
                                    {
                                        show_error(self, err.to_string());
                                        eprintln!(
                                            "GStreamer StateChangeError @ Play/Pause buttons: {}",
                                            err.to_string()
                                        );
                                    }
                                } else if current == gstreamer::State::Paused {
                                    if let Err(err) =
                                        self.playbin.set_state(gstreamer::State::Playing)
                                    {
                                        show_error(self, err.to_string());
                                        eprintln!(
                                            "GStreamer StateChangeError @ Play/Pause buttons: {}",
                                            err.to_string()
                                        );
                                    }
                                } else {
                                    let song_path =
                                        self.now_playing_song.as_ref().map(|s| s.path.clone());
                                    if let Some(path) = song_path {
                                        play_song(self, path);
                                    } else {
                                        show_error(self, "No song ready to play".to_owned()); // This should be removed in the future. Expected behaviour would be to disable the play/pause button.
                                    }
                                }
                            }

                            /////
                            // Seeking
                            // TODO: Make a seek_to() function in playback.rs
                            let (success, state, _pending) =
                                self.playbin.state(gstreamer::ClockTime::from_mseconds(0));
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    let current_duration = format!(
                                        "{}",
                                        self.now_playing_song
                                            .clone()
                                            .unwrap_or(SongCardData {
                                                title: "No song playing".to_string(),
                                                artist: "".to_string(),
                                                album: "".to_string(),
                                                length_string: "--:--".to_string(),
                                                cover_path: "".to_string(),
                                                path: PathBuf::from(""),
                                                texture: None,
                                                playing: false,
                                                metadata_loaded: true,
                                                display: true
                                            })
                                            .length_string
                                    );
                                    if success.is_ok()
                                        && (state == gstreamer::State::Playing
                                            || state == gstreamer::State::Paused)
                                    {
                                        let duration = self.duration_ms.max(1) as f32;
                                        let mut pos = self.position_ms as f32;
                                        ui.label(current_duration);
                                        let response = ui.add(
                                            egui::Slider::new(&mut pos, 0.0..=duration)
                                                .trailing_fill(true)
                                                .custom_formatter(|n, _| {
                                                    let total_seconds = (n / 1000.0) as i64;
                                                    let minutes = total_seconds / 60;
                                                    let seconds = total_seconds % 60;
                                                    format!("{:02}:{:02}", minutes, seconds)
                                                }),
                                        );
                                        if response.changed() {
                                            let seek_to =
                                                gstreamer::ClockTime::from_mseconds(pos as u64);

                                            self.playbin
                                                .seek_simple(
                                                    gstreamer::SeekFlags::FLUSH
                                                        | gstreamer::SeekFlags::KEY_UNIT,
                                                    seek_to,
                                                ) // Wow! Gstream just has that!
                                                .expect("Seek failed");
                                        }
                                    } else {
                                        ui.label(current_duration);
                                        ui.add_enabled(
                                            false,
                                            egui::Slider::new(&mut 0.0, 0.0..=1.0),
                                        );
                                    }
                                    if self.last_update.elapsed().as_millis() > 300 {
                                        // Set position
                                        if let Some(pos) =
                                            self.playbin.query_position::<gstreamer::ClockTime>()
                                        {
                                            self.position_ms = pos.mseconds();
                                        }
                                        self.last_update = std::time::Instant::now();
                                    }
                                },
                            );
                        });
                    });

                    /////
                    // Volume slider
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.group(|ui| {
                            // secondary controls
                            ui.set_width(150.0);

                            let response_volume = ui.add(
                                egui::Slider::new(&mut self.volume, 0.0..=1.0)
                                    .text("🔊") // egui speaker emoji looks kind of bad so replace this with something else eventually
                                    .trailing_fill(true) // 1.0 here is the max volume
                                    .show_value(false),
                            );
                            if response_volume.changed() {
                                let cubic_volume = (self.volume * self.volume * self.volume) as f64; // cubic slider & gstreamer needs f64
                                self.playbin.set_property("volume", cubic_volume);
                            }
                        });
                    });
                });
                if self.warning_show {
                    // todo: modal struct so that you don't have a million variables for every modal
                    delete_playlist_warning(self, ui);
                }
                if self.rename_playlist_show {
                    rename_playlist(self, ui);
                }
                if self.error_show {
                    Modal::new(Id::new("IO Error")).show(ui.ctx(), |ui| {
                        ui.set_width(200.0);
                        ui.heading("Error");
                        ui.label(&self.error_value);

                        ui.add_space(32.0);

                        egui::Sides::new().show(
                            ui,
                            |_ui| {},
                            |ui| {
                                if ui.button("aw dang").clicked() {
                                    self.error_show = false;
                                }

                                if ui.button("im sorry").clicked() {
                                    self.error_show = false;
                                }
                            },
                        );
                    });
                }
            });

        //--(*￣3￣)╭----(*￣3￣)╭---(*￣3￣)╭----(*￣3￣)╭--//
        // Side panel to display playlists and app controls //
        let song_card_jump_trigger_id = egui::Id::new("song_card_jump_trigger"); // used to scroll to top when switching playlists
        egui::SidePanel::left("playlists")
            .resizable(true)
            .min_width(30.0)
            .show(ctx, |ui| {
                ui.heading("miniQuartz");
                let fps = 1.0 / ctx.input(|i| i.stable_dt.max(0.0001)); // fps counter for extra awesome
                ui.label(format!("FPS: {:.1}", fps));
                ScrollArea::vertical().show(ui, |ui| {
                    ui.set_min_width(ui.available_width()); // this makes smooth resizing possible. feels kinda jank but whatever.
                    let list_id = ui.make_persistent_id("reorder_list_bars");
                    let dragging_index = ui.data(|d| d.get_temp::<usize>(list_id));
                    let mut drop_target_index = None;

                    if ui.selectable_label(false, "📁 Local Files").clicked() {
                        let local_path = std::path::PathBuf::from("playlists/playlist-1"); // todo: make user configurable
                        self.songs = Songs::new_from_folder(&local_path);
                        self.currently_selected_playlist_name = Some("Local Files".to_string());
                        self.currently_selected_playlist_path = local_path;
                        ui.data_mut(|d| d.insert_temp(song_card_jump_trigger_id, true));
                    }
                    for i in 0..self.playlists.len() {
                        let playlist_name = self.playlists[i]
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_else(|| "Unknown".to_string()); // unwrap_or_else might be unnecessary here, since a playlist should *never* not have a name; if it didn't, it wouldn't exist.
                        let playlist_name = &playlist_name[4..];
                        ui.with_layout(egui::Layout::top_down_justified(egui::Align::LEFT), |ui| {
                            let response = ui
                                .selectable_label(false, format!("📃  {}", &playlist_name))
                                .interact(egui::Sense::drag());
                            if response.drag_started() {
                                ui.data_mut(|d| d.insert_temp(list_id, i));
                            }

                            let menu_id =
                                ui.make_persistent_id(format!("context_menu_playlist{}", i));
                            self.apply_options(egui::Popup::context_menu(&response).id(menu_id))
                                .show(|ui| right_click_playlist(self, ui, i));

                            if response.dragged() {
                                ui.ctx().set_cursor_icon(egui::CursorIcon::Move);
                                egui::Tooltip::always_open(
                                    ui.ctx().clone(), // is clone right here? feels wrong dunno why
                                    ui.layer_id(),
                                    egui::Id::new("playlist_drag_tooltip"),
                                    egui::Pos2::ZERO,
                                )
                                .at_pointer()
                                .show(|ui| {
                                    ui.label(playlist_name);
                                });
                            }

                            if response.clicked() {
                                self.songs =
                                    Songs::new(&self.playlists[i], &self.redb_metadata_cache);
                                self.currently_selected_playlist_name =
                                    Some(playlist_name.to_string());
                                self.currently_selected_playlist_path =
                                    self.playlists[i].to_path_buf();
                                ui.data_mut(|d| d.insert_temp(song_card_jump_trigger_id, true));
                            }

                            let rect = response.rect;
                            if let (Some(pos), Some(_)) =
                                (ui.input(|i| i.pointer.interact_pos()), dragging_index)
                            {
                                if rect.contains(pos) {
                                    drop_target_index =
                                        Some(if pos.y < rect.center().y { i } else { i + 1 });
                                }
                            }

                            if let Some(target) = drop_target_index {
                                // drop indicator
                                if target == i {
                                    draw_drop_bar(ui, rect.left_top(), rect.right_top());
                                } else if target == self.playlists.len()
                                    && i == self.playlists.len() - 1
                                {
                                    draw_drop_bar(ui, rect.left_bottom(), rect.right_bottom());
                                }
                            }
                        });
                    }

                    if ui.selectable_label(false, "+").clicked() {
                        let count = to_base62(self.playlists.len() + 1, 4);
                        if let Err(error) =
                            create_empty_m3u(&format!("./playlists/{}new playlist.m3u", count))
                        {
                            show_error(self, error.to_string());
                            eprintln!("create_empty_m3u error: {}", error.to_string());
                        }
                        self.playlists = get_playlists("./playlists/").unwrap_or_default();
                    }
                    ui.label(format!(
                        "{}",
                        self.drag_origin.unwrap_or(egui::Pos2::new(0.0, 0.0))
                    ));
                    ui.label(format!("{}", self.test_thing.unwrap_or(0.0)));
                    ui.label(format!(
                        "{}",
                        path_to_string(&self.currently_selected_playlist_path)
                    ));

                    if ui.input(|i| i.pointer.any_released()) {
                        self.dragged_song_index = None;
                        if let (Some(from), Some(to)) = (dragging_index, drop_target_index) {
                            if from != to && to <= self.playlists.len() {
                                let item = self.playlists.remove(from);
                                let mut insert_at = if to > from { to - 1 } else { to };
                                if to > self.playlists.len() {
                                    // is this necessary? i think so.
                                    //show_error(self, "meoww".to_string());
                                    insert_at = to - 1;
                                }
                                self.playlists.insert(insert_at, item);

                                reset_playlist_ids(self);
                            }
                        }
                        ui.data_mut(|d| d.remove::<usize>(list_id));
                    }
                });
            });

        //--◑﹏◐---◑﹏◐---◑﹏◐---◑﹏◐---◑﹏◐---◑﹏◐---◑﹏◐---◑﹏◐---◑﹏◐---◑﹏◐-//
        //   Central panel to display: Playlist contents, album contents, artist pages  //

        egui::CentralPanel::default().show(ctx, |_ui| {
            // central panel has to be rendered after other panels
            egui::TopBottomPanel::top("Header")
                .resizable(false)
                .min_height(50.0)
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        let playlist_name = self
                            .currently_selected_playlist_name
                            .as_deref()
                            .unwrap_or("No playlist selected");
                        /*if self.currently_selected_playlist_name_texture.is_none() {
                            if let Ok(image) = image::open(&self.cover_path) {
                                let image = image.to_rgba8();
                                let size = [image.width() as usize, image.height() as usize];
                                let texture = ctx.load_texture(
                                    self.cover_path.clone(),
                                    egui::ColorImage::from_rgba_unmultiplied(size, &image),
                                    Default::default(),
                                );
                                self.texture = Some(texture);
                            }
                        }
                        if let Some(tex) = self.currently_selected_playlist_name {
                            ui.add(
                                egui::Image::new(tex).max_width(30.0).corner_radius(3), // todo: this should be user configurable. some people haaate corner radius on album art
                            );
                        }*/
                        ui.label(egui::RichText::new(playlist_name).size(32.0).strong());
                    });
                });
            egui::TopBottomPanel::top("Resizables")
                .resizable(false)
                //.min_height(50.0)
                .show(ctx, |ui| {
                    self.total_header_width = ui.available_width();
                    ui.horizontal(|ui| {
                        ui.label("#");
                        ui.separator();
                        ui.spacing_mut().item_spacing.x = 0.0;
                        egui::Resize::default()
                            .id_salt("Title")
                            .default_width(100.0)
                            .max_width(self.total_header_width - 110.0)
                            .min_height(ui.available_height())
                            .max_height(ui.available_height())
                            .with_stroke(false)
                            .show(ui, |ui| {
                                ui.label("Title");
                                self.title_header_width = ui.available_width();
                            });

                        ui.separator();

                        ui.label("Album");

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.add_space(17.0);
                            ui.label("🕒");
                            ui.separator();
                        });
                    });
                });
            egui::CentralPanel::default().show(ctx, |ui| {
                // Nested panels don't usually behave well but it works so idk lol.

                // jump to top when switching playlist stuff
                let should_jump = ui.data_mut(|d| {
                    d.remove_temp::<bool>(song_card_jump_trigger_id)
                        .unwrap_or(false)
                });
                let mut scroll_area = ScrollArea::vertical().scroll_source(
                    egui::containers::scroll_area::ScrollSource {
                        scroll_bar: true,
                        drag: false,
                        mouse_wheel: true,
                    },
                );

                if should_jump {
                    scroll_area = scroll_area.vertical_scroll_offset(0.0);
                }

                scroll_area.show(ui, |ui| {
                    // render buffer stuff
                    let row_height = self.row_height.unwrap_or(30.0); // proper row height: it feels wrong to be setting this every frame.
                    let total_rows = self.songs.articles.len(); // it feels wrong to be setting this every frame. this only really needs to be set if the shown list changes.

                    let clip_rect = ui.clip_rect();
                    let top = clip_rect.top();
                    let bottom = clip_rect.bottom();

                    let mut start = ((top - ui.min_rect().top()) / row_height).floor() as usize;
                    let mut end = ((bottom - ui.min_rect().top()) / row_height).ceil() as usize;

                    let render_buffer_size = 6;
                    // If slow-med scrolling causes metadata not to load, increase this.

                    start = start.saturating_sub(render_buffer_size);
                    end = (end + render_buffer_size).min(total_rows);

                    let above_px = start as f32 * row_height;
                    ui.add_space(above_px); // makes scroll bar look big (1/2)

                    for result in self.metadata_receiver.try_iter().take(1) {
                        // [[processing-metadata]]
                        // items are added to the metadata receiver at [[@metadata-thread]]
                        for (index, song) in self
                            .songs
                            .articles
                            .iter_mut()
                            .enumerate()
                            .filter(|(_, s)| s.path == result.path)
                        //.take(1)
                        {
                            // This if statement is here because in the event of duplicate songs this would be run n^2 times, n being the number of duplicate songs in a playlist.
                            if !self.loaded_paths.contains(&result.path)
                                || (self.currently_selected_playlist_name
                                    == Some("Local Files".to_string()))
                            {
                                // TODO: Actual folder-space check
                                println!(
                                    "Processing a metadata result: {}",
                                    path_to_string(&result.path)
                                );
                                self.loaded_paths.insert(result.path.clone());
                                song.album = result.data.album.clone();
                                song.artist = result.data.artist.clone();
                                song.length_string = result.data.length_string.clone();
                                if !result.data.title.is_empty() {
                                    song.title = result.data.title.clone();
                                } else {
                                    song.title = path_to_string_name(&song.path);
                                }
                                song.cover_path = result.data.cover_path.clone();
                                if let Err(e) = self.m3u_sender.send(M3uEditTask::Edit(EditTrack {
                                    playlist_path: path_to_string(
                                        &self.currently_selected_playlist_path.to_path_buf(),
                                    ),
                                    track_path: path_to_string(&song.path),
                                    index,
                                    album: song.album.clone(),
                                    artist: song.artist.clone(),
                                    cover: song.cover_path.clone(),
                                    title: song.title.clone(),
                                    length_string: song.length_string.clone(),
                                })) {
                                    eprintln!("Failed to add metadata to queue: {}", e);
                                }
                            }
                        }
                    }
                    // / //                 // / //
                    // / displaying song cards / //
                    // / //                 // / //
                    let mut clicked_song = false;
                    for song_index in start..end {
                        let (clicked, secondary_clicked, double_clicked, move_to) =
                            draw_song_card(self, ctx, ui, song_index);
                        if double_clicked {
                            let song: &SongCardData = &self.songs.articles[song_index];
                            let path = song.path.clone();

                            self.now_playing = Some(path.clone());
                            self.now_playing_song = Some(song.clone());

                            play_song(self, path);
                        } else if clicked || secondary_clicked {
                            clicked_song = true;
                            ui.input(|input| {
                                if input.modifiers.shift {
                                    if self.selected_songs.len() == 0 {
                                        self.selected_songs_origin = song_index;
                                    }
                                    self.selected_songs.clear();
                                    for i in self.selected_songs_origin.min(song_index)
                                        ..=self.selected_songs_origin.max(song_index)
                                    {
                                        self.selected_songs.push(i);
                                    }
                                    if !self.selected_songs.contains(&song_index) {
                                        self.selected_songs.push(song_index);
                                    } else {
                                        println!(
                                            "Attempted selecting already selected song; skipping"
                                        );
                                    }
                                } else if input.modifiers.ctrl {
                                    self.selected_songs.push(song_index);
                                    self.selected_songs_origin = song_index;
                                } else {
                                    if clicked
                                        || (secondary_clicked
                                            && !self.selected_songs.contains(&song_index))
                                    {
                                        self.selected_songs.clear();
                                        self.selected_songs.push(song_index);
                                        self.selected_songs_origin = song_index;
                                    }
                                }
                            });
                        }
                        if let Some(target_idx) = move_to {
                            if let Some(source_idx) = self.dragged_song_index {
                                self.swap_request = Some((source_idx, target_idx));
                            }
                        }
                    }
                    if let Some(dragged_idx) = self.dragged_song_index {
                        if let Some(song) = self.songs.articles.get(dragged_idx) {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::Move);
                            egui::Tooltip::always_open(
                                ui.ctx().clone(),
                                ui.layer_id(),
                                egui::Id::new("playlist_drag_tooltip"),
                                egui::Pos2::ZERO,
                            )
                            .at_pointer()
                            .show(|ui| {
                                ui.set_max_width(300.0);
                                ui.add(
                                    egui::Label::new(song.title.clone())
                                        .wrap_mode(egui::TextWrapMode::Wrap),
                                );
                            });
                        }
                    }
                    if ui.input(|i| i.pointer.primary_released()) {
                        if !clicked_song {
                            println!("clicked not song, deselecting all songs");
                            self.selected_songs.clear();
                        }
                        if let Some((from, to)) = self.swap_request {
                            self.m3u_sender
                                .send(M3uEditTask::Move(MoveTrack {
                                    file_path: path_to_string(
                                        &self.currently_selected_playlist_path,
                                    ),
                                    from,
                                    to,
                                }))
                                .unwrap();
                            let song_card = self.songs.articles.remove(from);
                            let insert_at = if from < to { to - 1 } else { to };
                            self.songs.articles.insert(insert_at, song_card);
                            self.swap_request = None;
                        }
                    }
                    let remaining_px = (total_rows - end) as f32 * row_height; //      <- part of render buffer
                    ui.add_space(remaining_px); // makes scroll bar look big (2/2)  <- part of render buffer
                });
            });

            /*ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                egui::warn_if_debug_build(ui); // this was in the egui example.
            });*/
        });
        ctx.request_repaint_after(std::time::Duration::from_millis(300)); // Updates UI every 300ms, so that the duration bar moves smoothly when tabbed out

        /*egui::Window::new("Egui Settings").show(ctx, |ui| {
            // todo: make this a settings page
            ScrollArea::vertical().show(ui, |ui| {
                ctx.settings_ui(ui);
            });
        });*/
    }
}
