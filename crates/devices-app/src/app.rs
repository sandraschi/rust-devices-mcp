//! egui dashboard: five tabs over one `dispatch`, zero JavaScript.
//!
//! Every button maps to a `Command` variant; async results arrive over mpsc
//! and the UI repaints while jobs are pending. Snapshots decode to RGB on
//! the worker and become GPU textures on the UI thread.
use devices_core::{
    camera::{self, CamSource},
    creds::{Credentials, Vendor},
    dispatch, Command,
};
use eframe::egui;
use serde_json::Value;
use std::collections::HashMap;
use std::future::Future;
use std::sync::{mpsc, Arc};
use std::time::Instant;

#[derive(PartialEq, Clone, Copy)]
enum Tab {
    Plugs,
    Hue,
    Weather,
    Cameras,
    Ring,
}

impl Tab {
    fn label(&self) -> &'static str {
        match self {
            Self::Plugs => "Plugs",
            Self::Hue => "Hue",
            Self::Weather => "Weather",
            Self::Cameras => "Cameras",
            Self::Ring => "Ring",
        }
    }
}

enum Job {
    Status(Value),
    Plug(Value),
    HueLights(Value),
    HueSet(Value),
    Weather(Value),
    Snapshot {
        label: String,
        meta: Value,
        jpeg: Option<Vec<u8>>,
    },
    Ring(Value),
}

struct Shot {
    label: String,
    meta: Value,
    texture: Option<egui::TextureHandle>,
}

pub struct DevicesApp {
    rt: Arc<tokio::runtime::Runtime>,
    tx: mpsc::Sender<Job>,
    rx: mpsc::Receiver<Job>,
    pending: usize,
    tab: Tab,
    status_line: String,
    raw: String,
    // Plugs tab.
    plug_vendor: String,
    plug_ip: String,
    plug_user: String,
    plug_pass: String,
    plug_cards: Vec<Value>,
    // Hue tab.
    hue_ip: String,
    hue_user: String,
    lights: Vec<Value>,
    bri_pending: HashMap<String, u8>,
    bri_sent_at: HashMap<String, Instant>,
    // Weather tab.
    wx_lat: String,
    wx_lon: String,
    wx_card: Option<Value>,
    // Cameras tab.
    cam_source: String,
    cam_ip: String,
    cam_model: String,
    cam_user: String,
    cam_pass: String,
    cam_index: String,
    cam_url: String,
    cam_token: String,
    shots: Vec<Shot>,
    // Ring tab.
    ring_email: String,
    ring_pass: String,
    ring_token: String,
    ring_id: String,
    ring_cards: Vec<Value>,
}

fn env_or(key: &str) -> String {
    std::env::var(key).unwrap_or_default()
}

impl DevicesApp {
    pub fn new(ctx: &eframe::CreationContext<'_>, rt: Arc<tokio::runtime::Runtime>) -> Self {
        ctx.egui_ctx.set_visuals(egui::Visuals::dark());
        let (tx, rx) = mpsc::channel();
        let mut app = Self {
            rt,
            tx,
            rx,
            pending: 0,
            tab: Tab::Plugs,
            status_line: "starting…".to_string(),
            raw: String::new(),
            plug_vendor: "tapo".to_string(),
            plug_ip: String::new(),
            plug_user: String::new(),
            plug_pass: String::new(),
            plug_cards: Vec::new(),
            hue_ip: env_or("HUE_BRIDGE_IP"),
            hue_user: env_or("HUE_USERNAME"),
            lights: Vec::new(),
            bri_pending: HashMap::new(),
            bri_sent_at: HashMap::new(),
            wx_lat: String::new(),
            wx_lon: String::new(),
            wx_card: None,
            cam_source: "usb".to_string(),
            cam_ip: String::new(),
            cam_model: String::new(),
            cam_user: String::new(),
            cam_pass: String::new(),
            cam_index: "0".to_string(),
            cam_url: String::new(),
            cam_token: env_or("RING_TOKEN_FILE"),
            shots: Vec::new(),
            ring_email: String::new(),
            ring_pass: String::new(),
            ring_token: env_or("RING_TOKEN_FILE"),
            ring_id: String::new(),
            ring_cards: Vec::new(),
        };
        app.spawn(|| async { Job::Status(dispatch(Command::Status).await) });
        app.spawn_weather(None, None);
        app
    }

    fn spawn<F>(&mut self, make: impl FnOnce() -> F + Send + 'static)
    where
        F: Future<Output = Job> + Send + 'static,
    {
        let tx = self.tx.clone();
        self.rt.spawn(async move {
            let _ = tx.send(make().await);
        });
        self.pending += 1;
    }

    fn note(&mut self, line: impl Into<String>) {
        self.status_line = line.into();
    }

    fn show_raw(&mut self, value: &Value) {
        self.raw = serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string());
    }

    fn drain(&mut self, ctx: &egui::Context) {
        let mut repaints = 0;
        while let Ok(job) = self.rx.try_recv() {
            self.pending = self.pending.saturating_sub(1);
            match job {
                Job::Status(value) => {
                    self.show_raw(&value);
                    self.note("connected to devices-core (in-process)");
                }
                Job::Plug(value) => {
                    self.show_raw(&value);
                    self.plug_cards.push(value);
                    if self.plug_cards.len() > 6 {
                        self.plug_cards.remove(0);
                    }
                }
                Job::HueLights(value) => {
                    self.show_raw(&value);
                    if let Some(list) = value.as_array() {
                        self.lights = list.clone();
                    } else {
                        self.lights = vec![value];
                    }
                }
                Job::HueSet(value) => {
                    self.show_raw(&value);
                }
                Job::Weather(value) => {
                    self.show_raw(&value);
                    self.wx_card = Some(value);
                }
                Job::Snapshot { label, meta, jpeg } => {
                    self.show_raw(&meta);
                    let texture = jpeg.and_then(|bytes| {
                        let rgb = image::load_from_memory(&bytes).ok()?.to_rgb8();
                        let (width, height) = (rgb.width() as usize, rgb.height() as usize);
                        Some(ctx.load_texture(
                            &label,
                            egui::ColorImage::from_rgb([width, height], &rgb.into_raw()),
                            egui::TextureOptions::LINEAR,
                        ))
                    });
                    if self.shots.len() >= 8 {
                        self.shots.remove(0);
                    }
                    self.shots.push(Shot {
                        label,
                        meta,
                        texture,
                    });
                }
                Job::Ring(value) => {
                    self.show_raw(&value);
                    self.ring_cards.push(value);
                    if self.ring_cards.len() > 6 {
                        self.ring_cards.remove(0);
                    }
                }
            }
            repaints += 1;
            if repaints > 16 {
                break;
            }
        }
        if self.pending > 0 {
            ctx.request_repaint();
        }
    }

    fn opt(text: &str) -> Option<String> {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    }

    fn plug_creds(&self) -> Result<(Vendor, Credentials), String> {
        let vendor = Vendor::parse(&self.plug_vendor).map_err(|e| e.to_string())?;
        let creds = Credentials::resolve(
            vendor,
            Self::opt(&self.plug_user),
            Self::opt(&self.plug_pass),
        )
        .map_err(|e| e.to_string())?;
        Ok((vendor, creds))
    }

    fn spawn_plug(&mut self, state: Option<bool>) {
        let parsed = self.plug_creds();
        let ip = self.plug_ip.trim().to_string();
        match (parsed, state) {
            (Ok((vendor, creds)), None) => {
                self.note(format!("querying {} plug @ {ip}…", vendor.as_str()));
                self.spawn(move || async move {
                    Job::Plug(
                        dispatch(Command::PlugStats {
                            vendor,
                            device_ip: ip,
                            creds,
                        })
                        .await,
                    )
                });
            }
            (Ok((vendor, creds)), Some(on)) => {
                self.note(format!("toggling {} plug @ {ip}…", vendor.as_str()));
                self.spawn(move || async move {
                    Job::Plug(
                        dispatch(Command::PlugToggle {
                            vendor,
                            device_ip: ip,
                            state: on,
                            creds,
                        })
                        .await,
                    )
                });
            }
            (Err(e), _) => {
                self.note(e.clone());
                self.raw = serde_json::json!({"error": e}).to_string();
            }
        }
    }

    fn spawn_hue_lights(&mut self) {
        let (ip, user) = (Self::opt(&self.hue_ip), Self::opt(&self.hue_user));
        self.note("listing Hue lights…");
        self.spawn(move || async move {
            Job::HueLights(
                dispatch(Command::HueLights {
                    bridge_ip: ip,
                    username: user,
                })
                .await,
            )
        });
    }

    fn spawn_hue_set(&mut self, light_id: String, on: bool, bri: Option<u8>) {
        let (ip, user) = (Self::opt(&self.hue_ip), Self::opt(&self.hue_user));
        self.spawn(move || async move {
            Job::HueSet(
                dispatch(Command::HueSetLight {
                    bridge_ip: ip,
                    username: user,
                    light_id,
                    on,
                    bri,
                })
                .await,
            )
        });
    }

    fn spawn_weather(&mut self, lat: Option<f64>, lon: Option<f64>) {
        self.note("fetching weather…");
        self.spawn(move || async move {
            Job::Weather(
                dispatch(Command::Weather {
                    latitude: lat,
                    longitude: lon,
                })
                .await,
            )
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn spawn_snapshot(
        &mut self,
        label: String,
        source: CamSource,
        device_ip: String,
        model: Option<String>,
        username: Option<String>,
        password: Option<String>,
        onvif_port: Option<u16>,
        device_index: Option<u32>,
        url: Option<String>,
        ring_token_file: Option<String>,
    ) {
        self.note(format!("grabbing {label}…"));
        self.spawn(move || async move {
            match devices_core::camera::snapshot_full(
                source,
                &device_ip,
                model,
                username,
                password,
                onvif_port,
                device_index,
                url,
                None,
                None,
                ring_token_file,
            )
            .await
            {
                Ok(frame) => {
                    let meta = serde_json::to_value(&frame.meta).unwrap_or(serde_json::Value::Null);
                    Job::Snapshot {
                        label,
                        meta,
                        jpeg: Some(frame.jpeg),
                    }
                }
                Err(e) => Job::Snapshot {
                    label,
                    meta: serde_json::json!({"error": e.to_string()}),
                    jpeg: None,
                },
            }
        });
    }

    fn spawn_ring(&mut self, doorbells: bool) {
        let (email, pass, token) = (
            Self::opt(&self.ring_email),
            Self::opt(&self.ring_pass),
            Self::opt(&self.ring_token),
        );
        self.note(if doorbells {
            "listing Ring doorbells…"
        } else {
            "checking Ring status…"
        });
        self.spawn(move || async move {
            let value = if doorbells {
                dispatch(Command::RingDoorbells {
                    email,
                    password: pass,
                    token_file: token,
                })
                .await
            } else {
                dispatch(Command::RingStatus {
                    email,
                    password: pass,
                    token_file: token,
                })
                .await
            };
            Job::Ring(value)
        });
    }
}

fn field(ui: &mut egui::Ui, label: &str, text: &mut String, password: bool, hint: &str) {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.add(
            egui::TextEdit::singleline(text)
                .password(password)
                .hint_text(hint)
                .desired_width(170.0),
        );
    });
}

fn value_rows(ui: &mut egui::Ui, value: &Value) {
    if let Some(err) = value.get("error").and_then(|e| e.as_str()) {
        ui.colored_label(egui::Color32::LIGHT_RED, err);
        return;
    }
    if let Some(map) = value.as_object() {
        egui::Grid::new(format!("grid-{:p}", map as *const _))
            .num_columns(2)
            .striped(true)
            .show(ui, |ui| {
                for (key, val) in map {
                    ui.label(key);
                    let text = match val {
                        Value::String(s) => s.clone(),
                        Value::Null => "—".to_string(),
                        other => other.to_string(),
                    };
                    ui.monospace(text);
                    ui.end_row();
                }
            });
    } else {
        ui.monospace(value.to_string());
    }
}

impl eframe::App for DevicesApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.drain(&ctx);

        egui::Panel::top("tabs").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.heading("Devices (Rust)");
                for tab in [Tab::Plugs, Tab::Hue, Tab::Weather, Tab::Cameras, Tab::Ring] {
                    ui.selectable_value(&mut self.tab, tab, tab.label());
                }
                if self.pending > 0 {
                    ui.spinner();
                    ui.label(format!("{} jobs", self.pending));
                }
            });
        });

        egui::Panel::bottom("status").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(&self.status_line);
            });
        });

        egui::CentralPanel::default().show(ui, |ui| match self.tab {
            Tab::Plugs => self.tab_plugs(ui),
            Tab::Hue => self.tab_hue(ui),
            Tab::Weather => self.tab_weather(ui),
            Tab::Cameras => self.tab_cameras(ui),
            Tab::Ring => self.tab_ring(ui),
        });

        egui::Panel::right("raw").resizable(true).show(ui, |ui| {
            ui.heading("Raw JSON");
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.monospace(&self.raw);
            });
        });
    }
}

impl DevicesApp {
    fn tab_plugs(&mut self, ui: &mut egui::Ui) {
        ui.heading("Smart plugs");
        field(
            ui,
            "Vendor",
            &mut self.plug_vendor,
            false,
            "tapo|kasa|shelly|demo",
        );
        field(ui, "Device IP", &mut self.plug_ip, false, "192.168.0.17");
        field(ui, "Username", &mut self.plug_user, false, "env fallback");
        field(ui, "Password", &mut self.plug_pass, true, "env fallback");
        ui.horizontal(|ui| {
            if ui.button("Get stats").clicked() {
                self.spawn_plug(None);
            }
            if ui.button("Turn ON").clicked() {
                self.spawn_plug(Some(true));
            }
            let off = egui::Button::new("Turn OFF").fill(egui::Color32::DARK_RED);
            if ui.add(off).clicked() {
                self.spawn_plug(Some(false));
            }
        });
        ui.separator();
        egui::ScrollArea::vertical().show(ui, |ui| {
            for card in self.plug_cards.clone() {
                ui.group(|ui| value_rows(ui, &card));
            }
        });
    }

    fn tab_hue(&mut self, ui: &mut egui::Ui) {
        ui.heading("Hue lighting");
        field(ui, "Bridge IP", &mut self.hue_ip, false, "192.168.0.236");
        field(ui, "Username", &mut self.hue_user, false, "HUE_USERNAME");
        if ui.button("List lights").clicked() {
            self.spawn_hue_lights();
        }
        ui.separator();
        let lights = self.lights.clone();
        egui::ScrollArea::vertical().show(ui, |ui| {
            for lamp in &lights {
                let id = lamp.get("id").and_then(|v| v.as_str()).unwrap_or("?");
                let name = lamp.get("name").and_then(|v| v.as_str()).unwrap_or("?");
                let on = lamp.get("on").and_then(|v| v.as_bool()).unwrap_or(false);
                let bri = lamp.get("bri").and_then(|v| v.as_u64()).unwrap_or(0) as u8;
                ui.group(|ui| {
                    ui.horizontal(|ui| {
                        ui.heading(name);
                        let pill = if on { "ON" } else { "OFF" };
                        ui.label(pill);
                    });
                    ui.horizontal(|ui| {
                        if ui.button("On").clicked() {
                            self.spawn_hue_set(id.to_string(), true, None);
                        }
                        if ui.button("Off").clicked() {
                            self.spawn_hue_set(id.to_string(), false, None);
                        }
                        let pending_bri = *self.bri_pending.entry(id.to_string()).or_insert(bri);
                        let mut slider_value = pending_bri;
                        let slider =
                            ui.add(egui::Slider::new(&mut slider_value, 1..=254).text("bri"));
                        self.bri_pending.insert(id.to_string(), slider_value);
                        if slider.drag_stopped() {
                            let now = Instant::now();
                            let id_string = id.to_string();
                            let last = self.bri_sent_at.get(&id_string);
                            if last
                                .map(|t| now.duration_since(*t).as_secs() >= 1)
                                .unwrap_or(true)
                            {
                                self.bri_sent_at.insert(id_string, now);
                                self.spawn_hue_set(id.to_string(), true, Some(slider_value));
                            }
                        }
                    });
                });
            }
        });
    }

    fn tab_weather(&mut self, ui: &mut egui::Ui) {
        ui.heading("Weather (Open-Meteo, keyless — Vienna default)");
        field(ui, "Lat", &mut self.wx_lat, false, "48.2082");
        field(ui, "Lon", &mut self.wx_lon, false, "16.3738");
        if ui.button("Refresh").clicked() {
            let lat = self.wx_lat.trim().parse().ok();
            let lon = self.wx_lon.trim().parse().ok();
            self.spawn_weather(lat, lon);
        }
        ui.separator();
        if let Some(card) = self.wx_card.clone() {
            ui.group(|ui| value_rows(ui, &card));
        }
    }

    fn tab_cameras(&mut self, ui: &mut egui::Ui) {
        ui.heading("Camera snapshots (real frames)");
        field(
            ui,
            "Source",
            &mut self.cam_source,
            false,
            "usb|url|ring|tapo|onvif|demo",
        );
        field(ui, "Device IP / doorbell id", &mut self.cam_ip, false, "");
        field(ui, "URL", &mut self.cam_url, false, "https://…jpg");
        field(ui, "Model", &mut self.cam_model, false, "tapo: c220…");
        field(ui, "Username", &mut self.cam_user, false, "");
        field(ui, "Password", &mut self.cam_pass, true, "");
        field(ui, "USB index", &mut self.cam_index, false, "0");
        if ui.button("Snapshot").clicked() {
            let source = match CamSource::parse(&self.cam_source) {
                Ok(source) => source,
                Err(e) => {
                    self.note(e.to_string());
                    self.raw = serde_json::json!({"error": e.to_string()}).to_string();
                    return;
                }
            };
            let device_index = self.cam_index.trim().parse().ok();
            let onvif_port: Option<u16> = None;
            let label = format!("{} {}", source.as_str(), self.cam_ip.trim());
            self.spawn_snapshot(
                label,
                source,
                self.cam_ip.trim().to_string(),
                Self::opt(&self.cam_model),
                Self::opt(&self.cam_user),
                Self::opt(&self.cam_pass),
                onvif_port,
                device_index,
                Self::opt(&self.cam_url),
                Self::opt(&self.cam_token),
            );
        }
        ui.separator();
        egui::ScrollArea::vertical().show(ui, |ui| {
            for shot in &self.shots {
                ui.group(|ui| {
                    ui.heading(&shot.label);
                    value_rows(ui, &shot.meta);
                    if let Some(tex) = &shot.texture {
                        ui.add(egui::Image::new(tex).max_width(560.0));
                    }
                });
            }
        });
    }

    fn tab_ring(&mut self, ui: &mut egui::Ui) {
        ui.heading("Ring");
        field(ui, "Email", &mut self.ring_email, false, "");
        field(ui, "Password", &mut self.ring_pass, true, "");
        field(
            ui,
            "Token file",
            &mut self.ring_token,
            false,
            "ring_token.cache",
        );
        field(ui, "Doorbell id", &mut self.ring_id, false, "for snapshot");
        ui.horizontal(|ui| {
            if ui.button("Status").clicked() {
                self.spawn_ring(false);
            }
            if ui.button("Doorbells").clicked() {
                self.spawn_ring(true);
            }
            if ui.button("Doorbell snapshot").clicked() {
                let id = self.ring_id.trim().to_string();
                let label = format!("ring {id}");
                let (email, pass, token) = (
                    Self::opt(&self.ring_email),
                    Self::opt(&self.ring_pass),
                    Self::opt(&self.ring_token),
                );
                self.spawn_snapshot(
                    label,
                    CamSource::Ring,
                    id,
                    None,
                    email,
                    pass,
                    None,
                    None,
                    None,
                    token,
                );
                self.tab = Tab::Cameras;
            }
        });
        ui.separator();
        egui::ScrollArea::vertical().show(ui, |ui| {
            for card in self.ring_cards.clone() {
                ui.group(|ui| value_rows(ui, &card));
            }
        });
    }
}

/// Headless parity path for CI (`--smoke`): same dispatch, no window.
pub fn smoke() -> eframe::Result<()> {
    let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("smoke: runtime: {e}");
            std::process::exit(1);
        }
    };
    let report = rt.block_on(async {
        let status = dispatch(Command::Status).await;
        let stats = dispatch(Command::PlugStats {
            vendor: Vendor::Demo,
            device_ip: "192.168.1.50".to_string(),
            creds: Credentials::default(),
        })
        .await;
        let shot = camera::snapshot(
            CamSource::Demo,
            "",
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .await;
        (status, stats, shot)
    });
    let ok = report.0.get("app").is_some()
        && report.1.get("vendor").is_some()
        && report.2.map(|s| s.bytes > 0).unwrap_or(false);
    if ok {
        println!("smoke: status + demo plug + demo snapshot OK");
        Ok(())
    } else {
        eprintln!("smoke: parity check failed");
        std::process::exit(1);
    }
}
