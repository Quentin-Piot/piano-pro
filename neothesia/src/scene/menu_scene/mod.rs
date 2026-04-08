mod state;
use state::{Page, UiState};

mod audio_import;
use audio_import::{AudioImportState, convert_selected_audio, open_audio_file_picker};

mod midi_picker;
use midi_picker::{load_from_library, open_midi_file_picker};

mod neo_btn;
use neo_btn::{neo_btn, neo_btn_icon};

mod settings;
mod tracks;

use std::{future::Future, time::Duration};

use crate::utils::{BoxFuture, window::WinitEvent};
use neothesia_core::render::{BgPipeline, QuadRenderer, TextRenderer};

use winit::{
    event::WindowEvent,
    keyboard::{Key, NamedKey},
};

use crate::{NeothesiaEvent, context::Context, icons, scene::Scene, song::Song};

use std::task::Waker;

use super::NuonRenderer;

type MsgFn = Box<dyn FnOnce(&mut UiState, &mut Context)>;

fn draw_card(
    ui: &mut nuon::Ui,
    width: f32,
    height: f32,
    radius: f32,
    border: nuon::Color,
    fill: nuon::Color,
) {
    nuon::quad()
        .size(width, height)
        .color(border)
        .border_radius([radius; 4])
        .build(ui);
    nuon::quad()
        .pos(1.0, 1.0)
        .size(width - 2.0, height - 2.0)
        .color(fill)
        .border_radius([radius - 1.0; 4])
        .build(ui);
}

fn on_async<T, Fut, FN>(future: Fut, f: FN) -> BoxFuture<MsgFn>
where
    T: 'static,
    Fut: Future<Output = T> + Send + 'static,
    FN: FnOnce(T, &mut UiState, &mut Context) + Send + 'static,
{
    Box::pin(async {
        let res = future.await;
        let f: MsgFn = Box::new(move |data, ctx| f(res, data, ctx));
        f
    })
}

#[derive(Default, Debug, Clone, Copy, Eq, PartialEq)]
enum Popup {
    #[default]
    None,
    OutputSelector,
    InputSelector,
}

impl Popup {
    fn toggle(&mut self, new: Self) {
        *self = if *self == new { Self::None } else { new }
    }

    fn close(&mut self) {
        *self = Self::None;
    }
}

pub struct MenuScene {
    bg_pipeline: BgPipeline,
    text_renderer: TextRenderer,
    nuon_renderer: NuonRenderer,

    state: UiState,

    context: std::task::Context<'static>,
    futures: Vec<BoxFuture<MsgFn>>,

    quad_pipeline: QuadRenderer,
    nuon: nuon::Ui,

    tracks_scroll: nuon::ScrollState,
    settings_scroll: nuon::ScrollState,
    popup: Popup,
    audio_spinner_phase: f32,

    name_input: nuon::TextInputState,
    renaming_stored_name: Option<String>,
}

impl MenuScene {
    pub fn new(ctx: &mut Context, song: Option<Song>) -> Self {
        let iced_state = UiState::new(ctx, song);

        let quad_pipeline = ctx.quad_renderer_facotry.new_renderer();
        let text_renderer = ctx.text_renderer_factory.new_renderer();

        Self {
            bg_pipeline: BgPipeline::new(&ctx.gpu),
            text_renderer,
            state: iced_state,
            nuon_renderer: NuonRenderer::new(ctx),

            context: std::task::Context::from_waker(noop_waker_ref()),
            futures: Vec::new(),

            quad_pipeline,
            nuon: nuon::Ui::new(),
            tracks_scroll: nuon::ScrollState::new(),
            settings_scroll: nuon::ScrollState::new(),
            popup: Popup::None,
            audio_spinner_phase: 0.0,
            name_input: nuon::TextInputState::new(),
            renaming_stored_name: None,
        }
    }

    fn main_ui(&mut self, ctx: &mut Context) {
        if self.state.pending_import.is_some() && *self.state.current() != Page::NameEntry {
            if let Some(pending) = &self.state.pending_import {
                self.name_input
                    .set_value(pending.entry.display_name.clone());
                self.state.go_to(Page::NameEntry);
            }
        }

        if self.state.is_loading() {
            let width = ctx.window_state.logical_size.width;
            let height = ctx.window_state.logical_size.height;

            nuon::label()
                .size(width, height)
                .font_size(30.0)
                .text("Loading...")
                .text_justify(nuon::TextJustify::Center)
                .build(&mut self.nuon);
            return;
        }

        let mut nuon = std::mem::replace(&mut self.nuon, nuon::Ui::new());

        match self.state.current() {
            Page::Main => self.main_page_ui(ctx, &mut nuon),
            Page::Settings => self.settings_page_ui(ctx, &mut nuon),
            Page::Library => self.library_page_ui(ctx, &mut nuon),
            Page::AudioImport => self.audio_import_page_ui(ctx, &mut nuon),
            Page::NameEntry => self.name_entry_page_ui(ctx, &mut nuon),
            Page::TrackSelection => self.tracks_page_ui(ctx, &mut nuon),
            Page::PlayConfirm => self.play_confirm_page_ui(ctx, &mut nuon),
        }

        self.nuon = nuon;
    }

    fn play_confirm_page_ui(&mut self, ctx: &mut Context, ui: &mut nuon::Ui) {
        let win_w = ctx.window_state.logical_size.width;
        let win_h = ctx.window_state.logical_size.height;

        let panel_w = (win_w - 80.0).clamp(520.0, 760.0);
        let panel_h = 272.0;
        let button_gap = 12.0;
        let button_w = ((panel_w - 120.0 - button_gap) / 2.0).clamp(188.0, 230.0);
        let button_h = 68.0;
        let actions_w = button_w * 2.0 + button_gap;

        let song_name = self
            .state
            .song()
            .map(|s| {
                s.display_name
                    .clone()
                    .unwrap_or_else(|| s.file.name.clone())
            })
            .unwrap_or_default();

        nuon::translate()
            .x(nuon::center_x(win_w, panel_w))
            .y(nuon::center_y(win_h, panel_h))
            .build(ui, |ui| {
                draw_card(
                    ui,
                    panel_w,
                    panel_h,
                    28.0,
                    nuon::theme::DIVIDER,
                    nuon::theme::PANEL,
                );

                nuon::quad()
                    .x(28.0)
                    .y(24.0)
                    .size(104.0, 24.0)
                    .color(nuon::theme::PRIMARY_SOFT)
                    .border_radius([12.0; 4])
                    .build(ui);
                nuon::label()
                    .x(28.0)
                    .y(24.0)
                    .size(104.0, 24.0)
                    .font_size(11.0)
                    .bold(true)
                    .color(nuon::theme::PRIMARY)
                    .text("READY TO PLAY")
                    .build(ui);

                nuon::label()
                    .x(28.0)
                    .y(60.0)
                    .text("Start playback")
                    .font_size(30.0)
                    .bold(true)
                    .text_justify(nuon::TextJustify::Left)
                    .size(panel_w - 56.0, 34.0)
                    .build(ui);

                nuon::translate().pos(28.0, 104.0).build(ui, |ui| {
                    draw_card(
                        ui,
                        panel_w - 56.0,
                        68.0,
                        14.0,
                        nuon::theme::DIVIDER,
                        nuon::theme::SURFACE,
                    );
                    nuon::label()
                        .x(16.0)
                        .y(12.0)
                        .size(panel_w - 88.0, 16.0)
                        .font_size(11.0)
                        .bold(true)
                        .color(nuon::theme::TEXT_MUTED)
                        .text_justify(nuon::TextJustify::Left)
                        .text("SELECTED PIECE")
                        .build(ui);
                    nuon::label()
                        .x(16.0)
                        .y(28.0)
                        .size(panel_w - 88.0, 26.0)
                        .font_size(20.0)
                        .bold(true)
                        .color(nuon::theme::TEXT)
                        .text_justify(nuon::TextJustify::Left)
                        .text(if song_name.is_empty() {
                            "No piece selected"
                        } else {
                            &song_name
                        })
                        .build(ui);
                });

                nuon::translate()
                    .pos(nuon::center_x(panel_w, actions_w), 188.0)
                    .build(ui, |ui| {
                        if neo_btn()
                            .size(button_w, button_h)
                            .label("Layout")
                            .icon(icons::note_list_icon())
                            .centered()
                            .build(ui)
                        {
                            self.state.go_to(Page::TrackSelection);
                        }

                        nuon::translate()
                            .x(button_w + button_gap)
                            .add_to_current(ui);

                        if neo_btn()
                            .size(button_w, button_h)
                            .label("Start Now")
                            .primary()
                            .centered()
                            .build(ui)
                        {
                            state::play(&self.state, ctx);
                        }
                    });
            });
    }

    fn main_page_ui(&mut self, ctx: &mut Context, ui: &mut nuon::Ui) {
        let win_w = ctx.window_state.logical_size.width;
        let win_h = ctx.window_state.logical_size.height;

        let shell_w = (win_w - 80.0).clamp(760.0, 1060.0);
        let shell_h = (win_h - 88.0).clamp(520.0, 590.0);
        let left_w = (shell_w * 0.39).clamp(300.0, 380.0);
        let gap = 22.0;
        let right_w = shell_w - left_w - gap - 64.0;
        let action_h = 66.0;

        nuon::translate()
            .x(nuon::center_x(win_w, shell_w))
            .y(nuon::center_y(win_h, shell_h))
            .build(ui, |ui| {
                draw_card(
                    ui,
                    shell_w,
                    shell_h,
                    30.0,
                    nuon::theme::DIVIDER,
                    nuon::theme::PANEL,
                );

                nuon::translate().pos(32.0, 30.0).build(ui, |ui| {
                    nuon::quad()
                        .size(122.0, 28.0)
                        .color(nuon::theme::PRIMARY_SOFT)
                        .border_radius([14.0; 4])
                        .build(ui);
                    nuon::label()
                        .size(122.0, 28.0)
                        .font_size(12.0)
                        .bold(true)
                        .text("PianoPro")
                        .build(ui);
                    nuon::label()
                        .y(52.0)
                        .text("PianoPro")
                        .size(left_w, 44.0)
                        .font_size(34.0)
                        .bold(true)
                        .text_justify(nuon::TextJustify::Left)
                        .build(ui);

                    let info_y = 136.0;
                    let info_h = 186.0;
                    nuon::translate().y(info_y).build(ui, |ui| {
                        draw_card(
                            ui,
                            left_w,
                            info_h,
                            24.0,
                            nuon::theme::DIVIDER,
                            nuon::theme::SURFACE,
                        );

                        if let Some(song) = self.state.song() {
                            let active_tracks = song
                                .file
                                .tracks
                                .iter()
                                .filter(|track| !track.notes.is_empty())
                                .count();
                            let note_count = song
                                .file
                                .tracks
                                .iter()
                                .map(|track| track.notes.len())
                                .sum::<usize>();

                            nuon::label()
                                .x(20.0)
                                .y(18.0)
                                .text("CURRENT PIECE")
                                .font_size(12.0)
                                .bold(true)
                                .color(nuon::theme::TEXT_MUTED)
                                .text_justify(nuon::TextJustify::Left)
                                .size(left_w - 40.0, 18.0)
                                .build(ui);
                            nuon::label()
                                .x(20.0)
                                .y(46.0)
                                .text(song.display_name.as_ref().unwrap_or(&song.file.name))
                                .font_size(27.0)
                                .bold(true)
                                .text_justify(nuon::TextJustify::Left)
                                .size(left_w - 40.0, 34.0)
                                .build(ui);
                            nuon::label()
                                .x(20.0)
                                .y(104.0)
                                .text(format!("{active_tracks} active tracks"))
                                .font_size(15.0)
                                .color(nuon::theme::TEXT_MUTED)
                                .text_justify(nuon::TextJustify::Left)
                                .size(left_w - 40.0, 20.0)
                                .build(ui);
                            nuon::label()
                                .x(20.0)
                                .y(128.0)
                                .text(format!("{note_count} notes in session"))
                                .font_size(15.0)
                                .color(nuon::theme::TEXT_MUTED)
                                .text_justify(nuon::TextJustify::Left)
                                .size(left_w - 40.0, 20.0)
                                .build(ui);
                        } else {
                            nuon::label()
                                .x(20.0)
                                .y(18.0)
                                .text(
                                    "Import a MIDI file to unlock\nplayback, tracks and transport.",
                                )
                                .font_size(16.0)
                                .bold(true)
                                .text_justify(nuon::TextJustify::Left)
                                .size(left_w - 40.0, 52.0)
                                .build(ui);
                        }
                    });
                });

                nuon::translate()
                    .pos(32.0 + left_w + gap, 42.0)
                    .build(ui, |ui| {
                        nuon::translate().y(0.0).add_to_current(ui);

                        if neo_btn()
                            .size(right_w, action_h)
                            .label("Start")
                            .icon(icons::play_icon())
                            .meta("ENTER")
                            .primary()
                            .build(ui)
                        {
                            self.state.go_to(Page::Library);
                        }

                        nuon::translate().y(action_h + 12.0).add_to_current(ui);

                        if neo_btn()
                            .size(right_w, action_h)
                            .label("Import Audio")
                            .icon(icons::note_list_icon())
                            .meta("A")
                            .build(ui)
                        {
                            self.futures.push(open_audio_file_picker(&mut self.state));
                        }

                        nuon::translate().y(action_h + 12.0).add_to_current(ui);

                        // Continue button - only show if there's a last opened song
                        if let Some(last_song_path) = ctx.config.last_opened_song() {
                            if neo_btn()
                                .size(right_w, action_h)
                                .label("Continue")
                                .icon(icons::play_circle_icon())
                                .meta("C")
                                .primary()
                                .build(ui)
                            {
                                if let Some(stored_name) =
                                    last_song_path.file_name().and_then(|n| n.to_str())
                                {
                                    self.futures
                                        .push(load_from_library(stored_name.to_string()));
                                }
                            }
                            nuon::translate().y(action_h + 12.0).add_to_current(ui);
                        }

                        if neo_btn()
                            .size(right_w, action_h)
                            .label("Free Play")
                            .icon(icons::balloon_icon())
                            .meta("F")
                            .build(ui)
                        {
                            state::freeplay(&self.state, ctx);
                        }

                        nuon::translate().y(action_h + 12.0).add_to_current(ui);

                        if neo_btn()
                            .size(right_w, action_h)
                            .label("Settings")
                            .icon(icons::gear_icon())
                            .meta("S")
                            .build(ui)
                        {
                            self.state.go_to(Page::Settings);
                        }

                        nuon::translate().y(action_h + 12.0).add_to_current(ui);

                        if neo_btn()
                            .size(right_w, action_h)
                            .label("Exit")
                            .icon(icons::exit_icon())
                            .meta("ESC")
                            .danger()
                            .build(ui)
                        {
                            ctx.proxy.send_event(NeothesiaEvent::Exit).ok();
                        }
                    });
            });
    }

    fn library_page_ui(&mut self, ctx: &mut Context, ui: &mut nuon::Ui) {
        let win_w = ctx.window_state.logical_size.width;
        let win_h = ctx.window_state.logical_size.height;

        let panel_w = (win_w - 80.0).clamp(600.0, 900.0);
        let panel_h = (win_h - 88.0).clamp(400.0, 700.0);
        let button_h = 66.0;
        let button_w = (panel_w - 64.0) / 2.0;
        let button_gap = 12.0;

        nuon::translate()
            .x(nuon::center_x(win_w, panel_w))
            .y(nuon::center_y(win_h, panel_h))
            .build(ui, |ui| {
                draw_card(
                    ui,
                    panel_w,
                    panel_h,
                    28.0,
                    nuon::theme::DIVIDER,
                    nuon::theme::PANEL,
                );

                nuon::quad()
                    .x(28.0)
                    .y(24.0)
                    .size(82.0, 24.0)
                    .color(nuon::theme::PRIMARY_SOFT)
                    .border_radius([12.0; 4])
                    .build(ui);
                nuon::label()
                    .x(28.0)
                    .y(24.0)
                    .size(82.0, 24.0)
                    .font_size(11.0)
                    .bold(true)
                    .color(nuon::theme::PRIMARY)
                    .text("LIBRARY")
                    .build(ui);

                let entries = ctx.config.library_entries();

                if entries.is_empty() {
                    nuon::label()
                        .x(28.0)
                        .y(120.0)
                        .text("No MIDI files in library yet")
                        .font_size(20.0)
                        .bold(true)
                        .color(nuon::theme::TEXT)
                        .text_justify(nuon::TextJustify::Center)
                        .size(panel_w - 56.0, 30.0)
                        .build(ui);
                    nuon::label()
                        .x(28.0)
                        .y(170.0)
                        .text("Import MIDI files to get started")
                        .font_size(14.0)
                        .color(nuon::theme::TEXT_MUTED)
                        .text_justify(nuon::TextJustify::Center)
                        .size(panel_w - 56.0, 20.0)
                        .build(ui);
                } else {
                    nuon::label()
                        .x(28.0)
                        .y(70.0)
                        .text(format!("{} piece(s) in library", entries.len()))
                        .font_size(14.0)
                        .bold(true)
                        .color(nuon::theme::TEXT)
                        .text_justify(nuon::TextJustify::Left)
                        .size(panel_w - 56.0, 20.0)
                        .build(ui);

                    let item_h = 50.0;
                    let item_gap = 8.0;
                    let max_items = ((panel_h - 200.0) / (item_h + item_gap)) as usize;

                    // Clone entries to avoid borrow of ctx while building UI
                    let entries_snapshot: Vec<_> = entries
                        .iter()
                        .map(|e| (e.stored_name.clone(), e.display_name.clone()))
                        .collect();

                    nuon::translate().pos(28.0, 110.0).build(ui, |ui| {
                        let btn_gap = 8.0;
                        let edit_btn_w = 40.0;
                        let title_btn_w = panel_w - 56.0 - btn_gap - edit_btn_w;

                        for (idx, (stored_name, display_name)) in
                            entries_snapshot.iter().take(max_items).enumerate()
                        {
                            nuon::translate()
                                .y((item_h + item_gap) * idx as f32)
                                .build(ui, |ui| {
                                    // Title button - load/play
                                    if neo_btn()
                                        .size(title_btn_w, item_h)
                                        .label(display_name)
                                        .build(ui)
                                    {
                                        self.futures.push(load_from_library(stored_name.clone()));
                                    }

                                    // Edit button
                                    nuon::translate()
                                        .x(title_btn_w + btn_gap)
                                        .add_to_current(ui);

                                    if neo_btn()
                                        .id(nuon::Id::hash(stored_name))
                                        .size(edit_btn_w, item_h)
                                        .icon(icons::pencil_icon())
                                        .centered()
                                        .build(ui)
                                    {
                                        self.renaming_stored_name = Some(stored_name.clone());
                                        self.name_input.set_value(display_name.clone());
                                        self.state.go_to(Page::NameEntry);
                                    }
                                });
                        }

                        if entries_snapshot.len() > max_items {
                            nuon::label()
                                .x(0.0)
                                .y((item_h + item_gap) * max_items as f32 + 10.0)
                                .text(format!(
                                    "... and {} more",
                                    entries_snapshot.len() - max_items
                                ))
                                .font_size(12.0)
                                .color(nuon::theme::TEXT_MUTED)
                                .text_justify(nuon::TextJustify::Left)
                                .size(panel_w - 56.0, 16.0)
                                .build(ui);
                        }
                    });
                }

                nuon::translate()
                    .pos(
                        nuon::center_x(panel_w, button_w * 2.0 + button_gap),
                        panel_h - 90.0,
                    )
                    .build(ui, |ui| {
                        if neo_btn()
                            .size(button_w, button_h)
                            .label("Back")
                            .centered()
                            .build(ui)
                        {
                            self.state.go_back();
                        }

                        nuon::translate()
                            .x(button_w + button_gap)
                            .add_to_current(ui);

                        if neo_btn()
                            .size(button_w, button_h)
                            .label("Import MIDI")
                            .primary()
                            .centered()
                            .build(ui)
                        {
                            self.futures.push(open_midi_file_picker(&mut self.state));
                        }
                    });
            });
    }

    fn audio_import_page_ui(&mut self, ctx: &mut Context, ui: &mut nuon::Ui) {
        let win_w = ctx.window_state.logical_size.width;
        let win_h = ctx.window_state.logical_size.height;

        let panel_w = (win_w - 80.0).clamp(600.0, 860.0);
        let panel_h = 360.0;
        let button_h = 66.0;
        let button_w = (panel_w - 64.0) / 2.0;
        let button_gap = 12.0;

        let selected_path = self
            .state
            .audio_import
            .selected_path()
            .and_then(|path| path.file_name())
            .and_then(|name| name.to_str())
            .unwrap_or("No audio file selected")
            .to_string();

        let status_text = match &self.state.audio_import {
            AudioImportState::Empty | AudioImportState::Selected { .. } => {
                "Ready to convert audio into MIDI".to_string()
            }
            AudioImportState::Converting { .. } => {
                let frames = ["|", "/", "-", "\\"];
                let frame = frames[(self.audio_spinner_phase as usize) % frames.len()];
                format!("{frame} Converting audio")
            }
            AudioImportState::Error { message, .. } => {
                let mut message = message.clone();
                if message.len() > 150 {
                    message.truncate(147);
                    message.push_str("...");
                }
                message
            }
        };

        let is_converting = self.state.audio_import.is_converting();

        nuon::translate()
            .x(nuon::center_x(win_w, panel_w))
            .y(nuon::center_y(win_h, panel_h))
            .build(ui, |ui| {
                draw_card(
                    ui,
                    panel_w,
                    panel_h,
                    28.0,
                    nuon::theme::DIVIDER,
                    nuon::theme::PANEL,
                );

                nuon::quad()
                    .x(28.0)
                    .y(24.0)
                    .size(118.0, 24.0)
                    .color(nuon::theme::PRIMARY_SOFT)
                    .border_radius([12.0; 4])
                    .build(ui);
                nuon::label()
                    .x(28.0)
                    .y(24.0)
                    .size(118.0, 24.0)
                    .font_size(11.0)
                    .bold(true)
                    .color(nuon::theme::PRIMARY)
                    .text("AUDIO IMPORT")
                    .build(ui);

                nuon::label()
                    .x(28.0)
                    .y(66.0)
                    .text("Convert audio")
                    .font_size(28.0)
                    .bold(true)
                    .color(nuon::theme::TEXT)
                    .text_justify(nuon::TextJustify::Left)
                    .size(panel_w - 56.0, 34.0)
                    .build(ui);

                nuon::translate().pos(28.0, 122.0).build(ui, |ui| {
                    draw_card(
                        ui,
                        panel_w - 56.0,
                        96.0,
                        14.0,
                        nuon::theme::DIVIDER,
                        nuon::theme::SURFACE,
                    );
                    nuon::label()
                        .x(16.0)
                        .y(14.0)
                        .size(panel_w - 88.0, 16.0)
                        .font_size(11.0)
                        .bold(true)
                        .color(nuon::theme::TEXT_MUTED)
                        .text_justify(nuon::TextJustify::Left)
                        .text("SELECTED AUDIO")
                        .build(ui);
                    nuon::label()
                        .x(16.0)
                        .y(36.0)
                        .size(panel_w - 88.0, 24.0)
                        .font_size(18.0)
                        .bold(true)
                        .color(nuon::theme::TEXT)
                        .text_justify(nuon::TextJustify::Left)
                        .text(&selected_path)
                        .build(ui);
                    nuon::label()
                        .x(16.0)
                        .y(64.0)
                        .size(panel_w - 88.0, 18.0)
                        .font_size(13.0)
                        .color(match self.state.audio_import {
                            AudioImportState::Error { .. } => nuon::theme::DANGER,
                            _ => nuon::theme::TEXT_MUTED,
                        })
                        .text_justify(nuon::TextJustify::Left)
                        .text(&status_text)
                        .build(ui);
                });

                nuon::translate()
                    .pos(
                        nuon::center_x(panel_w, button_w * 2.0 + button_gap),
                        panel_h - 90.0,
                    )
                    .build(ui, |ui| {
                        if neo_btn()
                            .size(button_w, button_h)
                            .label(if is_converting { "Working" } else { "Back" })
                            .centered()
                            .build(ui)
                            && !is_converting
                        {
                            self.state.audio_import = AudioImportState::Empty;
                            self.state.go_back();
                        }

                        nuon::translate()
                            .x(button_w + button_gap)
                            .add_to_current(ui);

                        let action_label = match self.state.audio_import {
                            AudioImportState::Error { .. } => "Try Again",
                            AudioImportState::Converting { .. } => "Converting",
                            _ => "Convert",
                        };

                        if neo_btn()
                            .size(button_w, button_h)
                            .label(action_label)
                            .primary()
                            .centered()
                            .build(ui)
                            && !is_converting
                            && let Some(future) = convert_selected_audio(&mut self.state)
                        {
                            self.futures.push(future);
                        }
                    });
            });
    }

    fn name_entry_page_ui(&mut self, ctx: &mut Context, ui: &mut nuon::Ui) {
        let win_w = ctx.window_state.logical_size.width;
        let win_h = ctx.window_state.logical_size.height;

        let panel_w = (win_w - 80.0).clamp(600.0, 900.0);
        let panel_h = 300.0;
        let button_h = 66.0;
        let button_w = (panel_w - 64.0) / 2.0;
        let button_gap = 12.0;

        nuon::translate()
            .x(nuon::center_x(win_w, panel_w))
            .y(nuon::center_y(win_h, panel_h))
            .build(ui, |ui| {
                draw_card(
                    ui,
                    panel_w,
                    panel_h,
                    28.0,
                    nuon::theme::DIVIDER,
                    nuon::theme::PANEL,
                );

                nuon::quad()
                    .x(28.0)
                    .y(24.0)
                    .size(82.0, 24.0)
                    .color(nuon::theme::PRIMARY_SOFT)
                    .border_radius([12.0; 4])
                    .build(ui);
                nuon::label()
                    .x(28.0)
                    .y(24.0)
                    .size(82.0, 24.0)
                    .font_size(11.0)
                    .bold(true)
                    .color(nuon::theme::PRIMARY)
                    .text("NAME ENTRY")
                    .build(ui);

                nuon::label()
                    .x(28.0)
                    .y(70.0)
                    .text("Name your piece")
                    .font_size(18.0)
                    .bold(true)
                    .color(nuon::theme::TEXT)
                    .text_justify(nuon::TextJustify::Left)
                    .size(panel_w - 56.0, 24.0)
                    .build(ui);

                // Render text input background
                nuon::quad()
                    .x(28.0)
                    .y(110.0)
                    .size(panel_w - 56.0, 50.0)
                    .color(nuon::theme::SURFACE_ELEVATED)
                    .border_radius([6.0; 4])
                    .build(ui);

                // Render text input border
                nuon::quad()
                    .x(28.0)
                    .y(110.0)
                    .size(panel_w - 56.0, 50.0)
                    .color(nuon::theme::PRIMARY_SOFT)
                    .border_radius([6.0; 4])
                    .build(ui);

                // Display input value with embedded cursor
                let text_x = 38.0;
                let text_y = 120.0;
                let cursor_pos = self.name_input.cursor.min(self.name_input.value.len());
                let before = &self.name_input.value[..cursor_pos];
                let after = &self.name_input.value[cursor_pos..];
                let display_text = format!("{}|{}", before, after);

                nuon::label()
                    .x(text_x)
                    .y(text_y)
                    .text(&display_text)
                    .font_size(16.0)
                    .color(nuon::theme::TEXT)
                    .text_justify(nuon::TextJustify::Left)
                    .size(panel_w - 76.0, 30.0)
                    .build(ui);

                nuon::translate()
                    .pos(
                        nuon::center_x(panel_w, button_w * 2.0 + button_gap),
                        panel_h - 90.0,
                    )
                    .build(ui, |ui| {
                        if neo_btn()
                            .size(button_w, button_h)
                            .label("Cancel")
                            .centered()
                            .build(ui)
                        {
                            self.cancel_name_entry();
                            self.state.go_back();
                        }

                        nuon::translate()
                            .x(button_w + button_gap)
                            .add_to_current(ui);

                        if neo_btn()
                            .size(button_w, button_h)
                            .label("Save")
                            .primary()
                            .centered()
                            .build(ui)
                        {
                            if self.confirm_name_entry(ctx) {
                                self.name_input.clear();
                                self.state.go_back();
                            }
                        }
                    });
            });
    }

    fn cancel_name_entry(&mut self) {
        if self.renaming_stored_name.is_none() {
            if let Some(pending) = self.state.pending_import.take() {
                let _ = std::fs::remove_file(&pending.stored_path);
            }
        }
        self.renaming_stored_name = None;
        self.name_input.clear();
    }

    fn confirm_name_entry(&mut self, ctx: &mut Context) -> bool {
        if let Some(rename_stored_name) = self.renaming_stored_name.take() {
            if ctx
                .config
                .update_midi_entry_name(&rename_stored_name, self.name_input.value.clone())
            {
                ctx.config.save();
                true
            } else {
                log::warn!("Failed to update MIDI entry name: stored_name not found");
                false
            }
        } else if let Some(pending) = self.state.pending_import.take() {
            ctx.config.add_midi_to_library(pending.entry.clone());
            ctx.config.save();
            true
        } else {
            false
        }
    }
}

impl Scene for MenuScene {
    #[profiling::function]
    fn update(&mut self, ctx: &mut Context, delta: Duration) {
        self.quad_pipeline.clear();
        self.bg_pipeline.update_time(delta);
        self.audio_spinner_phase = (self.audio_spinner_phase + delta.as_secs_f32() * 8.0) % 4.0;
        self.state.tick(ctx);

        self.futures
            .retain_mut(|f| match f.as_mut().poll(&mut self.context) {
                std::task::Poll::Ready(msg) => {
                    msg(&mut self.state, ctx);
                    false
                }
                std::task::Poll::Pending => true,
            });

        self.state.tick(ctx);

        self.main_ui(ctx);

        super::render_nuon(&mut self.nuon, &mut self.nuon_renderer, ctx);

        self.text_renderer.update(
            ctx.window_state.physical_size,
            ctx.window_state.scale_factor as f32,
        );
        self.quad_pipeline.prepare();
    }

    #[profiling::function]
    fn render<'pass>(&'pass mut self, rpass: &mut wgpu_jumpstart::RenderPass<'pass>) {
        self.bg_pipeline.render(rpass);
        self.quad_pipeline.render(rpass);
        self.text_renderer.render(rpass);
        self.nuon_renderer.render(rpass);
    }

    fn window_event(&mut self, ctx: &mut Context, event: &WindowEvent) {
        if let WindowEvent::MouseWheel { delta, .. } = event {
            match delta {
                winit::event::MouseScrollDelta::LineDelta(_, y) => {
                    let y = y * 60.0;
                    self.settings_scroll.update(y);
                    self.tracks_scroll.update(y);
                }
                winit::event::MouseScrollDelta::PixelDelta(position) => {
                    self.settings_scroll.update(position.y as f32);
                    self.tracks_scroll.update(position.y as f32);
                }
            }
        }

        if event.cursor_moved() {
            self.nuon.mouse_move(
                ctx.window_state.cursor_logical_position.x,
                ctx.window_state.cursor_logical_position.y,
            );
        } else if event.left_mouse_pressed() {
            self.nuon.mouse_down();
        } else if event.left_mouse_released() {
            self.nuon.mouse_up();
        } else if event.back_mouse_pressed() {
            self.state.go_back();
        }

        match self.state.current() {
            Page::Main => {
                if event.key_pressed(Key::Named(NamedKey::Enter)) {
                    self.state.go_to(Page::Library);
                }

                if event.key_pressed(Key::Named(NamedKey::Escape)) {
                    ctx.proxy.send_event(NeothesiaEvent::Exit).ok();
                }

                if event.key_pressed(Key::Character("s")) {
                    self.state.go_to(Page::Settings);
                }

                if event.key_pressed(Key::Character("f")) {
                    state::freeplay(&self.state, ctx);
                }

                if event.key_pressed(Key::Character("a")) {
                    self.futures.push(open_audio_file_picker(&mut self.state));
                }
            }
            Page::Settings => {
                if event.key_pressed(Key::Named(NamedKey::Escape)) {
                    self.state.go_back();
                }
            }
            Page::Library => {
                if event.key_pressed(Key::Named(NamedKey::Tab)) {
                    self.futures.push(open_midi_file_picker(&mut self.state));
                }

                if event.key_pressed(Key::Named(NamedKey::Escape)) {
                    self.state.go_back();
                }
            }
            Page::AudioImport => {
                if event.key_pressed(Key::Named(NamedKey::Enter))
                    && !self.state.audio_import.is_converting()
                    && let Some(future) = convert_selected_audio(&mut self.state)
                {
                    self.futures.push(future);
                }

                if event.key_pressed(Key::Named(NamedKey::Escape))
                    && !self.state.audio_import.is_converting()
                {
                    self.state.audio_import = AudioImportState::Empty;
                    self.state.go_back();
                }
            }
            Page::NameEntry => {
                if event.key_pressed(Key::Named(NamedKey::Enter)) {
                    if self.confirm_name_entry(ctx) {
                        self.name_input.clear();
                        self.state.go_back();
                    }
                }

                if event.key_pressed(Key::Named(NamedKey::Escape)) {
                    self.cancel_name_entry();
                    self.state.go_back();
                }

                // Handle text input
                match event {
                    WindowEvent::KeyboardInput { event, .. } => {
                        use winit::keyboard::Key;
                        if event.state == winit::event::ElementState::Pressed {
                            match &event.logical_key {
                                Key::Named(named_key) => match named_key {
                                    NamedKey::Backspace => {
                                        self.name_input.backspace();
                                    }
                                    NamedKey::Delete => {
                                        self.name_input.delete();
                                    }
                                    NamedKey::ArrowLeft => {
                                        self.name_input.move_cursor_left();
                                    }
                                    NamedKey::ArrowRight => {
                                        self.name_input.move_cursor_right();
                                    }
                                    NamedKey::Home => {
                                        self.name_input.move_cursor_home();
                                    }
                                    NamedKey::End => {
                                        self.name_input.move_cursor_end();
                                    }
                                    NamedKey::Space => {
                                        self.name_input.insert_char(' ');
                                    }
                                    _ => {}
                                },
                                Key::Character(s) => {
                                    // Handle character input
                                    for c in s.chars() {
                                        if !c.is_control() {
                                            self.name_input.insert_char(c);
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    _ => {}
                }
            }
            Page::TrackSelection => {
                if event.key_pressed(Key::Named(NamedKey::Enter)) {
                    state::play(&self.state, ctx);
                }

                if event.key_pressed(Key::Named(NamedKey::Escape)) {
                    self.state.go_back();
                }
            }
            Page::PlayConfirm => {
                if event.key_pressed(Key::Named(NamedKey::Enter)) {
                    state::play(&self.state, ctx);
                }

                if event.key_pressed(Key::Character("l")) {
                    self.state.go_to(Page::TrackSelection);
                }

                if event.key_pressed(Key::Named(NamedKey::Escape)) {
                    self.state.go_back();
                }
            }
        }
    }
}

fn noop_waker_ref() -> &'static Waker {
    use std::{
        ptr::null,
        task::{RawWaker, RawWakerVTable},
    };

    unsafe fn noop_clone(_data: *const ()) -> RawWaker {
        noop_raw_waker()
    }

    unsafe fn noop(_data: *const ()) {}

    const NOOP_WAKER_VTABLE: RawWakerVTable = RawWakerVTable::new(noop_clone, noop, noop, noop);

    const fn noop_raw_waker() -> RawWaker {
        RawWaker::new(null(), &NOOP_WAKER_VTABLE)
    }

    struct SyncRawWaker(RawWaker);
    unsafe impl Sync for SyncRawWaker {}

    static NOOP_WAKER_INSTANCE: SyncRawWaker = SyncRawWaker(noop_raw_waker());

    // SAFETY: `Waker` is #[repr(transparent)] over its `RawWaker`.
    unsafe { &*(&NOOP_WAKER_INSTANCE.0 as *const RawWaker as *const Waker) }
}
