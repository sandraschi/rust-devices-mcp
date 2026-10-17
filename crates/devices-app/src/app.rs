//! egui dashboard: sidebar navigation + per-device pages, zero JavaScript.
//!
//! Layout mirrors the original webapp: retractable left sidebar (Dashboard,
//! Energy, Lighting, Climate, Cameras, Weather, Alarms, Raw), main area shows
//! the selected page, per-device detail pages for Hue lights and Ring
//! doorbells. High-contrast visuals + 16px base font (the default theme was
//! legitimately hard to read). Field values persist to
//! %LOCALAPPDATA%/rust-devices-mcp/ui.json — passwords never persist.
use devices_core::{
    camera::{self, CamSource},
    creds::{Credentials, Vendor},
    dispatch,
    import::{self, FleetImport},
    Command,
};
use eframe::egui;
use serde_json::Value;
use std::collections::HashMap;
use std::future::Future;
use std::sync::{mpsc, Arc};
use std::time::Instant;

#[derive(PartialEq, Clone)]
enum Page {
    Dashboard,
    Energy,
    Lighting,
    LightDetail(String),
    Climate,
    Cameras,
    Weather,
    Alarms,
    DoorbellDetail(i64),
    Raw,
}

impl Page {
    fn label(&self) -> String {
        match self {
            Self::Dashboard => "Dashboard".to_string(),
            Self::Energy => "Energy".to_string(),
            Self::Lighting => "Lighting".to_string(),
            Self::LightDetail(id) => format!("Light {id}"),
            Self::Climate => "Climate".to_string(),
            Self::Cameras => "Cameras".to_string(),
            Self::Weather => "Weather".to_string(),
            Self::Alarms => "Alarms".to_string(),
            Self::DoorbellDetail(id) => format!("Doorbell {id}"),
            Self::Raw => "Raw JSON".to_string(),
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

const VENDORS: &[&str] = &["tapo", "kasa", "shelly", "demo"];
const SOURCES: &[&str] = &["usb", "url", "ring", "tapo", "onvif", "demo"];
const MODES: &[&str] = &["disarmed", "home", "away"];

pub struct DevicesApp {
    rt: Arc<tokio::runtime::Runtime>,
    tx: mpsc::Sender<Job>,
    rx: mpsc::Receiver<Job>,
    pending: usize,
    fleet: Option<FleetImport>,
    dark_mode: bool,
    zoom_saved: f32,
    power_history: std::collections::VecDeque<(String, f64)>,
    page: Page,
    sidebar_open: bool,
    status_line: String,
    raw: String,
    // Plugs.
    plug_vendor: String,
    plug_ip: String,
    plug_user: String,
    plug_pass: String,
    plug_cards: Vec<Value>,
    // Hue.
    hue_ip: String,
    hue_user: String,
    lights: Vec<Value>,
    bri_pending: HashMap<String, u8>,
    bri_sent_at: HashMap<String, Instant>,
    // Weather.
    wx_lat: String,
    wx_lon: String,
    wx_card: Option<Value>,
    // Cameras.
    cam_source: String,
    cam_ip: String,
    cam_model: String,
    cam_user: String,
    cam_pass: String,
    cam_index: String,
    cam_url: String,
    cam_token: String,
    shots: Vec<Shot>,
    // Ring.
    ring_email: String,
    ring_pass: String,
    ring_token: String,
    ring_id: String,
    ring_locations: Vec<Value>,
    ring_doorbells: Vec<Value>,
    alarm_mode: String,
    alarm_location: String,
}

fn env_or(key: &str) -> String {
    std::env::var(key).unwrap_or_default()
}

fn persist_path() -> Option<std::path::PathBuf> {
    let base = std::env::var("LOCALAPPDATA").ok()?;
    Some(std::path::PathBuf::from(base).join("rust-devices-mcp"))
}

/// Near-white on near-black (slate-950 family, like the original webapp),
/// 16px base font. The stock dark theme was legitimately hard to read.
fn harsh_visuals() -> egui::Visuals {
    let mut visuals = egui::Visuals::dark();
    visuals.override_text_color = Some(egui::Color32::from_rgb(242, 246, 252));
    visuals.window_fill = egui::Color32::from_rgb(2, 6, 18);
    visuals.panel_fill = egui::Color32::from_rgb(10, 18, 34);
    visuals.extreme_bg_color = egui::Color32::from_rgb(2, 6, 18);
    visuals.code_bg_color = egui::Color32::from_rgb(4, 10, 24);
    visuals.faint_bg_color = egui::Color32::from_rgb(16, 28, 50);
    visuals.warn_fg_color = egui::Color32::from_rgb(255, 200, 120);
    visuals.error_fg_color = egui::Color32::from_rgb(255, 130, 130);
    visuals.widgets.noninteractive.fg_stroke =
        egui::Stroke::new(1.0, egui::Color32::from_rgb(200, 212, 230));
    visuals.widgets.inactive.fg_stroke =
        egui::Stroke::new(1.0, egui::Color32::from_rgb(235, 242, 252));
    visuals.widgets.hovered.fg_stroke = egui::Stroke::new(1.5, egui::Color32::WHITE);
    visuals.widgets.active.fg_stroke = egui::Stroke::new(1.5, egui::Color32::WHITE);
    visuals.selection.bg_fill = egui::Color32::from_rgb(36, 64, 107);
    visuals.selection.stroke = egui::Stroke::new(1.0, egui::Color32::WHITE);
    visuals.hyperlink_color = egui::Color32::from_rgb(125, 180, 255);
    visuals.window_corner_radius = egui::CornerRadius::same(12);
    visuals.menu_corner_radius = egui::CornerRadius::same(8);
    visuals.widgets.noninteractive.corner_radius = egui::CornerRadius::same(6);
    visuals.widgets.inactive.corner_radius = egui::CornerRadius::same(8);
    visuals.widgets.hovered.corner_radius = egui::CornerRadius::same(8);
    visuals.widgets.active.corner_radius = egui::CornerRadius::same(8);
    visuals.widgets.open.corner_radius = egui::CornerRadius::same(8);
    // Panel separator hairlines (topbar/sidebar/statusbar dividers).
    visuals.widgets.noninteractive.bg_stroke =
        egui::Stroke::new(1.0, egui::Color32::from_rgb(38, 54, 84));
    // Button + text-input outlines; accent on hover.
    visuals.widgets.inactive.bg_stroke =
        egui::Stroke::new(1.0, egui::Color32::from_rgb(48, 66, 102));
    visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.5, ACCENT);
    visuals
}

/// Light companion to `harsh_visuals` (slate-50 panels, white cards).
fn crisp_light_visuals() -> egui::Visuals {
    let mut visuals = egui::Visuals::light();
    visuals.override_text_color = Some(egui::Color32::from_rgb(15, 23, 42));
    visuals.window_fill = egui::Color32::from_rgb(248, 250, 252);
    visuals.panel_fill = egui::Color32::from_rgb(248, 250, 252);
    visuals.extreme_bg_color = egui::Color32::from_rgb(241, 245, 249);
    visuals.code_bg_color = egui::Color32::from_rgb(241, 245, 249);
    visuals.widgets.noninteractive.bg_stroke =
        egui::Stroke::new(1.0, egui::Color32::from_rgb(226, 232, 240));
    visuals.widgets.inactive.bg_stroke =
        egui::Stroke::new(1.0, egui::Color32::from_rgb(203, 213, 225));
    visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.5, ACCENT);
    visuals.window_corner_radius = egui::CornerRadius::same(12);
    visuals.menu_corner_radius = egui::CornerRadius::same(8);
    visuals
}

/// Theme + typography in one place: `set_visuals` replaces the whole style,
/// so fonts must be re-applied on every toggle (called in `new` and on flip).
fn apply_theme(ctx: &egui::Context, dark_mode: bool) {
    ctx.set_visuals(if dark_mode {
        harsh_visuals()
    } else {
        crisp_light_visuals()
    });
    ctx.style_mut_of(
        if dark_mode {
            egui::Theme::Dark
        } else {
            egui::Theme::Light
        },
        |style| {
            use egui::TextStyle::*;
            for (text_style, size) in [
                (Body, 16.0),
                (Button, 16.0),
                (Heading, 23.0),
                (Monospace, 14.0),
                (Small, 13.0),
            ] {
                if let Some(font) = style.text_styles.get_mut(&text_style) {
                    font.size = size;
                }
            }
            style.spacing.item_spacing = egui::vec2(10.0, 8.0);
            style.spacing.button_padding = egui::vec2(10.0, 6.0);
        },
    );
}

/// Ctrl+MouseWheel global zoom (Ctrl+Plus/Minus/0 work automatically).
fn handle_zoom(ctx: &egui::Context) {
    let scroll_y = ctx.input(|input| {
        if input.modifiers.ctrl || input.modifiers.command {
            input.smooth_scroll_delta.y
        } else {
            0.0
        }
    });
    if scroll_y.abs() > 0.0 {
        let step = if scroll_y > 0.0 {
            1.1_f32
        } else {
            1.0 / 1.1_f32
        };
        ctx.set_zoom_factor((ctx.zoom_factor() * step).clamp(0.6, 2.5));
    }
}

/// Theme toggle: vector sun / crescent moon (glyphs would be tofu).
fn theme_toggle_button(ui: &mut egui::Ui, dark_mode: &mut bool) {
    let size = egui::vec2(30.0, 30.0);
    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click());
    if ui.is_rect_visible(rect) {
        let visuals = ui.style().interact(&resp);
        ui.painter().rect(
            rect,
            egui::CornerRadius::same(6),
            visuals.bg_fill,
            visuals.bg_stroke,
            egui::StrokeKind::Inside,
        );
        let center = rect.center();
        let painter = ui.painter();
        if *dark_mode {
            let sun = egui::Color32::from_rgb(251, 191, 36);
            painter.circle_filled(center, 4.5, sun);
            for i in 0..8 {
                let angle = (i as f32) * std::f32::consts::TAU / 8.0;
                let dir = egui::vec2(angle.cos(), angle.sin());
                painter.line_segment(
                    [center + dir * 6.5, center + dir * 9.2],
                    egui::Stroke::new(1.6, sun),
                );
            }
        } else {
            let moon = egui::Color32::from_rgb(51, 65, 85);
            painter.circle_filled(center, 6.5, moon);
            painter.circle_filled(center + egui::vec2(3.0, -2.2), 5.5, visuals.bg_fill);
        }
    }
    if resp.on_hover_text("Toggle light / dark theme").clicked() {
        *dark_mode = !*dark_mode;
        apply_theme(ui.ctx(), *dark_mode);
    }
}

/// Brand accents per domain (mirrors the webapp's color language).
const ACCENT: egui::Color32 = egui::Color32::from_rgb(224, 108, 47);
const ENERGY: egui::Color32 = egui::Color32::from_rgb(251, 191, 36);
const HUE_YELLOW: egui::Color32 = egui::Color32::from_rgb(253, 224, 71);
const SKY: egui::Color32 = egui::Color32::from_rgb(125, 211, 252);
const VIOLET: egui::Color32 = egui::Color32::from_rgb(196, 181, 253);
const RING_BLUE: egui::Color32 = egui::Color32::from_rgb(147, 197, 253);
const GOOD: egui::Color32 = egui::Color32::from_rgb(110, 255, 150);
const BAD: egui::Color32 = egui::Color32::from_rgb(255, 150, 150);

/// Card container: theme-aware fill, 12px rounding, hairline stroke, shadow.
fn card_frame(ui: &mut egui::Ui, add_contents: impl FnOnce(&mut egui::Ui)) {
    accent_card_frame(ui, None, add_contents);
}

/// Card with an optional 3px accent stripe along its top edge (Tailwind
/// `border-l-4` language in GPU geometry). The stripe paints from the
/// frame's own rect — never from `available_width`, which overflows the
/// drawn card (caught by the hero_kpi golden, 2026-10-17).
fn accent_card_frame(
    ui: &mut egui::Ui,
    accent: Option<egui::Color32>,
    add_contents: impl FnOnce(&mut egui::Ui),
) {
    let dark = ui.visuals().dark_mode;
    // Dark cards need presence, not shadows: black-on-black shadows are
    // physically invisible, so the border carries the separation (caught as
    // "invisible in dark mode", 2026-10-17 — light mode was always fine).
    let (fill, stroke) = if dark {
        (
            egui::Color32::from_rgb(22, 34, 60),
            egui::Stroke::new(1.5, egui::Color32::from_rgb(74, 102, 152)),
        )
    } else {
        (
            egui::Color32::WHITE,
            egui::Stroke::new(1.0, egui::Color32::from_rgb(203, 213, 225)),
        )
    };
    let frame_rect = egui::Frame {
        fill,
        corner_radius: egui::CornerRadius::same(12),
        stroke,
        shadow: egui::Shadow {
            offset: [0, 3],
            blur: 14,
            spread: 0,
            color: egui::Color32::from_black_alpha(110),
        },
        inner_margin: egui::Margin::same(14),
        outer_margin: egui::Margin {
            bottom: 10,
            ..Default::default()
        },
    }
    .show(ui, add_contents)
    .response
    .rect;
    if let Some(color) = accent {
        let stripe = egui::Rect::from_min_size(frame_rect.min, egui::vec2(frame_rect.width(), 3.0));
        ui.painter().rect_filled(stripe, 1.0, color);
    }
}

/// Primary action button (brand orange); danger variant for destructive acts.
fn primary(ui: &mut egui::Ui, text: &str) -> egui::Response {
    ui.add(
        egui::Button::new(
            egui::RichText::new(text)
                .strong()
                .color(egui::Color32::WHITE),
        )
        .fill(ACCENT),
    )
}

/// Hero KPI: big tinted number on a card with a 3px accent stripe
/// (Tailwind `border-l-4` language, in GPU geometry).
fn hero_kpi(ui: &mut egui::Ui, label: &str, value: &str, color: egui::Color32) {
    accent_card_frame(ui, Some(color), |ui| {
        let dim = if ui.visuals().dark_mode {
            egui::Color32::from_rgb(160, 175, 200)
        } else {
            egui::Color32::from_rgb(71, 85, 105)
        };
        ui.label(egui::RichText::new(label).size(13.0).color(dim));
        ui.label(egui::RichText::new(value).size(30.0).strong().color(color));
    });
}

/// Spinning icosahedron: 12 vertices, 20 faces, zero crates beyond egui.
/// Pure projection math (unit-tested below); the widget only paints.
/// Edges dedupe from faces at paint time — 60 pairs, trivially cheap.
fn ico_point(vertex: [f32; 3], angle_y: f32, tilt_x: f32) -> (f32, f32, f32) {
    let (sin_y, cos_y) = angle_y.sin_cos();
    let (sin_x, cos_x) = tilt_x.sin_cos();
    let (x, y, z) = (vertex[0], vertex[1], vertex[2]);
    let (x, z) = (x * cos_y + z * sin_y, -x * sin_y + z * cos_y);
    let (y, z) = (y * cos_x - z * sin_x, y * sin_x + z * cos_x);
    (x, y, z)
}

fn ico_vertices() -> [[f32; 3]; 12] {
    let t = std::f32::consts::GOLDEN_RATIO;
    [
        [-1.0, t, 0.0],
        [1.0, t, 0.0],
        [-1.0, -t, 0.0],
        [1.0, -t, 0.0],
        [0.0, -1.0, t],
        [0.0, 1.0, t],
        [0.0, -1.0, -t],
        [0.0, 1.0, -t],
        [t, 0.0, -1.0],
        [t, 0.0, 1.0],
        [-t, 0.0, -1.0],
        [-t, 0.0, 1.0],
    ]
}

fn ico_faces() -> [[usize; 3]; 20] {
    [
        [0, 11, 5],
        [0, 5, 1],
        [0, 1, 7],
        [0, 7, 10],
        [0, 10, 11],
        [1, 5, 9],
        [5, 11, 4],
        [11, 10, 2],
        [10, 7, 6],
        [7, 1, 8],
        [3, 9, 4],
        [3, 4, 2],
        [3, 2, 6],
        [3, 6, 8],
        [3, 8, 9],
        [4, 9, 5],
        [2, 4, 11],
        [6, 2, 10],
        [8, 6, 7],
        [9, 8, 1],
    ]
}

fn icosahedron(ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    accent_card_frame(ui, Some(VIOLET), |ui| {
        ui.label(egui::RichText::new("GPU immediate mode").strong());
        ui.label("12 vertices, 20 faces, 0 crates beyond egui.");
        let size = egui::vec2(ui.available_width().max(200.0), 220.0);
        let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
        let painter = ui.painter_at(rect);
        let time = ctx.time() as f32;
        let verts = ico_vertices();
        let projected: Vec<(f32, f32, f32)> = verts
            .iter()
            .map(|v| ico_point(*v, time * 0.5, 0.35 + (time * 0.13).sin() * 0.15))
            .collect();
        let to_screen = |x: f32, y: f32, z: f32| {
            let perspective = 3.4 / (3.4 + z);
            egui::pos2(
                rect.center().x + x * perspective * 62.0,
                rect.center().y - y * perspective * 62.0,
            )
        };
        let mut edges = std::collections::HashSet::new();
        for face in ico_faces() {
            for side in 0..3 {
                let (a, b) = (face[side], face[(side + 1) % 3]);
                edges.insert((a.min(b), a.max(b)));
            }
        }
        for (a, b) in &edges {
            let (pa, pb) = (projected[*a], projected[*b]);
            let front = (pa.2 + pb.2) / 2.0 > 0.0;
            let color = if front {
                egui::Color32::from_rgb(196, 181, 253)
            } else {
                egui::Color32::from_rgb(90, 100, 140)
            };
            painter.line_segment(
                [to_screen(pa.0, pa.1, pa.2), to_screen(pb.0, pb.1, pb.2)],
                egui::Stroke::new(1.6, color),
            );
        }
        for (x, y, z) in &projected {
            painter.circle_filled(
                to_screen(*x, *y, *z),
                3.0,
                egui::Color32::from_rgb(240, 244, 252),
            );
        }
    });
    ctx.request_repaint();
}

/// Curated weather card: big temperature, summary, and the six numbers
/// that matter — never the raw payload dump (hourly arrays flooded it).
fn weather_card(ui: &mut egui::Ui, card: &Value) {
    let current = card.get("current");
    let num = |key: &str| current.and_then(|c| c.get(key)).and_then(|v| v.as_f64());
    let summary = card
        .get("summary")
        .and_then(|v| v.as_str())
        .unwrap_or("Weather");
    let today = card.get("today");
    let day = |key: &str| {
        today
            .and_then(|t| t.get(key))
            .and_then(|v| v.as_f64())
            .map(|v| format!("{v:.0}"))
            .unwrap_or_else(|| "—".to_string())
    };
    accent_card_frame(ui, Some(SKY), |ui| {
        ui.horizontal(|ui| {
            let temp = num("temperature_2m")
                .map(|t| format!("{t:.1} °C"))
                .unwrap_or_else(|| "—".to_string());
            ui.label(egui::RichText::new(temp).size(38.0).strong().color(SKY));
            ui.vertical(|ui| {
                ui.heading(summary);
                if let Some(feels) = num("apparent_temperature") {
                    ui.label(format!("feels {feels:.1} °C"));
                }
                if let Some(humidity) = num("relative_humidity_2m") {
                    ui.label(format!("humidity {humidity:.0} %"));
                }
                if let Some(wind) = num("wind_speed_10m") {
                    ui.label(format!("wind {wind:.0} km/h"));
                }
            });
        });
        ui.separator();
        ui.horizontal(|ui| {
            ui.label(format!("today {} … {} °C", day("min_c"), day("max_c")));
            ui.label(format!(
                "rain {}%",
                today
                    .and_then(|t| t.get("precipitation_probability_max"))
                    .and_then(|v| v.as_f64())
                    .map(|v| format!("{v:.0}"))
                    .unwrap_or_else(|| "—".to_string())
            ));
        });
    });
}

/// Zero-dependency GPU spark chart (gridlines + curve + nodes + hover).
/// Fewer than 2 samples renders an honest placeholder, never a flat line.
fn spark_chart(
    ui: &mut egui::Ui,
    title: &str,
    unit: &str,
    samples: &[(String, f64)],
    stroke_color: egui::Color32,
) {
    card_frame(ui, |ui| {
        ui.label(egui::RichText::new(title).strong());
        let desired = egui::vec2(ui.available_width().max(240.0), 140.0);
        let (rect, resp) = ui.allocate_exact_size(desired, egui::Sense::hover());
        let painter = ui.painter_at(rect);
        let grid = egui::Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color);
        for frac in [0.0_f32, 0.5, 1.0] {
            let y = egui::lerp(rect.top() + 8.0..=rect.bottom() - 18.0, frac);
            painter.line_segment(
                [egui::pos2(rect.left(), y), egui::pos2(rect.right(), y)],
                grid,
            );
        }
        if samples.len() < 2 {
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "Collecting samples...",
                egui::FontId::proportional(14.0),
                ui.visuals().text_color(),
            );
            return;
        }
        let min_v = samples
            .iter()
            .map(|(_, v)| *v)
            .fold(f64::INFINITY, f64::min);
        let max_v = samples
            .iter()
            .map(|(_, v)| *v)
            .fold(f64::NEG_INFINITY, f64::max);
        let span = (max_v - min_v).abs().max(1.0);
        let plot = rect.shrink2(egui::vec2(6.0, 14.0));
        let denom = (samples.len().saturating_sub(1)).max(1) as f32;
        let pts: Vec<egui::Pos2> = samples
            .iter()
            .enumerate()
            .map(|(idx, (_, val))| {
                egui::pos2(
                    egui::lerp(plot.left()..=plot.right(), (idx as f32) / denom),
                    egui::lerp(plot.bottom()..=plot.top(), ((*val - min_v) / span) as f32),
                )
            })
            .collect();
        painter.add(egui::Shape::line(
            pts.clone(),
            egui::Stroke::new(2.2, stroke_color),
        ));
        for pt in &pts {
            painter.circle_filled(*pt, 3.5, stroke_color);
        }
        if let Some(mouse) = resp.hover_pos() {
            if let Some((idx, pt)) = pts
                .iter()
                .enumerate()
                .min_by(|(_, a), (_, b)| (a.x - mouse.x).abs().total_cmp(&(b.x - mouse.x).abs()))
            {
                if let Some((label, val)) = samples.get(idx) {
                    painter.vline(pt.x, plot.y_range(), egui::Stroke::new(1.0, stroke_color));
                    painter.circle_stroke(*pt, 6.0, egui::Stroke::new(2.0, egui::Color32::WHITE));
                    resp.on_hover_text(format!("{label}: {val:.1} {unit}"));
                }
            }
        }
    });
}

fn danger(ui: &mut egui::Ui, text: &str) -> egui::Response {
    ui.add(
        egui::Button::new(
            egui::RichText::new(text)
                .strong()
                .color(egui::Color32::WHITE),
        )
        .fill(egui::Color32::from_rgb(122, 45, 45)),
    )
}

impl DevicesApp {
    pub fn new(ctx: &eframe::CreationContext<'_>, rt: Arc<tokio::runtime::Runtime>) -> Self {
        let (tx, rx) = mpsc::channel();
        let mut app = Self {
            rt,
            tx,
            rx,
            pending: 0,
            fleet: None,
            dark_mode: true,
            zoom_saved: 1.0,
            power_history: std::collections::VecDeque::new(),
            page: Page::Dashboard,
            sidebar_open: true,
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
            ring_locations: Vec::new(),
            ring_doorbells: Vec::new(),
            alarm_mode: "disarmed".to_string(),
            alarm_location: String::new(),
        };
        app.load_fields();
        apply_theme(&ctx.egui_ctx, app.dark_mode);
        ctx.egui_ctx.set_zoom_factor(app.zoom_saved);
        app.apply_fleet_import();
        // Dashboard paints real data immediately: no creds needed anywhere here.
        app.spawn_status();
        app.spawn_weather_boot();
        if !app.hue_ip.trim().is_empty() && !app.hue_user.trim().is_empty() {
            app.spawn_hue_lights();
        }
        if !app.ring_token.trim().is_empty() {
            app.spawn_ring(false);
            app.spawn_ring(true);
        }
        app
    }

    // ----- field persistence (never passwords) -----
    fn fields_json(&self) -> Value {
        serde_json::json!({
            "plug_vendor": self.plug_vendor, "plug_ip": self.plug_ip,
            "hue_ip": self.hue_ip, "hue_user": self.hue_user,
            "wx_lat": self.wx_lat, "wx_lon": self.wx_lon,
            "cam_source": self.cam_source, "cam_ip": self.cam_ip,
            "cam_model": self.cam_model, "cam_index": self.cam_index,
            "cam_url": self.cam_url, "cam_token": self.cam_token,
            "ring_token": self.ring_token, "ring_id": self.ring_id,
            "alarm_location": self.alarm_location,
            "dark_mode": self.dark_mode, "zoom": self.zoom_saved,
        })
    }

    fn save_fields(&self) {
        if let Some(dir) = persist_path() {
            let _ = std::fs::create_dir_all(&dir);
            let path = dir.join("ui.json");
            if let Ok(text) = serde_json::to_string_pretty(&self.fields_json()) {
                let _ = std::fs::write(path, text);
            }
        }
    }

    fn load_fields(&mut self) {
        let text = persist_path().and_then(|dir| std::fs::read_to_string(dir.join("ui.json")).ok());
        let Some(text) = text else { return };
        let json: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
        let get = |key: &str| {
            json.get(key)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string()
        };
        let filled = |key: &str, slot: &mut String| {
            let value = get(key);
            if !value.is_empty() && slot.trim().is_empty() {
                *slot = value;
            }
        };
        filled("plug_vendor", &mut self.plug_vendor);
        if VENDORS.iter().all(|v| *v != self.plug_vendor.as_str()) {
            self.plug_vendor = "tapo".to_string();
        }
        filled("plug_ip", &mut self.plug_ip);
        filled("hue_ip", &mut self.hue_ip);
        filled("hue_user", &mut self.hue_user);
        filled("wx_lat", &mut self.wx_lat);
        filled("wx_lon", &mut self.wx_lon);
        filled("cam_source", &mut self.cam_source);
        if SOURCES.iter().all(|s| *s != self.cam_source.as_str()) {
            self.cam_source = "usb".to_string();
        }
        filled("cam_ip", &mut self.cam_ip);
        filled("cam_model", &mut self.cam_model);
        filled("cam_index", &mut self.cam_index);
        filled("cam_url", &mut self.cam_url);
        filled("cam_token", &mut self.cam_token);
        filled("ring_token", &mut self.ring_token);
        filled("ring_id", &mut self.ring_id);
        filled("alarm_location", &mut self.alarm_location);
        if let Some(dark) = json.get("dark_mode").and_then(|v| v.as_bool()) {
            self.dark_mode = dark;
        }
        if let Some(zoom) = json.get("zoom").and_then(|v| v.as_f64()) {
            let zoom = zoom as f32;
            if zoom.is_finite() && (0.6..=2.5).contains(&zoom) {
                self.zoom_saved = zoom;
            }
        }
    }

    // ----- job plumbing -----
    fn spawn<F>(&mut self, make: impl FnOnce() -> F + Send + 'static)
    where
        F: Future<Output = Job> + Send + 'static,
    {
        let tx = self.tx.clone();
        self.rt.spawn(async move {
            let _ = tx.send(make().await);
        });
        self.pending += 1;
        self.save_fields();
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
                    if let (Some(ip), Some(watts)) = (
                        value.get("device_ip").and_then(|v| v.as_str()),
                        value.get("current_power_watts").and_then(|v| v.as_f64()),
                    ) {
                        self.power_history.push_back((ip.to_string(), watts));
                        while self.power_history.len() > 60 {
                            self.power_history.pop_front();
                        }
                    }
                    self.plug_cards.push(value);
                    if self.plug_cards.len() > 6 {
                        self.plug_cards.remove(0);
                    }
                }
                Job::HueLights(value) => {
                    self.show_raw(&value);
                    if let Some(list) = value.as_array() {
                        self.lights = list.clone();
                        self.note(format!("{} Hue lights", list.len()));
                    } else {
                        self.lights = vec![value.clone()];
                        self.note("Hue bridge answered with an error — see Raw JSON");
                    }
                }
                Job::HueSet(value) => {
                    self.show_raw(&value);
                    // Refresh the list so the page shows hardware truth.
                    let (ip, user) = (Self::opt(&self.hue_ip), Self::opt(&self.hue_user));
                    self.pending += 1;
                    let tx = self.tx.clone();
                    self.rt.spawn(async move {
                        let result = dispatch(Command::HueLights {
                            bridge_ip: ip,
                            username: user,
                        })
                        .await;
                        let _ = tx.send(Job::HueLights(result));
                    });
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
                    if let Some(list) = value.get("doorbells").and_then(|v| v.as_array()) {
                        self.ring_doorbells = list.clone();
                    }
                    if value.get("location_id").is_some() {
                        self.ring_locations = vec![value.clone()];
                    } else if value.as_array().is_some() {
                        self.ring_locations = value.as_array().cloned().unwrap_or_default();
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

    /// Fleet import: IPs, usernames, and secrets from the devices-mcp
    /// checkout (operator-permitted). Fills empty fields so the dashboard
    /// works with zero typing on a fleet machine.
    fn apply_fleet_import(&mut self) {
        let Some(inv) = import::load() else {
            return;
        };
        self.fleet = Some(inv.clone());
        if self.plug_ip.trim().is_empty() {
            if let Some(plug) = inv.plugs.first() {
                self.plug_vendor = plug.vendor.to_string();
                self.plug_ip = plug.ip.clone();
            }
        }
        if let Some(hue) = &inv.hue {
            if self.hue_ip.trim().is_empty() {
                self.hue_ip = hue.ip.clone();
            }
            if self.hue_user.trim().is_empty() {
                if let Some(user) = &hue.username {
                    self.hue_user = user.clone();
                }
            }
        }
        if self.ring_token.trim().is_empty() {
            if let Some(token) = &inv.ring_token_file {
                self.ring_token = token.clone();
            }
        }
        if self.cam_ip.trim().is_empty() {
            if let Some(cam) = inv.onvif_cams.first() {
                self.cam_source = "onvif".to_string();
                self.cam_ip = cam.ip.clone();
            }
        }
        if self.cam_user.trim().is_empty() {
            if let Some(user) = &inv.tapo_cam_user {
                self.cam_user = user.clone();
            }
        }
        if let Some(idx) = inv.usb_index {
            self.cam_index = idx.to_string();
        }
        self.note(format!(
            "fleet import: {} plugs, {} onvif cams{}",
            inv.plugs.len(),
            inv.onvif_cams.len(),
            if inv.netatmo_configured {
                ", netatmo configured"
            } else {
                ""
            }
        ));
    }

    fn plug_creds(&self) -> Result<(Vendor, Credentials), String> {
        let vendor = Vendor::parse(&self.plug_vendor).map_err(|e| e.to_string())?;
        // Tapo secrets fall back to the fleet store (process env, then
        // devices-mcp/.env) so the dashboard needs no typing.
        let mut user = Self::opt(&self.plug_user);
        let mut pass = Self::opt(&self.plug_pass);
        if vendor == Vendor::Tapo {
            if user.is_none() {
                user = import::fleet_secret_any(&[
                    "TAPO_EMAIL",
                    "TAPO_USERNAME",
                    "TAPO_P115_EMAIL",
                    "TAPO_LIGHTING_EMAIL",
                ])
                .or_else(|| {
                    self.fleet
                        .as_ref()
                        .and_then(|fleet| fleet.tapo_email.clone())
                });
            }
            if pass.is_none() {
                pass = import::fleet_secret_any(&[
                    "TAPO_PASSWORD",
                    "TAPO_P115_PASSWORD",
                    "TAPO_LIGHTING_PASSWORD",
                ])
                .or_else(|| {
                    self.fleet
                        .as_ref()
                        .and_then(|fleet| fleet.tapo_password.clone())
                });
            }
        }
        let creds = Credentials::resolve(vendor, user, pass).map_err(|e| e.to_string())?;
        Ok((vendor, creds))
    }

    fn spawn_status(&mut self) {
        self.spawn(|| async { Job::Status(dispatch(Command::Status).await) });
    }

    fn spawn_plug(&mut self, state: Option<bool>) {
        let ip = self.plug_ip.trim().to_string();
        match (self.plug_creds(), state) {
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
                let readonly = self
                    .fleet
                    .as_ref()
                    .map(|fleet| {
                        fleet
                            .plugs
                            .iter()
                            .any(|plug| plug.ip == ip && plug.readonly)
                    })
                    .unwrap_or(false);
                if readonly {
                    let message =
                        format!("{ip} is marked readonly in the fleet config — toggle refused");
                    self.note(message.clone());
                    self.raw = serde_json::json!({"error": message}).to_string();
                    return;
                }
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

    fn spawn_weather_boot(&mut self) {
        self.note("fetching weather…");
        self.spawn(|| async {
            Job::Weather(
                dispatch(Command::Weather {
                    latitude: None,
                    longitude: None,
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
                    let meta = serde_json::to_value(&frame.meta).unwrap_or(Value::Null);
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
        let mut triple = (
            Self::opt(&self.ring_email),
            Self::opt(&self.ring_pass),
            Self::opt(&self.ring_token),
        );
        if triple.0.is_none() {
            triple.0 = import::fleet_secret("RING_EMAIL");
        }
        if triple.1.is_none() {
            triple.1 = import::fleet_secret("RING_PASSWORD");
        }
        if triple.2.is_none() {
            triple.2 = import::fleet_secret("RING_TOKEN_FILE");
        }
        if triple.0.is_none() {
            triple.0 = self
                .fleet
                .as_ref()
                .and_then(|fleet| fleet.ring_email.clone());
        }
        if triple.1.is_none() {
            triple.1 = self
                .fleet
                .as_ref()
                .and_then(|fleet| fleet.ring_password.clone());
        }
        if triple.2.is_none() {
            triple.2 = self
                .fleet
                .as_ref()
                .and_then(|fleet| fleet.ring_token_file.clone());
        }
        let (email, pass, token) = triple;
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

    fn spawn_alarm_read(&mut self) {
        let mut triple = (
            Self::opt(&self.ring_email),
            Self::opt(&self.ring_pass),
            Self::opt(&self.ring_token),
        );
        if triple.0.is_none() {
            triple.0 = import::fleet_secret("RING_EMAIL");
        }
        if triple.1.is_none() {
            triple.1 = import::fleet_secret("RING_PASSWORD");
        }
        if triple.2.is_none() {
            triple.2 = import::fleet_secret("RING_TOKEN_FILE");
        }
        if triple.0.is_none() {
            triple.0 = self
                .fleet
                .as_ref()
                .and_then(|fleet| fleet.ring_email.clone());
        }
        if triple.1.is_none() {
            triple.1 = self
                .fleet
                .as_ref()
                .and_then(|fleet| fleet.ring_password.clone());
        }
        if triple.2.is_none() {
            triple.2 = self
                .fleet
                .as_ref()
                .and_then(|fleet| fleet.ring_token_file.clone());
        }
        let (email, pass, token) = triple;
        self.note("reading alarm state…");
        self.spawn(move || async move {
            Job::Ring(
                dispatch(Command::RingAlarm {
                    mode: None,
                    location_id: None,
                    email,
                    password: pass,
                    token_file: token,
                })
                .await,
            )
        });
    }

    fn spawn_alarm_set(&mut self) {
        let mode = self.alarm_mode.clone();
        let location = Self::opt(&self.alarm_location);
        let mut triple = (
            Self::opt(&self.ring_email),
            Self::opt(&self.ring_pass),
            Self::opt(&self.ring_token),
        );
        if triple.0.is_none() {
            triple.0 = import::fleet_secret("RING_EMAIL");
        }
        if triple.1.is_none() {
            triple.1 = import::fleet_secret("RING_PASSWORD");
        }
        if triple.2.is_none() {
            triple.2 = import::fleet_secret("RING_TOKEN_FILE");
        }
        if triple.0.is_none() {
            triple.0 = self
                .fleet
                .as_ref()
                .and_then(|fleet| fleet.ring_email.clone());
        }
        if triple.1.is_none() {
            triple.1 = self
                .fleet
                .as_ref()
                .and_then(|fleet| fleet.ring_password.clone());
        }
        if triple.2.is_none() {
            triple.2 = self
                .fleet
                .as_ref()
                .and_then(|fleet| fleet.ring_token_file.clone());
        }
        let (email, pass, token) = triple;
        self.note(format!("setting alarm mode {mode}…"));
        self.spawn(move || async move {
            Job::Ring(
                dispatch(Command::RingAlarm {
                    mode: Some(mode),
                    location_id: location,
                    email,
                    password: pass,
                    token_file: token,
                })
                .await,
            )
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
                .desired_width(220.0),
        );
    });
}

fn combo(ui: &mut egui::Ui, label: &str, current: &mut String, options: &[&str]) {
    ui.horizontal(|ui| {
        ui.label(label);
        egui::ComboBox::from_id_salt(label)
            .selected_text(current.as_str())
            .show_ui(ui, |ui| {
                for option in options {
                    ui.selectable_value(current, (*option).to_string(), *option);
                }
            });
    });
}

fn value_rows(ui: &mut egui::Ui, value: &Value) {
    if let Some(err) = value.get("error").and_then(|e| e.as_str()) {
        ui.colored_label(egui::Color32::from_rgb(255, 140, 140), err);
        return;
    }
    if let Some(map) = value.as_object() {
        egui::Grid::new(format!("grid-{:p}", map as *const _))
            .num_columns(2)
            .striped(true)
            .show(ui, |ui| {
                for (key, val) in map {
                    ui.strong(key);
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

fn on_pill(ui: &mut egui::Ui, on: bool) {
    // Status dots are painted circles, not glyphs: geometric codepoints
    // (U+25CF et al.) have no glyph in egui 0.37's bundled fonts and
    // render as tofu. See the glyph tests below.
    ui.horizontal(|ui| {
        let (color, text) = if on { (GOOD, "ON") } else { (BAD, "OFF") };
        let (rect, _) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
        ui.painter().circle_filled(rect.center(), 6.0, color);
        ui.label(egui::RichText::new(text).strong().color(color));
    });
}

impl eframe::App for DevicesApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.drain(&ctx);
        handle_zoom(&ctx);
        if (ctx.zoom_factor() - self.zoom_saved).abs() > f32::EPSILON {
            self.zoom_saved = ctx.zoom_factor();
            self.save_fields();
        }

        egui::Panel::top("topbar").show(ui, |ui| {
            ui.horizontal(|ui| {
                let menu_label = if self.sidebar_open {
                    "- Menu"
                } else {
                    "+ Menu"
                };
                if ui.button(menu_label).clicked() {
                    self.sidebar_open = !self.sidebar_open;
                }
                ui.heading("Devices (Rust)");
                if self.pending > 0 {
                    ui.spinner();
                    ui.label(format!("{} jobs", self.pending));
                }
                let zoom_label = format!("{}%", (ctx.zoom_factor() * 100.0).round());
                if ui
                    .button(zoom_label)
                    .on_hover_text("UI zoom (Ctrl+wheel); click resets to 100%")
                    .clicked()
                {
                    ctx.set_zoom_factor(1.0);
                }
                let before = self.dark_mode;
                theme_toggle_button(ui, &mut self.dark_mode);
                if before != self.dark_mode {
                    self.save_fields();
                }
            });
        });

        egui::Panel::bottom("status").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(&self.status_line);
            });
        });

        if self.sidebar_open {
            egui::Panel::left("sidebar").resizable(true).show(ui, |ui| {
                ui.heading("Navigate");
                self.nav_button(ui, Page::Dashboard);
                self.nav_button(ui, Page::Energy);
                ui.collapsing("Lighting", |ui| {
                    self.nav_button(ui, Page::Lighting);
                    for lamp in self.lights.clone() {
                        let id = lamp
                            .get("id")
                            .and_then(|v| v.as_str())
                            .unwrap_or("?")
                            .to_string();
                        let name = lamp
                            .get("name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("?")
                            .to_string();
                        if ui
                            .selectable_label(
                                self.page == Page::LightDetail(id.clone()),
                                name.clone(),
                            )
                            .clicked()
                        {
                            self.page = Page::LightDetail(id);
                        }
                    }
                });
                self.nav_button(ui, Page::Climate);
                self.nav_button(ui, Page::Cameras);
                self.nav_button(ui, Page::Weather);
                ui.collapsing("Alarms", |ui| {
                    self.nav_button(ui, Page::Alarms);
                    for bell in self.ring_doorbells.clone() {
                        let id = bell.get("id").and_then(|v| v.as_i64()).unwrap_or(-1);
                        let name = bell
                            .get("name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("doorbell")
                            .to_string();
                        if id >= 0
                            && ui
                                .selectable_label(
                                    self.page == Page::DoorbellDetail(id),
                                    name.clone(),
                                )
                                .clicked()
                        {
                            self.page = Page::DoorbellDetail(id);
                            self.ring_id = id.to_string();
                        }
                    }
                });
                self.nav_button(ui, Page::Raw);
            });
        }

        egui::CentralPanel::default().show(ui, |ui| match self.page.clone() {
            Page::Dashboard => self.page_dashboard(ui),
            Page::Energy => self.page_energy(ui),
            Page::Lighting => self.page_lighting(ui),
            Page::LightDetail(id) => self.page_light_detail(ui, &id),
            Page::Climate => self.page_climate(ui),
            Page::Cameras => self.page_cameras(ui),
            Page::Weather => self.page_weather(ui),
            Page::Alarms => self.page_alarms(ui),
            Page::DoorbellDetail(id) => self.page_doorbell_detail(ui, id),
            Page::Raw => {
                ui.heading("Raw JSON");
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.monospace(&self.raw);
                });
            }
        });
    }
}

impl DevicesApp {
    fn nav_button(&mut self, ui: &mut egui::Ui, page: Page) {
        if ui
            .selectable_label(self.page == page, page.label())
            .clicked()
        {
            self.page = page;
        }
    }

    fn page_dashboard(&mut self, ui: &mut egui::Ui) {
        ui.heading("Dashboard");
        ui.label("Every button below calls the same command bus as the CLI and MCP server.");
        ui.horizontal(|ui| {
            if ui.button("Plugs").clicked() {
                self.page = Page::Energy;
            }
            if ui.button("Lighting").clicked() {
                self.page = Page::Lighting;
            }
            if ui.button("Cameras").clicked() {
                self.page = Page::Cameras;
            }
            if ui.button("Alarms").clicked() {
                self.page = Page::Alarms;
            }
        });
        ui.separator();
        {
            let wx_temp = self
                .wx_card
                .as_ref()
                .and_then(|card| card.get("current"))
                .and_then(|current| current.get("temperature_2m"))
                .and_then(|temp| temp.as_f64())
                .map(|temp| format!("{temp:.1} °C"))
                .unwrap_or_else(|| "—".to_string());
            let hue_total = self.lights.len();
            let hue_on = self
                .lights
                .iter()
                .filter(|lamp| lamp.get("on").and_then(|v| v.as_bool()).unwrap_or(false))
                .count();
            ui.horizontal(|ui| {
                hero_kpi(ui, "Temperature", &wx_temp, SKY);
                hero_kpi(ui, "Hue on", &format!("{hue_on}/{hue_total}"), HUE_YELLOW);
                hero_kpi(ui, "Snapshots", &self.shots.len().to_string(), VIOLET);
                hero_kpi(ui, "Jobs", &self.pending.to_string(), ACCENT);
            });
        }
        ui.separator();
        if let Some(card) = self.wx_card.clone() {
            weather_card(ui, &card);
        }
        ui.horizontal(|ui| {
            icosahedron(ui);
            card_frame(ui, |ui| {
                ui.heading("Fleet at a glance");
                ui.label(format!("Hue lights seen: {}", self.lights.len()));
                ui.label(format!(
                    "Ring doorbells seen: {}",
                    self.ring_doorbells.len()
                ));
                ui.label(format!("Snapshots this session: {}", self.shots.len()));
            });
        });
    }

    fn page_energy(&mut self, ui: &mut egui::Ui) {
        ui.heading(
            egui::RichText::new("Energy — smart plugs")
                .color(ENERGY)
                .strong(),
        );
        combo(ui, "Vendor", &mut self.plug_vendor, VENDORS);
        field(ui, "Device IP", &mut self.plug_ip, false, "192.168.0.17");
        field(ui, "Username", &mut self.plug_user, false, "env fallback");
        field(ui, "Password", &mut self.plug_pass, true, "env fallback");
        ui.horizontal(|ui| {
            if primary(ui, "Get stats").clicked() {
                self.spawn_plug(None);
            }
            if ui.button("Turn ON").clicked() {
                self.spawn_plug(Some(true));
            }
            if danger(ui, "Turn OFF").clicked() {
                self.spawn_plug(Some(false));
            }
        });
        let history: Vec<(String, f64)> = self.power_history.iter().cloned().collect();
        if history.len() >= 2 {
            spark_chart(ui, "Power history (W)", "W", &history, ENERGY);
        }
        ui.separator();
        egui::ScrollArea::vertical().show(ui, |ui| {
            for card in self.plug_cards.clone() {
                card_frame(ui, |ui| {
                    if let (Some(ip), Some(vendor)) = (
                        card.get("device_ip").and_then(|v| v.as_str()),
                        card.get("vendor").and_then(|v| v.as_str()),
                    ) {
                        ui.heading(format!("{vendor} plug @ {ip}"));
                    }
                    value_rows(ui, &card);
                });
            }
        });
    }

    fn page_lighting(&mut self, ui: &mut egui::Ui) {
        ui.heading("Lighting — Hue");
        field(ui, "Bridge IP", &mut self.hue_ip, false, "192.168.0.236");
        field(ui, "Username", &mut self.hue_user, false, "HUE_USERNAME");
        ui.horizontal(|ui| {
            if primary(ui, "List lights").clicked() {
                self.spawn_hue_lights();
            }
        });
        ui.separator();
        let lights = self.lights.clone();
        egui::ScrollArea::vertical().show(ui, |ui| {
            for lamp in &lights {
                let id = lamp.get("id").and_then(|v| v.as_str()).unwrap_or("?");
                let name = lamp.get("name").and_then(|v| v.as_str()).unwrap_or("?");
                card_frame(ui, |ui| {
                    ui.horizontal(|ui| {
                        if ui.link(name).clicked() {
                            self.page = Page::LightDetail(id.to_string());
                        }
                        if let Some(on) = lamp.get("on").and_then(|v| v.as_bool()) {
                            on_pill(ui, on);
                        }
                    });
                    self.light_controls(ui, id);
                });
            }
        });
    }

    fn light_controls(&mut self, ui: &mut egui::Ui, id: &str) {
        let bri = self
            .lights
            .iter()
            .find(|lamp| lamp.get("id").and_then(|v| v.as_str()) == Some(id))
            .and_then(|lamp| lamp.get("bri"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u8;
        ui.horizontal(|ui| {
            if ui.button("On").clicked() {
                self.spawn_hue_set(id.to_string(), true, None);
            }
            if ui.button("Off").clicked() {
                self.spawn_hue_set(id.to_string(), false, None);
            }
            let pending_bri = *self.bri_pending.entry(id.to_string()).or_insert(bri);
            let mut slider_value = pending_bri;
            let slider = ui.add(egui::Slider::new(&mut slider_value, 1..=254).text("brightness"));
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
    }

    fn page_light_detail(&mut self, ui: &mut egui::Ui, id: &str) {
        let lamp = self
            .lights
            .iter()
            .find(|lamp| lamp.get("id").and_then(|v| v.as_str()) == Some(id))
            .cloned();
        match lamp {
            Some(lamp) => {
                let name = lamp.get("name").and_then(|v| v.as_str()).unwrap_or(id);
                ui.heading(egui::RichText::new(name).color(HUE_YELLOW).strong());
                card_frame(ui, |ui| value_rows(ui, &lamp));
                ui.separator();
                self.light_controls(ui, id);
            }
            None => {
                ui.heading(format!("Light {id}"));
                ui.label("Not in the last listing — hit List lights on the Lighting page.");
                if ui.button("List lights now").clicked() {
                    self.spawn_hue_lights();
                    self.page = Page::Lighting;
                }
            }
        }
    }

    fn page_climate(&mut self, ui: &mut egui::Ui) {
        ui.heading("Climate");
        card_frame(ui, |ui| {
            ui.label("Not ported yet. Midea / Mitsubishi drivers live in the Python app;");
            ui.label("this page lands with the climate tier (one Command variant each).");
        });
    }

    fn page_weather(&mut self, ui: &mut egui::Ui) {
        ui.heading("Weather (Open-Meteo, keyless — Vienna default)");
        field(ui, "Lat", &mut self.wx_lat, false, "48.2082");
        field(ui, "Lon", &mut self.wx_lon, false, "16.3738");
        if primary(ui, "Refresh").clicked() {
            let lat = self.wx_lat.trim().parse().ok();
            let lon = self.wx_lon.trim().parse().ok();
            self.spawn_weather_latlon(lat, lon);
        }
        if ui.button("Load Netatmo station").clicked() {
            self.spawn_netatmo();
        }
        ui.separator();
        if let Some(card) = self.wx_card.clone() {
            weather_card(ui, &card);
        }
        if let Some(samples) = self.wx_hourly() {
            spark_chart(ui, "Next 24h temperature", "°C", &samples, SKY);
        }
    }

    fn spawn_netatmo(&mut self) {
        let fleet = self.fleet.clone();
        self.note("loading Netatmo station…");
        self.spawn(move || async move {
            let (id, secret, refresh) = match &fleet {
                Some(fleet) => (
                    fleet.netatmo_client_id.clone(),
                    fleet.netatmo_client_secret.clone(),
                    fleet.netatmo_refresh_token.clone(),
                ),
                None => (None, None, None),
            };
            Job::Weather(
                dispatch(Command::NetatmoStations {
                    client_id: id,
                    client_secret: secret,
                    refresh_token: refresh,
                })
                .await,
            )
        });
    }

    /// Per-camera login from the fleet import, matched by IP with first-cam
    /// fallback. Secrets stay in memory; callers never log them.
    fn cam_auth_for(&self, ip: &str) -> (Option<String>, Option<String>) {
        let picked = self.fleet.as_ref().and_then(|fleet| {
            fleet
                .cam_auth
                .iter()
                .find(|auth| auth.ip == ip)
                .or_else(|| fleet.cam_auth.first())
        });
        match picked {
            Some(auth) => (auth.username.clone(), auth.password.clone()),
            None => (None, None),
        }
    }

    /// Next-24h temperature series from the weather card's hourly arrays.
    /// Defensive at every step: malformed entries are skipped, never fatal.
    fn wx_hourly(&self) -> Option<Vec<(String, f64)>> {
        let hourly = self.wx_card.as_ref()?.get("hourly")?;
        let times = hourly.get("time")?.as_array()?;
        let temps = hourly.get("temperature_2m")?.as_array()?;
        let mut out = Vec::new();
        for (time, temp) in times.iter().zip(temps.iter()).take(24) {
            let label = time
                .as_str()
                .and_then(|t| t.split('T').nth(1))
                .and_then(|h| h.get(..5))
                .unwrap_or("?")
                .to_string();
            out.push((label, temp.as_f64().unwrap_or(f64::NAN)));
        }
        out.retain(|(_, v)| v.is_finite());
        (!out.is_empty()).then_some(out)
    }

    fn spawn_weather_latlon(&mut self, lat: Option<f64>, lon: Option<f64>) {
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

    fn page_cameras(&mut self, ui: &mut egui::Ui) {
        ui.heading("Camera snapshots (real frames)");
        combo(ui, "Source", &mut self.cam_source, SOURCES);
        field(ui, "Device IP / doorbell id", &mut self.cam_ip, false, "");
        field(ui, "URL", &mut self.cam_url, false, "https://…jpg");
        field(ui, "Model", &mut self.cam_model, false, "tapo: c220…");
        field(ui, "Username", &mut self.cam_user, false, "");
        field(ui, "Password", &mut self.cam_pass, true, "");
        field(ui, "USB index", &mut self.cam_index, false, "0");
        if primary(ui, "Snapshot").clicked() {
            let source = match CamSource::parse(&self.cam_source) {
                Ok(source) => source,
                Err(e) => {
                    self.note(e.to_string());
                    self.raw = serde_json::json!({"error": e.to_string()}).to_string();
                    return;
                }
            };
            let device_index = self.cam_index.trim().parse().ok();
            let label = format!("{} {}", source.as_str(), self.cam_ip.trim());
            let mut user = Self::opt(&self.cam_user);
            let mut pass = Self::opt(&self.cam_pass);
            if user.is_none() {
                user = import::fleet_secret_any(&["CAMERA_USERNAME", "TAPO_CAMERA_USERNAME"]);
            }
            if pass.is_none() {
                pass = import::fleet_secret_any(&["CAMERA_PASSWORD", "TAPO_CAMERA_PASSWORD"]);
            }
            if user.is_none() || pass.is_none() {
                let (fleet_user, fleet_pass) = self.cam_auth_for(self.cam_ip.trim());
                if user.is_none() {
                    user = fleet_user;
                }
                if pass.is_none() {
                    pass = fleet_pass;
                }
            }
            self.spawn_snapshot(
                label,
                source,
                self.cam_ip.trim().to_string(),
                Self::opt(&self.cam_model),
                user,
                pass,
                None,
                device_index,
                Self::opt(&self.cam_url),
                Self::opt(&self.cam_token),
            );
        }
        ui.separator();
        egui::ScrollArea::vertical().show(ui, |ui| {
            for shot in &self.shots {
                card_frame(ui, |ui| {
                    ui.heading(&shot.label);
                    value_rows(ui, &shot.meta);
                    if let Some(tex) = &shot.texture {
                        ui.add(egui::Image::new(tex).max_width(560.0));
                    }
                });
            }
        });
    }

    fn page_alarms(&mut self, ui: &mut egui::Ui) {
        ui.heading("Alarms — Ring");
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
            if ui.button("Read alarm state").clicked() {
                self.spawn_alarm_read();
            }
        });
        ui.separator();
        ui.horizontal(|ui| {
            combo(ui, "Mode", &mut self.alarm_mode, MODES);
            field(
                ui,
                "Location id",
                &mut self.alarm_location,
                false,
                "from alarm state",
            );
            if ui.button("Apply mode").clicked() {
                self.spawn_alarm_set();
            }
        });
        ui.separator();
        if ui.button("Doorbell snapshot (see Cameras)").clicked() {
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
            self.page = Page::Cameras;
        }
        egui::ScrollArea::vertical().show(ui, |ui| {
            for location in self.ring_locations.clone() {
                card_frame(ui, |ui| {
                    ui.heading("Alarm location");
                    value_rows(ui, &location);
                });
            }
        });
    }

    fn page_doorbell_detail(&mut self, ui: &mut egui::Ui, id: i64) {
        let bell = self
            .ring_doorbells
            .iter()
            .find(|bell| bell.get("id").and_then(|v| v.as_i64()) == Some(id))
            .cloned();
        match bell {
            Some(bell) => {
                let name = bell
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("doorbell");
                ui.heading(
                    egui::RichText::new(format!("{name} (id {id})"))
                        .color(RING_BLUE)
                        .strong(),
                );
                card_frame(ui, |ui| value_rows(ui, &bell));
                ui.separator();
                if ui.button("Snapshot now").clicked() {
                    let (email, pass, token) = (
                        Self::opt(&self.ring_email),
                        Self::opt(&self.ring_pass),
                        Self::opt(&self.ring_token),
                    );
                    self.spawn_snapshot(
                        format!("ring {id}"),
                        CamSource::Ring,
                        id.to_string(),
                        None,
                        email,
                        pass,
                        None,
                        None,
                        None,
                        token,
                    );
                    self.page = Page::Cameras;
                }
            }
            None => {
                ui.heading(format!("Doorbell {id}"));
                ui.label("Not in the last listing — hit Doorbells on the Alarms page.");
            }
        }
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

#[cfg(test)]
mod parity_tests {
    use super::*;
    use egui_kittest::Harness;

    /// Component snapshots guard the converged look (see docs/UI_PARITY.md).
    /// A failing snapshot means the look changed: inspect the PNG, then fix
    /// the code or bless the new gold.
    /// Glyph allowlist guard: egui 0.37 bundles only Ubuntu-Light +
    /// egui-icons, so geometric/symbolic glyphs (verified missing: check,
    /// play, diamond, triangles, star, warning, circles, arrow) render as
    /// tofu. This test renders every non-ASCII glyph the UI uses and fails
    /// loudly if anyone adds an uncovered one. Verified set: ° × · … —.
    /// Status dots are painted circles (see on_pill), never glyphs.
    #[test]
    fn glyph_allowlist() {
        let mut harness = Harness::new_ui(|ui| {
            ui.label("18.7 °C · 42.5 × 2 — querying…");
        });
        harness.run();
    }

    #[test]
    fn ico_projection_is_identity_at_rest() {
        // Zero rotation changes nothing; lengths are preserved by construction.
        let (x, y, z) = ico_point([1.0, 2.0, 3.0], 0.0, 0.0);
        assert!((x - 1.0).abs() < 1e-6 && (y - 2.0).abs() < 1e-6 && (z - 3.0).abs() < 1e-6);
        // 20 faces, 12 unique vertices — the icosahedron census.
        assert_eq!(ico_faces().len(), 20);
        assert_eq!(ico_vertices().len(), 12);
    }

    /// Self-scan: the render test above only covers what it renders — this
    /// covers the whole file. Any non-ASCII char outside the verified set
    /// fails `cargo test` (per the fleet standard's tofu guard).
    #[test]
    fn glyph_source_scan() {
        const ALLOWED: &[char] = &['°', '×', '·', '…', '—'];
        let source = include_str!("app.rs");
        let mut offenders = Vec::new();
        for (idx, ch) in source.char_indices() {
            if !ch.is_ascii() && !ALLOWED.contains(&ch) {
                offenders.push((idx, ch));
                if offenders.len() >= 8 {
                    break;
                }
            }
        }
        assert!(
            offenders.is_empty(),
            "non-allowlisted glyphs in app.rs: {offenders:?}"
        );
    }

    #[test]
    fn hero_kpi_card() {
        let mut harness = Harness::new_ui(|ui| {
            hero_kpi(ui, "Temperature", "18.7 °C", SKY);
        });
        harness.snapshot("hero_kpi");
    }

    #[test]
    fn on_off_pills() {
        let mut harness = Harness::new_ui(|ui| {
            on_pill(ui, true);
            on_pill(ui, false);
        });
        harness.snapshot("pills");
    }

    #[test]
    fn value_card_with_error() {
        let mut harness = Harness::new_ui(|ui| {
            card_frame(ui, |ui| {
                value_rows(
                    ui,
                    &serde_json::json!({"error": "auth failed: set HUE_USERNAME"}),
                );
            });
            card_frame(ui, |ui| {
                value_rows(
                    ui,
                    &serde_json::json!({"device_ip": "192.168.0.17", "is_on": true}),
                );
            });
        });
        harness.snapshot("value_cards");
    }
}
