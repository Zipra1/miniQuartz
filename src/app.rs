//use std::collections::{binary_heap::{IntoIter, Iter}, hash_map::Iter};
use egui::ScrollArea;
use egui_extras::{TableBuilder,Column};
/// We derive Deserialize/Serialize so we can persist app state on shutdown.
#[derive(serde::Deserialize, serde::Serialize)]
#[serde(default)] // if we add new fields, give them default values when deserializing old state
pub struct TemplateApp {
    // Example stuff:
    label: String,

    #[serde(skip)] // This how you opt-out of serialization of a field
    value: f32,

    #[serde(skip)] // Opting out of serialization needs this thing above it. What's serde?
    songs: Songs, // I think this is basically doing what the 2021: 7a Rust tutorial said, but fits into the template given by egui. Probably better to do it like this? Feels cleaner.

    #[serde(skip)]
    row_height: Option<f32>,

    #[serde(skip)]
    col1_width: Option<f32>,

    #[serde(skip)]
    col2_width: Option<f32>,
}

impl Default for TemplateApp {
    fn default() -> Self {
        Self {
            // Example stuff:
            label: "Hello World!".to_owned(),
            value: 2.7,
            // Not example stuff:
            songs: Songs::new(),
            row_height: None,
            col1_width: None,
            col2_width: None,
        }
    }
}

impl TemplateApp {
    /// Called once before the first frame.

    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // This is also where you can customize the look and feel of egui using
        // `cc.egui_ctx.set_visuals` and `cc.egui_ctx.set_fonts`.

        // Load previous app state (if any).
        // Note that you must enable the `persistence` feature for this to work.
        if let Some(storage) = cc.storage {
            eframe::get_value(storage, eframe::APP_KEY).unwrap_or_default()
        } else {
            Default::default()
        }
    }
}

struct Songs {
    articles: Vec<SongCardData>
}

struct SongCardData{
    title: String,
    artist: String,
    length: String,
    cover_path: String,
    texture: Option<egui::TextureHandle>,
}

impl Songs{
    fn new() -> Songs{
        let iter = (0..8000).map(|a| SongCardData{ // todo: replace with file searching
            title: (a).to_string(),
            artist: (a+1).to_string(),
            length: (a*2).to_string(),
            cover_path: "assets/icon-256.png".to_owned(),
            texture: None,
        });
        Songs{
            articles: Vec::from_iter(iter)
        }
    }
}

impl SongCardData { //i must be for real this section is written by ai. im Sorry. but im fuck at rust,, this should be rewritten later, though.
    fn load_texture_if_needed(&mut self, ctx: &egui::Context) {
        if self.texture.is_none() {
            if let Ok(image) = image::open(&self.cover_path) {
                let image = image.to_rgba8();
                let size = [image.width() as usize, image.height() as usize];
                let texture = ctx.load_texture(
                    self.cover_path.clone(), 
                    egui::ColorImage::from_rgba_unmultiplied(size, &image),
                    Default::default()
                );
                self.texture = Some(texture);
            }
        }
    }
}

//--(^人^)---(^人^)--//
//   Main app logic  //
//--(^人^)---(^人^)--//
impl eframe::App for TemplateApp {
    /// Called by the framework to save state before shutdown.
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, eframe::APP_KEY, self);
    } //todo: deciper how the example app does this stuff; how do you add something to be saved on reboot?

    /// Called each time the UI needs repainting, which may be many times per second.
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Put your widgets into a `SidePanel`, `TopBottomPanel`, `CentralPanel`, `Window` or `Area`.
        // For inspiration and more examples, go to https://emilk.github.io/egui
        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            // The top panel is often a good place for a menu bar:

            egui::MenuBar::new().ui(ui, |ui| {
                // NOTE: no File->Quit on web pages!
                let is_web = cfg!(target_arch = "wasm32");
                if !is_web {
                    ui.menu_button("File", |ui| {
                        if ui.button("Quit").clicked() {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    });
                    ui.add_space(16.0);
                }

                egui::widgets::global_theme_preference_buttons(ui);
            });
        });

        //--\(￣︶￣*\))---\(￣︶￣*\))---\(￣︶￣*\))---\(￣︶￣*\))--//
        //    Bottom bar to display track info and track controls    //
        //--\(￣︶￣*\))---\(￣︶￣*\))---\(￣︶￣*\))---\(￣︶￣*\))--//
        egui::TopBottomPanel::bottom("status") // todo: make this resizable properly.
        .resizable(true)
        .min_height(50.0)
        .show(ctx, |ui|{
            ScrollArea::horizontal().show(ui,|ui|{
                ui.set_min_height(ui.available_height());
                ui.label("Status hereeee!!!");
            });
        });

        //--(*￣3￣)╭----(*￣3￣)╭---(*￣3￣)╭----(*￣3￣)╭--//
        // Side panel to display playlists and app controls //
        //--(*￣3￣)╭----(*￣3￣)╭---(*￣3￣)╭----(*￣3￣)╭--//
        egui::SidePanel::left("playlists")
        .resizable(true)
        .min_width(30.0)
        .show(ctx, |ui|{
            ui.heading("miniQuartz");
            let fps = 1.0 / ctx.input(|i| i.stable_dt.max(0.0001)); // fps counter for extra awesome
            ui.label(format!("FPS: {:.1}", fps));
            ScrollArea::vertical().show(ui, |ui|{
                ui.set_min_width(ui.available_width()); // this makes smooth resizing possible. feels kinda jank but whatever.
                ui.label("Playlists Hereeeeeeeeeee");
            });
            
        });

        //--◑﹏◐---◑﹏◐---◑﹏◐---◑﹏◐---◑﹏◐---◑﹏◐---◑﹏◐---◑﹏◐---◑﹏◐---◑﹏◐-//
        //   Central pain to display: Playlist contents, album contents, artist pages   //
        //--◑﹏◐---◑﹏◐---◑﹏◐---◑﹏◐---◑﹏◐---◑﹏◐---◑﹏◐---◑﹏◐---◑﹏◐---◑﹏◐-//
        egui::CentralPanel::default().show(ctx, |ui| { // central panel has to be rendered after other panels
            ui.heading("Playlist Name Here");
            let available_width = ui.available_width();
            let col_time_width= 130.0; // defioned here bc its used in many places and itd be annoying to change them both every time
            let col1_width = self.col1_width.unwrap_or(30.0);
            let col2_width = self.col2_width.unwrap_or(30.0);
            ui.label(col1_width.to_string());
            ui.label(col2_width.to_string());
            let last_column_width = available_width-(20.0+col2_width+col_time_width); // proper row height: it feels wrong to be setting this every frame.
                TableBuilder::new(ui)
                            .column(Column::exact(20.0))
                            .column(Column::auto().resizable(true).at_least(50.0))
                            .column(Column::exact(last_column_width))
                            .column(Column::exact(col_time_width))
                            .header(20.0, |mut header| {
                                header.col(|ui|{
                                    ui.vertical_centered(|ui|{
                                        ui.heading("#");
                                    });
                                });
                                header.col(|ui|{
                                    ui.vertical_centered(|ui|{
                                        ui.heading("Name");
                                        self.col2_width = Some(ui.available_width());
                                    });
                                });
                                header.col(|ui|{
                                    ui.vertical_centered(|ui|{
                                        ui.heading("Album");
                                        self.col1_width = Some(ui.available_width());
                                    });
                                });
                                header.col(|ui|{
                                    ui.vertical_centered(|ui|{
                                        ui.heading("Time");
                                    });
                                });
                            })
                            .body(|mut body| {
                                body.row(0.0, |mut row| {
                                    row.col(|ui|{
                                    });
                                    row.col(|ui|{ // urghh the grabby bits are actually attached to these so u cant remove these empty cells
                                    });
                                    row.col(|ui|{
                                    });
                                    row.col(|ui|{
                                    });
                                });
                            });

            ScrollArea::vertical()
            //.max_width(available_width-5.0)
            .show(ui,|ui|{

                // render buffer stuff
                let row_height = self.row_height.unwrap_or(30.0); // proper row height: it feels wrong to be setting this every frame.
                let total_rows = self.songs.articles.len(); // it feels wrong to be setting this every frame. this only really needs to be set if the shown list changes.

                let clip_rect = ui.clip_rect();
                let top = clip_rect.top();
                let bottom= clip_rect.bottom();

                let mut start = ((top - ui.min_rect().top()) / row_height).floor() as usize;
                let mut end = ((bottom - ui.min_rect().top()) / row_height).ceil() as usize;

                let render_buffer_size = 5; // If fast scrolling causes images not to load, increase this.

                start = start.saturating_sub(render_buffer_size);
                end = (end + render_buffer_size).min(total_rows);

                let above_px = start as f32 * row_height;
                ui.add_space(above_px); // makes scroll bar look big (1/2)

                ui.label(available_width.to_string());

                for i in start..end{ // Display tracks that should be displayed
                // no longer render buffer stuff
                    let song = &mut self.songs.articles[i]; // Ampersand makes it read-only, since the for loop tries to own "articles"
                    song.load_texture_if_needed(ctx);

                    //ui.set_min_width(available_width-20.0);
                    let group = ui.group(|ui|{
                        //ui.horizontal(|ui|{
                        TableBuilder::new(ui)
                            .column(Column::exact(col2_width+20.0)) // 20.0 comes from the first header (#) column's exact width. should be set to a variable! todo
                            .column(Column::exact(col1_width))
                            .header(30.0, |mut header|{
                                header.col(|ui|{
                                    ui.horizontal(|ui|{
                                        if let Some(tex) = &song.texture {
                                            ui.add(
                                            egui::Image::new(tex) // TODO: Images are currently stored at native resolution and then scaled down here. They should be stored at display resolution.
                                                .max_width(30.0)
                                                .corner_radius(10),
                                        );
                                        } else {
                                            ui.label("img not found"); // TODO: "no album" image instead of text
                                        }
                                        ui.vertical_centered(|ui|{ // song & artist names
                                            ui.label(egui::RichText::new(format!("Title: {}", song.title)).strong());
                                            ui.label(format!("Artist {}", song.artist));
                                        });
                                    });
                                });
                                header.col(|ui|{
                                    ui.vertical_centered(|ui|{
                                        ui.label("nyaaaaaaaa");
                                    });
                                });
                            });
                            

                            ui.with_layout(egui::Layout::right_to_left(egui::Align::TOP), |ui|{ // song length
                                ui.add_space(20.0);
                                ui.label(format!("Length {}", song.length)); // this will need to convert whatever songs have (probably ms) into H:M:S format in the future
                            }); 
                        //});
                    });
                    if self.row_height.is_none(){
                        self.row_height = Some(group.response.rect.height()); // todo: this is in the for loop and is probably fuck for performance \(￣︶￣*\))
                    }
                }

                
                let remaining_px = (total_rows - end) as f32 * row_height; //      <- part of render buffer
                ui.add_space(remaining_px); // makes scroll bar look big (2/2)  <- part of render buffer
            });
            

            ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                egui::warn_if_debug_build(ui); // this was in the example thing and idk if its needed or if theres a benefit to removing it
            });
        });
    }
}
