use std::path::PathBuf;

use egui::{Context, Id, Ui};

use crate::TemplateApp;
use crate::app::{AddTrack, M3uEditTask, RemovePlaylist, RemoveTrack, load_metadata_if_needed};
use crate::playlist::{SongCardData, create_empty_m3u};
use crate::playlist::{get_playlists, reset_playlist_ids};
use crate::utilities::{path_to_string, path_to_string_name, show_error, to_base62};

/// UI ///
/// Drawing functions
///

pub fn draw_drop_bar(ui: &mut egui::Ui, start: egui::Pos2, end: egui::Pos2) {
    // This should be in a different UI file, since this UI file is meant to be juist for songs.
    let color = ui.visuals().selection.bg_fill;
    let stroke = egui::Stroke::new(2.0, color);
    ui.painter().line_segment([start, end], stroke);
    ui.painter().circle_filled(start, 3.0, stroke.color);
    ui.painter().circle_filled(end, 3.0, stroke.color);
}

pub fn right_click_song_card(
    app: &mut TemplateApp,
    ui: &mut egui::Ui,
    mut song_data: SongCardData,
) {
    ui.set_max_width(200.0);

    ui.menu_button("Add to playlist", |ui| {
        for playlist in &app.playlists {
            let playlist_name = &path_to_string_name(playlist)[4..];
            let playlist_path = path_to_string(&playlist.to_path_buf());
            if ui.button(playlist_name).clicked() {
                for song in &app.selected_songs {
                    if let Err(e) = app.m3u_sender.send(M3uEditTask::Add(AddTrack {
                        file_path: playlist_path.clone(),
                        new_song: app.songs.articles[song.clone()].clone(),
                    })) {
                        eprintln!("Failed to queue M3uEditTask: {}", e);
                    }
                }
                // if Some(playlist_path) == app.currently_selected_playlist_name {
                //     for song in &app.selected_songs {
                //         app.songs.articles.push(app.songs.articles[song.clone()].clone());
                //     }
                // }

                // not really sure why push isn't working here? this is definetly not the way extend is meant to be used
                if Some(playlist_name) == app.currently_selected_playlist_name.as_deref() {
                    for song in &app.selected_songs {
                        app.songs
                            .articles
                            .extend([app.songs.articles[song.clone()].clone()]);
                    }
                }
            }
        }
        let _ = ui.button("todo - New Playlist & Playlist Folders");
    });
    if ui.button("Remove from playlist").clicked() {
        let playlist_path = path_to_string(&app.currently_selected_playlist_path.to_path_buf());
        app.selected_songs.sort_by(|a, b| b.cmp(a));
        for song in &app.selected_songs {
            if let Err(e) = app.m3u_sender.send(M3uEditTask::Remove(RemoveTrack {
                file_path: playlist_path.clone(),
                index_to_remove: song.clone(),
            })) {
                //show_error(app, format!("Failed to add removal to queue: {}", e));
                eprintln!("Failed to add removal to queue: {}", e);
            }
            if song < &app.songs.articles.len() {
                app.songs.articles.remove(*song);
            } else {
                eprintln!("song_ui | Remove from playlist: Index for removal out of bounds.");
            }
        }
    }
    if ui.button("Update Metadata").clicked() {
        load_metadata_if_needed(&mut song_data, app.metadata_sender.clone());
    }
}

pub fn right_click_playlist(app: &mut TemplateApp, ui: &mut egui::Ui, playlist_index: usize) {
    ui.set_max_width(200.0);
    let playlist = &app.playlists[playlist_index];
    if ui.button("Rename playlist").clicked() {
        app.rename_playlist_show = true;
        app.playlist_to_rename = Some(playlist.to_path_buf());
        let name = &path_to_string_name(playlist)[4..];
        app.rename_to = name[..name.len() - 4].to_string();
    }
    if ui.button("Delete playlist").clicked() {
        app.warning_show = true;
        app.playlist_to_delete = Some(playlist.to_path_buf());
    }
    if ui.button("Add new playlist").clicked() {
        reset_playlist_ids(app);
        let count = to_base62(app.playlists.len() + 1, 4);
        let new_playlist_path = PathBuf::from(format!("./playlists/{}new playlist.m3u", count));
        if let Err(error) = create_empty_m3u(&new_playlist_path) {
            //show_error(app, error.to_string());
            eprintln!("create_empty_m3u error: {}", error.to_string());
        }
        app.rename_playlist_show = true;
        app.playlist_to_rename = Some(new_playlist_path.clone());
        let name = &path_to_string_name(&new_playlist_path)[4..];
        app.rename_to = name[..name.len() - 4].to_string();

        app.playlists = get_playlists("./playlists/").unwrap_or_default();
    }
}

pub fn delete_playlist_warning(app: &mut TemplateApp, ui: &mut egui::Ui) {
    let modal_response = egui::Modal::new(Id::new("Deletion warning")).show(ui.ctx(), |ui| {
        ui.set_width(200.0);
        ui.heading("Delete playlist?");
        ui.label(path_to_string(
            &app.playlist_to_delete.clone().unwrap_or(PathBuf::from("")),
        ));

        ui.add_space(32.0);

        egui::Sides::new().show(
            ui,
            |_ui| {},
            |ui| {
                if ui.button("Yes").clicked() {
                    app.warning_show = false;
                    if app.playlist_to_delete.is_some() {
                        if let Err(e) =
                            app.m3u_sender
                                .send(M3uEditTask::RemovePlaylist(RemovePlaylist {
                                    file_path: app.playlist_to_delete.clone(),
                                }))
                        {
                            eprintln!("Failed to add playlist deletion to queue: {}", e);
                        }
                        if let Err(e) =
                            std::fs::remove_file(app.playlist_to_delete.as_ref().unwrap())
                        {
                            show_error(app, format!("Failed to delete file: {}", e.to_string()));
                            println!("delete_playlist_warning: {}", e);
                        }
                        // let index = app.playlists.iter().position(|item| item == &app.playlist_to_delete.clone().unwrap_or(PathBuf::from("")));
                        // app.playlists.remove(index.unwrap_or(69420));

                        app.playlists = get_playlists("./playlists/").unwrap_or_default(); // get because one is now deleted & reset_playlist_ids wont like that
                        reset_playlist_ids(app);
                        app.playlists = get_playlists("./playlists/").unwrap_or_default(); // get again because id's are now changed
                        // doing this twice? bleh. should just be removing the single removed playlist from ram or skipping it in get_playlists*/
                    }
                }

                if ui.button("Cancel").clicked() {
                    app.warning_show = false;
                    app.playlist_to_delete = None;
                }
            },
        );
    });
    if modal_response.response.clicked_elsewhere() {
        println!("clicked outside of deletion modal; closing modal");
        app.warning_show = false;
        app.playlist_to_delete = None;
    }
}

pub fn rename_playlist(app: &mut TemplateApp, ui: &mut egui::Ui) {
    let modal_response = egui::Modal::new(Id::new("Playlist options")).show(ui.ctx(), |ui| {
        ui.set_width(200.0);
        ui.heading("Rename playlist");
        let mut text = app.rename_to.clone();
        ui.text_edit_singleline(&mut app.rename_to);

        ui.add_space(32.0);

        egui::Sides::new().show(
            ui,
            |_ui| {},
            |ui| {
                if ui.button("Save").clicked() {
                    let idx = &path_to_string_name(&app.playlist_to_rename.as_ref().unwrap())[..4];
                    let mut set_current = false;
                    if &app.currently_selected_playlist_path == app.playlist_to_rename.as_ref().unwrap_or(&app.currently_selected_playlist_path){
                        set_current = true;
                    }
                    text = format!("{}{}.m3u",idx,text);
                    if let Some(old_path) = &app.playlist_to_rename{
                        if let Some(parent) = old_path.parent(){
                            let new_path = parent.join(&text);
                            if &new_path != old_path{
                                if new_path.try_exists().unwrap_or(false){
                                    show_error(app,format!("Playlist already exists! If you're seeing this, something went very wrong (✿uwu)\nold path: {} \nnew path: {}",path_to_string(old_path),path_to_string(&new_path)));
                                    eprintln!("{}","Playlist already exists?".to_string());
                                }else{
                                    if let Err(error) = std::fs::rename(app.playlist_to_rename.as_ref().unwrap(), &new_path) {
                                        show_error(
                                            app,
                                            format!(
                                                "rename playlist err: {} | from: {} | to: {}",
                                                error.to_string(),
                                                path_to_string(&app.playlist_to_rename.as_ref().unwrap()),
                                                text
                                            ),
                                        );
                                        eprintln!("rename_playlist std::fs::rename err: {} | from: {} | to: {}",
                                                error.to_string(),
                                                path_to_string(&app.playlist_to_rename.as_ref().unwrap()),
                                                text)
                                    }
                                    app.playlists = get_playlists("./playlists/").unwrap_or_default();
                                    if set_current{
                                        app.currently_selected_playlist_path = new_path;
                                    }
                                }
                            }
                        }
                    }
                    app.rename_playlist_show = false;
                    app.playlist_to_rename = None;
                }
                if ui.button("Cancel").clicked(){
                    app.rename_playlist_show = false;
                    app.playlist_to_rename = None;
                }
            },
        );
    });
    if modal_response.response.clicked_elsewhere() {
        println!("clicked outside of renaming modal; closing modal");
        app.rename_playlist_show = false;
        app.playlist_to_rename = None;
    }
}

pub fn draw_song_card(
    app: &mut TemplateApp,
    ctx: &Context,
    ui: &mut Ui,
    i: usize,
) -> (bool, bool, bool, Option<usize>) {
    if app.songs.articles.len() <= i {
        eprintln!("draw_song_card: Index out of range error");
        return (false, false, false, None);
    } /* this triggering when removing a song from a playlist is normal, since when you delete a song
    it removes an item from app.songs.articles before the for loop drawing the cards is finished.
    not sure if there's a better way to handle it, but this feels Just Okay. */
    let song = &mut app.songs.articles[i];
    let selected = app.selected_songs.contains(&i) as i32 as f32;
    let mut clicked = false;
    let mut secondary_clicked = false;
    let mut double_clicked = false;
    let mut move_to = None;
    if !song.display {
        println!(
            "song display false; nothing rendered for {} - {}",
            song.title, song.artist
        );
        return (false, false, false, None);
    }
    if !app.loaded_paths.contains(&song.path)
        || (app.currently_selected_playlist_name == Some("Local Files".to_string()))
    {
        // TODO: Actual folder-space check
        load_metadata_if_needed(song, app.metadata_sender.clone());
        /* This is only being done ONCE per song, because load_metadata_if_needed is actually just for the cache.
           The song cards themselves retrieve their data only from the cache. */
    }
    song.load_texture_if_needed(ctx);

    ui.spacing_mut().item_spacing.y = 0.0;

    let response = ui
        .scope_builder(
            egui::UiBuilder::new()
                .id_salt(i)
                .sense(egui::Sense::click()),
            |ui| {
                let response = ui.response();

            let is_upper_half = ui.input(|i| i.pointer.hover_pos()).map_or(true, |pos| pos.y < response.rect.center().y);

            if ui.input(|i| i.pointer.primary_released()) {
                app.drag_origin = None;
                app.dragging_song = None;
            }
            if response.is_pointer_button_down_on() && ui.input(|i| i.pointer.primary_down()){
                app.dragging_song = Some(i);
                app.drag_origin = ui.input(|i| i.pointer.press_origin());
            }
            if app.dragging_song == Some(i) {
                /* this needs the check to see if we are dragging the right song because without
                it every card gets set to being dragged because dragging_song is global.
                hovered() is broken here, because song cards are in a scroll area & that steals focus.
                so, the song card to drag should be set upon response.is_pointer_button_down_on()
                that way, only the clicked song cards index gets set to i.
                so much for just a drag buffer! */
                let delta = ui.input(|i| i.pointer.latest_pos()).unwrap_or(egui::Pos2::new(0.0,0.0)).distance(app.drag_origin.unwrap_or(egui::Pos2::new(0.0,0.0)));
                app.test_thing = Some(delta);
                if delta > 2.0 {
                    app.dragged_song_index = Some(i);
                }
            }

            if let Some(from_idx) = app.dragged_song_index {
                if response.contains_pointer() && from_idx != i {
                    let rect = response.rect;
                    let mut start = rect.left_bottom();
                    let mut end = rect.right_bottom();
                    if is_upper_half {
                        move_to = Some(i);
                        start = rect.left_top();
                        end = rect.right_top();
                    } else{
                        move_to = Some(i+1);
                    }
                    draw_drop_bar(ui, start, end);
                }
            }

            let visuals = ui.style().interact(&response);
            let fill_color =
                if response.contains_pointer() || response.has_focus() {
                    visuals.bg_fill.gamma_multiply(0.3*(selected+1.0))
                } else {
                    visuals.bg_fill.gamma_multiply(selected*0.3)
                };
            egui::Frame::new()
                .fill(fill_color)
                .inner_margin(ui.spacing().menu_margin)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let playing = app.now_playing == Some(song.path.clone());
                        let color_playing = if playing
                        /* todo: this should be based off of the ID in the list, and the currently selected playlist.
                        Will need to also add logic in the song reordering area to change the currently selected ID */
                        {
                            ui.visuals().selection.stroke.color
                        } else {
                            ui.visuals().strong_text_color()
                        };
                                if playing{
                                    ui.add(egui::Label::new(egui::RichText::new("▶").color(ui.visuals().selection.stroke.color)).truncate());
                                } else {
                                    ui.add(egui::Label::new(egui::RichText::new(format!("{}",i+1)).color(ui.visuals().text_color())).truncate());
                                };

                        ui.scope(|ui| {
                            ui.set_width(
                                app.title_header_width + 25.0,
                            );
                            if let Some(tex) = &song.texture {
                                ui.add(
                                    egui::Image::new(tex)
                                        .max_width(30.0)
                                        .corner_radius(3), // todo: this should be user configurable. some people haaate corner radius on album art
                                );
                            } else {
                                ui.add(
                                    egui::Spinner::new()
                                        .size(30.0)
                                        .color(egui::Color32::BLUE),
                                        /* for some reason the spinner is slightly larger than the image, despite being 30.0?
                                            it might have some sort of padding, but im not sure how to change that. */
                                );
                            }
                            ui.vertical(|ui| {
                                // song & artist names
                                ui.add(
                                    egui::Label::new(
                                        egui::RichText::new(
                                            &song.title,
                                        )
                                        .color(color_playing),
                                    )
                                    .truncate(),
                                );
                                ui.add(
                                    egui::Label::new(&song.artist)
                                        .truncate(),
                                );
                            });
                        });
                        let remaining_width = (ui.available_width() - 60.0).clamp(0.0, f32::MAX);

                        ui.allocate_ui_with_layout(
                            egui::vec2(
                                remaining_width,
                                ui.available_height(),
                            ),
                            egui::Layout::left_to_right(
                                egui::Align::Center,
                            ),
                            |ui| {
                                ui.add(
                                    egui::Label::new(&song.album)
                                        .truncate(),
                                );
                            },
                        );
                        ui.with_layout(
                            egui::Layout::right_to_left(
                                egui::Align::TOP,
                            ),
                            |ui| {
                                ui.add_space(10.0);
                                ui.label(format!(
                                    "{}",
                                    &song.length_string
                                ));
                                //ui.label(path_to_string(&song.path));
                            },
                        );
                    });
                });
            },
        )
        .response;
    if response.double_clicked() {
        double_clicked = true;
    }
    if response.clicked() {
        clicked = true;
    }
    if response.secondary_clicked() {
        secondary_clicked = true;
    }

    if app.row_height.is_none() {
        app.row_height = Some(response.rect.height()); // todo: this is in the for loop and is probably fuck for performance \(￣︶￣*\))
    } // this really only needs to be done on startup

    /*if app.now_playing == Some(song.path.clone()) {
        // todo: this check should be based on file *and* playlist!
        ui.painter()
            .rect_filled(response.rect, 4.0, egui::Color32::from_white_alpha(10));
    }*/
    let song_send = song.clone();
    app.apply_options(
        egui::Popup::context_menu(&response).id(Id::new(format!("context_menu{}", i))),
    )
    .show(|ui| right_click_song_card(app, ui, song_send));

    (clicked, secondary_clicked, double_clicked, move_to)
}
