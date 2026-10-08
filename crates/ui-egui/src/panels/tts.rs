//! Text to Speech panel: write what the narrator says, pick a language, voice, vocal pitch and pace,
//! hear it, and save it as a narration clip. Selecting a narration clip on the timeline loads its
//! script and settings here (and brings the panel forward), and Save regenerates it in place.
//! Every action is a `tts.*` command, so the panel, MCP and the control channel share one path.
//!
//! Automation ids: `tts.language`, `tts.voice`, `tts.hearVoice`, `tts.advanced` (twirl),
//! `tts.pitch`, `tts.pace`, `tts.text`, `tts.addPause`, `tts.preview`, `tts.save`, `tts.new`.

use egui::{Align2, Rect, Sense, Stroke, StrokeKind, pos2, vec2};
use filmcraft_project::{ClipId, VocalPitch};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::FilmcraftApp;
use crate::icons::{self, Icon};
use crate::theme::Tokens;

const PAD: f32 = 14.0;
const FOOTER_H: f32 = 48.0;

/// What the panel is writing (UI state, serde so agents can read it).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct TtsDraft {
    pub text: String,
    pub language: String,
    pub voice: String,
    pub pitch: VocalPitch,
    pub pace: f64,
    pub advanced_open: bool,
    /// The narration clip and item the draft was loaded from (None: a new narration).
    pub loaded_from: Option<(u64, u64)>,
}

impl Default for TtsDraft {
    fn default() -> Self {
        TtsDraft {
            text: String::new(),
            language: "en-US".into(),
            voice: filmcraft_tts::default_voice_id().into(),
            pitch: VocalPitch::Default,
            pace: 1.0,
            advanced_open: true,
            loaded_from: None,
        }
    }
}

impl TtsDraft {
    fn params(&self) -> serde_json::Value {
        json!({"text": self.text, "voice": self.voice, "pitch": self.pitch.id(), "pace": self.pace})
    }
}

/// The selected narration clip and its item, if any.
fn selected_narration(app: &FilmcraftApp) -> Option<(ClipId, u64)> {
    let clip = filmcraft_engine::narration::target_clip(&app.session, &serde_json::Value::Null)?;
    let item = app.session.active_sequence()?.find_item(clip)?.1.item;
    Some((clip, item.0))
}

/// Load the selected narration clip into the draft when the selection moves to a different one,
/// and bring the panel forward. Runs every frame, also when the panel is hidden.
pub fn follow_selection(app: &mut FilmcraftApp) {
    let sel = selected_narration(app);
    match sel {
        Some((clip, item)) if app.ui.tts.loaded_from != Some((clip.0, item)) => {
            let first_time = app.ui.tts.loaded_from.map(|(c, _)| c) != Some(clip.0);
            if let Some(n) = app.session.project.narrations.get(&filmcraft_project::ItemId(item)).cloned() {
                let d = &mut app.ui.tts;
                d.text = n.text;
                d.language = n.language;
                d.voice = n.voice;
                d.pitch = n.pitch;
                d.pace = n.pace;
                d.loaded_from = Some((clip.0, item));
                if first_time {
                    app.show_panel(crate::dock::PanelKind::TextToSpeech);
                }
            }
        }
        None if app.ui.tts.loaded_from.is_some() => {
            // deselected: keep the text so it can be saved as a new narration
            app.ui.tts.loaded_from = None;
        }
        _ => {}
    }
}

enum Action {
    Run(&'static str, serde_json::Value, bool),
    New,
}

pub fn show(app: &mut FilmcraftApp, ui: &mut egui::Ui, rect: Rect) {
    let t = app.tokens;
    let painter = ui.painter().clone();
    painter.rect_filled(rect, 0.0, t.panel_bg);
    let voices = filmcraft_tts::voices();
    let editing = app.ui.tts.loaded_from.map(|(c, _)| c);
    let mut actions: Vec<Action> = Vec::new();
    let auto = &mut app.auto;
    let d = &mut app.ui.tts;

    let body = Rect::from_min_max(pos2(rect.min.x + PAD, rect.min.y + 8.0), pos2(rect.max.x - PAD, rect.max.y - FOOTER_H));
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(body).id_salt("tts-body"));
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(&mut child, |ui| {
        ui.set_max_width(ui.available_width() - 6.0);
        let w = ui.available_width();
        // mode line: editing a clip, or writing a new narration
        ui.horizontal(|ui| {
            let mode = if editing.is_some() { "Editing the selected narration" } else { "New narration" };
            ui.label(egui::RichText::new(mode).color(t.text_dim).size(11.5));
            if editing.is_some() {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let b = ui.small_button("New");
                    auto.add("tts.new", visible(ui, b.rect), "New narration");
                    if b.clicked() {
                        actions.push(Action::New);
                    }
                });
            }
        });
        ui.add_space(6.0);
        label(ui, &t, "Language");
        let lang_name = filmcraft_tts::LANGUAGES.iter().find(|(id, _)| *id == d.language).map_or(d.language.as_str(), |(_, n)| n);
        let cb = egui::ComboBox::from_id_salt("tts-language").selected_text(lang_name).width(w).show_ui(ui, |ui| {
            for (id, name) in filmcraft_tts::LANGUAGES {
                if ui.selectable_label(d.language == *id, *name).clicked() {
                    d.language = id.to_string();
                }
            }
        });
        auto.add("tts.language", visible(ui, cb.response.rect), "Language");
        ui.add_space(10.0);
        label(ui, &t, "Voice");
        let voice_name = voices.iter().find(|v| v.id == d.voice).map_or(d.voice.as_str(), |v| v.name);
        let cb = egui::ComboBox::from_id_salt("tts-voice").selected_text(voice_name).width(w).show_ui(ui, |ui| {
            for v in voices.iter().filter(|v| v.language == d.language) {
                let r = ui.selectable_label(d.voice == v.id, v.name).on_hover_text(v.description);
                if r.clicked() {
                    d.voice = v.id.to_string();
                }
            }
        });
        auto.add("tts.voice", visible(ui, cb.response.rect), "Voice");
        ui.add_space(4.0);
        let (r, resp) = ui.allocate_exact_size(vec2(130.0, 22.0), Sense::click());
        let col = if resp.hovered() { t.accent_hover } else { t.accent };
        icons::paint(ui.painter(), Rect::from_center_size(pos2(r.min.x + 8.0, r.center().y), vec2(14.0, 14.0)), Icon::Play, col);
        ui.painter().text(pos2(r.min.x + 22.0, r.center().y), Align2::LEFT_CENTER, "Hear this voice", Tokens::semibold(12.0), col);
        auto.add("tts.hearVoice", visible(ui, r), "Hear this voice");
        if resp.clicked() {
            actions.push(Action::Run("tts.preview", json!({"sample": true, "voice": d.voice, "pitch": d.pitch.id(), "pace": d.pace}), true));
        }
        ui.add_space(8.0);
        let (hr, open) = crate::widgets::section_header(ui, egui::Id::new("tts-advanced"), "Advanced", d.advanced_open, &t, true);
        auto.add("tts.advanced", visible(ui, hr.rect), "Advanced");
        d.advanced_open = open;
        if open {
            ui.add_space(4.0);
            label(ui, &t, "Vocal pitch");
            let cb = egui::ComboBox::from_id_salt("tts-pitch").selected_text(d.pitch.label()).width(w).show_ui(ui, |ui| {
                for p in VocalPitch::ALL {
                    if ui.selectable_label(d.pitch == p, p.label()).clicked() {
                        d.pitch = p;
                    }
                }
            });
            auto.add("tts.pitch", visible(ui, cb.response.rect), "Vocal pitch");
            ui.add_space(10.0);
            label(ui, &t, "Pace");
            ui.spacing_mut().slider_width = w - 56.0;
            let s = ui.add(egui::Slider::new(&mut d.pace, filmcraft_tts::MIN_PACE..=filmcraft_tts::MAX_PACE).step_by(0.05).fixed_decimals(2).suffix("×"));
            auto.add("tts.pace", visible(ui, s.rect), "Pace");
            let (r, _) = ui.allocate_exact_size(vec2(w - 56.0, 14.0), Sense::hover());
            for (x, l) in [(0.0, "0.5×"), (1.0 / 3.0, "1×"), (1.0, "2×")] {
                let px = r.min.x + 6.0 + (r.width() - 12.0) * x;
                ui.painter().text(pos2(px, r.center().y), Align2::CENTER_CENTER, l, Tokens::ui(10.5), t.text_faint);
            }
        }
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Text").color(t.text).size(12.5).strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let (ir, i) = ui.allocate_exact_size(vec2(14.0, 14.0), Sense::hover());
                icons::paint(ui.painter(), ir, Icon::Info, t.text_faint);
                i.on_hover_text("Add pause inserts [pause 1s]. Change the number for a longer or shorter pause, e.g. [pause 500ms] or [pause 2s].");
                let b = ui.small_button("Add pause");
                auto.add("tts.addPause", visible(ui, b.rect), "Add pause");
                if b.clicked() {
                    if !d.text.is_empty() && !d.text.ends_with(' ') {
                        d.text.push(' ');
                    }
                    d.text.push_str(filmcraft_tts::script::PAUSE_MARKER);
                    d.text.push(' ');
                }
            });
        });
        ui.add_space(4.0);
        let te = egui::TextEdit::multiline(&mut d.text)
            .hint_text("Type what you want the voice to say")
            .desired_width(w)
            .desired_rows(8)
            .char_limit(filmcraft_tts::MAX_TEXT_BYTES);
        let r = ui.add(te);
        auto.add("tts.text", visible(ui, r.rect), "Text");
    });

    // footer: Preview | Save
    let fy = rect.max.y - FOOTER_H + 10.0;
    let half = (rect.width() - PAD * 2.0 - 10.0) / 2.0;
    let can_say = filmcraft_tts::check_text(&d.text).is_ok();
    let pr = Rect::from_min_size(pos2(rect.min.x + PAD, fy), vec2(half, 28.0));
    if button(ui, &t, pr, Icon::Play, "Preview", false, can_say, "tts-preview").clicked() && can_say {
        actions.push(Action::Run("tts.preview", d.params(), true));
    }
    auto.add("tts.preview", pr, "Preview");
    let sr = Rect::from_min_size(pos2(pr.max.x + 10.0, fy), vec2(half, 28.0));
    let can_save = can_say && app.session.active_sequence().is_some();
    if button(ui, &t, sr, Icon::Plus, if editing.is_some() { "Save" } else { "Add to timeline" }, true, can_save, "tts-save").clicked() && can_save {
        let mut p = d.params();
        match editing {
            Some(c) => {
                p["clip"] = json!(c);
                actions.push(Action::Run("tts.edit", p, false));
            }
            None => actions.push(Action::Run("tts.create", p, false)),
        }
    }
    auto.add("tts.save", sr, "Save");

    for a in actions {
        match a {
            Action::New => {
                let keep = (app.ui.tts.language.clone(), app.ui.tts.voice.clone(), app.ui.tts.pitch, app.ui.tts.pace);
                app.session.state.selection.clear();
                app.ui.tts = TtsDraft { language: keep.0, voice: keep.1, pitch: keep.2, pace: keep.3, ..Default::default() };
            }
            Action::Run(cmd, p, play) => match app.session.execute(cmd, p) {
                Ok(r) => {
                    if play {
                        if let Err(e) = app.play_tts_preview() {
                            app.ui.status = e;
                        }
                    } else {
                        app.ui.status = match cmd {
                            "tts.create" => format!("Narration added on {}", r["track"].as_str().unwrap_or("an audio track")),
                            _ => "Narration updated".into(),
                        };
                    }
                }
                Err(e) => app.ui.status = e.to_string(),
            },
        }
    }
}

/// The part of `r` the scroll area shows (what a click can reach).
fn visible(ui: &egui::Ui, r: Rect) -> Rect {
    r.intersect(ui.clip_rect())
}

fn label(ui: &mut egui::Ui, t: &Tokens, text: &str) {
    ui.label(egui::RichText::new(text).color(t.text).size(12.5).strong());
    ui.add_space(2.0);
}

#[allow(clippy::too_many_arguments)]
fn button(ui: &mut egui::Ui, t: &Tokens, r: Rect, icon: Icon, text: &str, primary: bool, active: bool, id: &str) -> egui::Response {
    let resp = ui.interact(r, egui::Id::new(id), Sense::click());
    let p = ui.painter();
    let (bg, fg) = match (primary, active) {
        (true, true) => (if resp.hovered() { t.accent_hover } else { t.accent }, egui::Color32::WHITE),
        (_, false) => (t.field_bg, t.text_faint),
        (false, true) => (if resp.hovered() { t.hover } else { t.field_bg }, t.text),
    };
    p.rect_filled(r, 4.0, bg);
    if !primary {
        p.rect_stroke(r, 4.0, Stroke::new(1.0, t.field_border), StrokeKind::Inside);
    }
    let g = p.layout_no_wrap(text.to_string(), Tokens::semibold(12.5), fg);
    let total = 18.0 + g.size().x;
    let x0 = r.center().x - total / 2.0;
    icons::paint(p, Rect::from_center_size(pos2(x0 + 7.0, r.center().y), vec2(14.0, 14.0)), icon, fg);
    p.galley(pos2(x0 + 18.0, r.center().y - g.size().y / 2.0), g, fg);
    resp
}
