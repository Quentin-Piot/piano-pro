use std::hash::Hash;
use std::time::{Duration, Instant};

use crate::{NeothesiaEvent, context::Context, icons, song::PlayerConfig};

use super::{
    PlayingScene,
    animation::{Animated, Easing},
};

pub struct TopBar {
    pub topbar_expand_animation: Animated<bool, Instant>,
    is_expanded: bool,

    settings_animation: Animated<bool, Instant>,

    pub settings_active: bool,

    looper_active: bool,
    loop_start: Duration,
    loop_end: Duration,

    pub tracks_scroll: nuon::ScrollState,
}

impl TopBar {
    pub fn new() -> Self {
        Self {
            topbar_expand_animation: Animated::new(false)
                .duration(1000.)
                .easing(Easing::EaseOutExpo)
                .delay(30.0),
            settings_animation: Animated::new(false)
                .duration(1000.)
                .easing(Easing::EaseOutExpo)
                .delay(30.0),

            is_expanded: false,
            settings_active: false,

            looper_active: false,
            loop_start: Duration::ZERO,
            loop_end: Duration::ZERO,

            tracks_scroll: nuon::ScrollState::new(),
        }
    }

    pub fn is_looper_active(&self) -> bool {
        self.looper_active
    }

    pub fn loop_start_timestamp(&self) -> Duration {
        self.loop_start
    }

    pub fn loop_end_timestamp(&self) -> Duration {
        self.loop_end
    }

    #[profiling::function]
    pub fn update(scene: &mut PlayingScene, ctx: &mut Context) {
        let PlayingScene { top_bar, .. } = scene;

        top_bar.is_expanded = true;

        top_bar
            .topbar_expand_animation
            .transition(top_bar.is_expanded, ctx.frame_timestamp);
        top_bar
            .settings_animation
            .transition(top_bar.settings_active, ctx.frame_timestamp);

        Self::ui(scene, ctx);
    }

    #[profiling::function]
    pub fn ui(this: &mut PlayingScene, ctx: &mut Context) {
        let mut ui = std::mem::replace(&mut this.nuon, nuon::Ui::new());

        nuon::translate()
            .y(this.top_bar.topbar_expand_animation.animate_bool(
                -75.0 + 5.0,
                0.0,
                ctx.frame_timestamp,
            ))
            .build(&mut ui, |ui| {
                Self::panel(this, ctx, ui);
            });

        if this.top_bar.settings_active {
            Self::settings_panel(this, ctx, &mut ui);
        }

        this.nuon = ui;
    }

    fn panel(this: &mut PlayingScene, ctx: &mut Context, ui: &mut nuon::Ui) {
        let win_w = ctx.window_state.logical_size.width;

        nuon::quad()
            .size(win_w, 30.0 + 45.0)
            .color(nuon::theme::PANEL)
            .build(ui);

        Self::panel_left(this, ctx, ui);
        Self::panel_center(this, ctx, ui);
        Self::panel_right(this, ctx, ui);

        // ProggressBar
        nuon::translate().y(30.0).build(ui, |ui| {
            Self::proggress_bar(this, ctx, ui);
        });
    }

    fn button() -> nuon::Button {
        nuon::button()
            .size(30.0, 30.0)
            .color(nuon::theme::SURFACE_ELEVATED)
            .hover_color(nuon::theme::SURFACE_HOVER)
            .preseed_color(nuon::theme::SURFACE_PRESSED)
            .border_radius([5.0; 4])
    }

    fn panel_left(this: &mut PlayingScene, ctx: &mut Context, ui: &mut nuon::Ui) {
        if Self::button().icon(icons::left_arrow_icon()).build(ui) {
            ctx.proxy
                .send_event(NeothesiaEvent::MainMenu(Some(this.player.song().clone())))
                .ok();
        }
    }

    fn panel_center(_this: &mut PlayingScene, ctx: &mut Context, ui: &mut nuon::Ui) {
        let win_w = ctx.window_state.logical_size.width;
        let pill_w = 45.0 * 2.0;

        nuon::translate()
            .x(win_w / 2.0 - pill_w / 2.0)
            .y(5.0)
            .build(ui, |ui| {
                if nuon::button()
                    .size(45.0, 20.0)
                    .color(nuon::theme::SURFACE_ELEVATED)
                    .hover_color(nuon::theme::SURFACE_HOVER)
                    .preseed_color(nuon::theme::SURFACE_PRESSED)
                    .border_radius([10.0, 0.0, 0.0, 10.0])
                    .icon(icons::minus_icon())
                    .text_justify(nuon::TextJustify::Left)
                    .build(ui)
                {
                    ctx.config
                        .set_speed_multiplier(ctx.config.speed_multiplier() - 0.1);
                }

                nuon::label()
                    .text(format!(
                        "{}%",
                        (ctx.config.speed_multiplier() * 100.0).round()
                    ))
                    .bold(true)
                    .size(45.0 * 2.0, 20.0)
                    .build(ui);

                if nuon::button()
                    .size(45.0, 20.0)
                    .x(45.0)
                    .color(nuon::theme::SURFACE_ELEVATED)
                    .hover_color(nuon::theme::SURFACE_HOVER)
                    .preseed_color(nuon::theme::SURFACE_PRESSED)
                    .border_radius([0.0, 10.0, 10.0, 0.0])
                    .icon(icons::plus_icon())
                    .text_justify(nuon::TextJustify::Right)
                    .build(ui)
                {
                    ctx.config
                        .set_speed_multiplier(ctx.config.speed_multiplier() + 0.1);
                }
            });
    }

    fn panel_right(this: &mut PlayingScene, ctx: &mut Context, ui: &mut nuon::Ui) {
        nuon::translate()
            .x(ctx.window_state.logical_size.width)
            .build(ui, |ui| {
                nuon::translate().x(-30.0).add_to_current(ui);

                if Self::button()
                    .icon(if this.top_bar.settings_active {
                        icons::gear_fill_icon()
                    } else {
                        icons::gear_icon()
                    })
                    .build(ui)
                {
                    this.top_bar.settings_active = !this.top_bar.settings_active;
                }

                nuon::translate().x(-30.0).add_to_current(ui);

                if Self::button().icon(icons::repeat_icon()).build(ui) {
                    this.top_bar.looper_active = !this.top_bar.looper_active;

                    // Looper enabled for the first time
                    if this.top_bar.looper_active
                        && this.top_bar.loop_start.is_zero()
                        && this.top_bar.loop_end.is_zero()
                    {
                        this.top_bar.loop_start = this.player.time();
                        this.top_bar.loop_end = this.player.time() + Duration::from_secs(5);
                    }
                }

                nuon::translate().x(-30.0).add_to_current(ui);

                if Self::button()
                    .icon(if this.player.is_paused() {
                        icons::play_icon()
                    } else {
                        icons::pause_icon()
                    })
                    .build(ui)
                {
                    this.player.pause_resume();
                }
            });
    }

    fn settings_panel(this: &mut PlayingScene, ctx: &mut Context, ui: &mut nuon::Ui) {
        let win_w = ctx.window_state.logical_size.width;
        let win_h = ctx.window_state.logical_size.height;

        const PADDING: f32 = 28.0;
        const ROW_H: f32 = 68.0;
        const ROW_GAP: f32 = 10.0;
        const CHIP_SIZE: f32 = 36.0;
        const PILL_W: f32 = 54.0;
        const PILL_H: f32 = 30.0;

        // Pre-compute all data from song BEFORE any closures to avoid borrow conflicts.
        let file_tracks = this.player.song().file.tracks.clone();
        let track_configs: Vec<crate::song::TrackConfig> =
            this.player.song().config.tracks.to_vec();
        let track_ids: Vec<usize> = file_tracks
            .iter()
            .filter(|t| !t.notes.is_empty())
            .map(|t| t.track_id)
            .collect();
        let track_count = track_ids.len();
        let color_schema = ctx.config.color_schema().to_vec();

        // Panel sizing
        let panel_w = (win_w - 64.0).clamp(440.0, 580.0);
        let header_h = 116.0;
        let rows_h = if track_count == 0 {
            0.0
        } else {
            track_count as f32 * (ROW_H + ROW_GAP) - ROW_GAP
        };
        let close_h = 52.0;
        let ideal_h = PADDING + header_h + rows_h + PADDING + close_h + PADDING;
        let panel_h = ideal_h.min(win_h * 0.82);
        let panel_x = nuon::center_x(win_w, panel_w).max(0.0);
        let panel_y = nuon::center_y(win_h, panel_h).max(75.0);

        #[derive(Debug)]
        enum Ev {
            PlayerConfig(usize, PlayerConfig),
            ToggleVisible(usize),
            Close,
        }
        let mut events: Vec<Ev> = Vec::new();

        nuon::layer().overlay(true).build(ui, |ui| {
            // Backdrop
            nuon::quad()
                .size(win_w, win_h)
                .color(nuon::Color::new(0.0, 0.0, 0.0, 0.45))
                .build(ui);

            nuon::translate().pos(panel_x, panel_y).build(ui, |ui| {
                // Panel: border + fill (draw_card style)
                nuon::quad()
                    .size(panel_w, panel_h)
                    .color(nuon::theme::DIVIDER)
                    .border_radius([28.0; 4])
                    .build(ui);
                nuon::quad()
                    .pos(1.0, 1.0)
                    .size(panel_w - 2.0, panel_h - 2.0)
                    .color(nuon::theme::PANEL)
                    .border_radius([27.0; 4])
                    .build(ui);

                // Header
                nuon::translate().pos(PADDING, PADDING).build(ui, |ui| {
                    nuon::quad()
                        .size(74.0, 24.0)
                        .color(nuon::theme::PRIMARY_SOFT)
                        .border_radius([12.0; 4])
                        .build(ui);
                    nuon::label()
                        .size(74.0, 24.0)
                        .font_size(11.0)
                        .bold(true)
                        .color(nuon::theme::PRIMARY)
                        .text("TRACKS")
                        .build(ui);

                    nuon::label()
                        .y(36.0)
                        .text("Track Controls")
                        .size(panel_w - PADDING * 2.0, 38.0)
                        .font_size(28.0)
                        .bold(true)
                        .text_justify(nuon::TextJustify::Left)
                        .build(ui);

                    nuon::label()
                        .y(80.0)
                        .text(format!(
                            "{} active track{}",
                            track_count,
                            if track_count == 1 { "" } else { "s" }
                        ))
                        .size(panel_w - PADDING * 2.0, 20.0)
                        .font_size(13.0)
                        .color(nuon::theme::TEXT_MUTED)
                        .text_justify(nuon::TextJustify::Left)
                        .build(ui);
                });

                // Scrollable track rows
                let rows_area_h = panel_h - PADDING - header_h - PADDING - close_h - PADDING;

                nuon::translate().y(PADDING + header_h).build(ui, |ui| {
                    let scroll_state = this.top_bar.tracks_scroll;
                    let new_scroll = nuon::scroll()
                        .scissor_size(panel_w, rows_area_h)
                        .scroll(scroll_state)
                        .build(ui, |ui| {
                            for (i, &track_id) in track_ids.iter().enumerate() {
                                nuon::translate().x(PADDING).build(ui, |ui| {
                                    let row_w = panel_w - PADDING * 2.0;
                                    let config = &track_configs[track_id];
                                    let track = &file_tracks[track_id];

                                    let is_muted = config.player == PlayerConfig::Mute;
                                    let track_color = if is_muted || !config.visible {
                                        nuon::Color::new_u8(140, 140, 150, 1.0)
                                    } else {
                                        let color_id = track.track_color_id % color_schema.len();
                                        let c = &color_schema[color_id].base;
                                        nuon::Color::new_u8(c.0, c.1, c.2, 1.0)
                                    };
                                    let chip_hover = nuon::Color::new(
                                        (track_color.r + 0.08).min(1.0),
                                        (track_color.g + 0.08).min(1.0),
                                        (track_color.b + 0.08).min(1.0),
                                        1.0,
                                    );

                                    // Row: border + fill
                                    nuon::quad()
                                        .size(row_w, ROW_H)
                                        .color(nuon::theme::DIVIDER)
                                        .border_radius([18.0; 4])
                                        .build(ui);
                                    nuon::quad()
                                        .pos(1.0, 1.0)
                                        .size(row_w - 2.0, ROW_H - 2.0)
                                        .color(nuon::theme::SURFACE)
                                        .border_radius([17.0; 4])
                                        .build(ui);

                                    // Color chip (visibility toggle)
                                    let chip_x = 14.0;
                                    let chip_y = nuon::center_y(ROW_H, CHIP_SIZE);

                                    let vis_ev = nuon::click_area(nuon::Id::hash_with(|h| {
                                        "svis".hash(h);
                                        i.hash(h);
                                    }))
                                    .pos(chip_x, chip_y)
                                    .size(CHIP_SIZE, CHIP_SIZE)
                                    .build(ui);

                                    let drawn_chip = if vis_ev.is_hovered() || vis_ev.is_pressed() {
                                        chip_hover
                                    } else {
                                        track_color
                                    };

                                    nuon::quad()
                                        .pos(chip_x, chip_y)
                                        .size(CHIP_SIZE, CHIP_SIZE)
                                        .color(drawn_chip)
                                        .border_radius([CHIP_SIZE / 2.0; 4])
                                        .build(ui);
                                    nuon::label()
                                        .pos(chip_x, chip_y)
                                        .size(CHIP_SIZE, CHIP_SIZE)
                                        .font_size(CHIP_SIZE * 0.42)
                                        .color(nuon::Color::WHITE)
                                        .icon(icons::note_list_icon())
                                        .build(ui);

                                    if vis_ev.is_clicked() {
                                        events.push(Ev::ToggleVisible(track_id));
                                    }

                                    // Pill buttons: Mute / Auto / Human
                                    let pills_w = PILL_W * 3.0;
                                    let pill_x = row_w - pills_w - 14.0;
                                    let pill_y = nuon::center_y(ROW_H, PILL_H);

                                    let reg = nuon::theme::SURFACE_ELEVATED;
                                    let reg_h = nuon::theme::SURFACE_HOVER;

                                    let pill_configs: &[(PlayerConfig, &str, [f32; 4])] = &[
                                        (
                                            PlayerConfig::Mute,
                                            "Mute",
                                            [PILL_H / 2.0, 0.0, 0.0, PILL_H / 2.0],
                                        ),
                                        (PlayerConfig::Auto, "Auto", [0.0; 4]),
                                        (
                                            PlayerConfig::Human,
                                            "Human",
                                            [0.0, PILL_H / 2.0, PILL_H / 2.0, 0.0],
                                        ),
                                    ];

                                    for (idx, (mode, label, radius)) in
                                        pill_configs.iter().enumerate()
                                    {
                                        let px = pill_x + idx as f32 * PILL_W;
                                        let active = config.player == *mode;
                                        let pill_ev = nuon::click_area(nuon::Id::hash_with(|h| {
                                            "spill".hash(h);
                                            i.hash(h);
                                            idx.hash(h);
                                        }))
                                        .pos(px, pill_y)
                                        .size(PILL_W, PILL_H)
                                        .build(ui);

                                        let bg = if active {
                                            if pill_ev.is_hovered() || pill_ev.is_pressed() {
                                                chip_hover
                                            } else {
                                                track_color
                                            }
                                        } else if pill_ev.is_hovered() || pill_ev.is_pressed() {
                                            reg_h
                                        } else {
                                            reg
                                        };

                                        nuon::quad()
                                            .pos(px, pill_y)
                                            .size(PILL_W, PILL_H)
                                            .color(bg)
                                            .border_radius(*radius)
                                            .build(ui);

                                        let text_color = if active {
                                            nuon::Color::WHITE
                                        } else {
                                            nuon::theme::TEXT
                                        };

                                        nuon::label()
                                            .pos(px, pill_y)
                                            .size(PILL_W, PILL_H)
                                            .font_size(11.0)
                                            .bold(true)
                                            .color(text_color)
                                            .text(*label)
                                            .build(ui);

                                        if pill_ev.is_clicked() {
                                            events.push(Ev::PlayerConfig(track_id, *mode));
                                        }
                                    }

                                    // Track title + subtitle
                                    let text_x = chip_x + CHIP_SIZE + 12.0;
                                    let text_w = (pill_x - text_x - 10.0).max(40.0);

                                    let title = if track.has_drums && !track.has_other_than_drums {
                                        "Percussion"
                                    } else {
                                        let instrument_id = track
                                            .programs
                                            .last()
                                            .map(|p| p.program as usize)
                                            .unwrap_or(0);
                                        midi_file::INSTRUMENT_NAMES[instrument_id]
                                    };

                                    nuon::label()
                                        .pos(text_x, nuon::center_y(ROW_H, 38.0))
                                        .size(text_w, 20.0)
                                        .font_size(15.0)
                                        .bold(true)
                                        .text_justify(nuon::TextJustify::Left)
                                        .color(nuon::theme::TEXT)
                                        .text(title)
                                        .build(ui);

                                    nuon::label()
                                        .pos(text_x, nuon::center_y(ROW_H, 38.0) + 22.0)
                                        .size(text_w, 16.0)
                                        .font_size(11.5)
                                        .text_justify(nuon::TextJustify::Left)
                                        .color(nuon::theme::TEXT_MUTED)
                                        .text(format!("{} notes", track.notes.len()))
                                        .build(ui);
                                });

                                nuon::translate().y(ROW_H + ROW_GAP).add_to_current(ui);
                            }
                        });
                    this.top_bar.tracks_scroll = new_scroll;
                });

                // Close button (draw_card style, secondary)
                let close_y = panel_h - PADDING - close_h;
                let close_w = panel_w - PADDING * 2.0;

                nuon::translate().pos(PADDING, close_y).build(ui, |ui| {
                    let close_ev = nuon::click_area("settings_close")
                        .size(close_w, close_h)
                        .build(ui);

                    let hovered = close_ev.is_hovered() || close_ev.is_pressed();
                    let fill = if hovered {
                        nuon::theme::SURFACE_HOVER
                    } else {
                        nuon::theme::SURFACE
                    };

                    nuon::quad()
                        .size(close_w, close_h)
                        .color(nuon::theme::DIVIDER)
                        .border_radius([16.0; 4])
                        .build(ui);
                    nuon::quad()
                        .pos(1.0, 1.0)
                        .size(close_w - 2.0, close_h - 2.0)
                        .color(fill)
                        .border_radius([15.0; 4])
                        .build(ui);

                    nuon::label()
                        .size(close_w, close_h)
                        .font_size(15.0)
                        .bold(true)
                        .color(nuon::theme::TEXT)
                        .text("Close")
                        .build(ui);

                    if close_ev.is_clicked() {
                        events.push(Ev::Close);
                    }
                });
            });
        });

        // Apply events
        let mut needs_rebuild = false;
        for ev in events {
            match ev {
                Ev::Close => {
                    this.top_bar.settings_active = false;
                }
                Ev::ToggleVisible(track_id) => {
                    let track = &mut this.player.song_mut().config.tracks[track_id];
                    // Don't allow toggling visibility when muted (mute controls visibility)
                    if track.player != PlayerConfig::Mute {
                        track.visible = !track.visible;
                        needs_rebuild = true;
                    }
                }
                Ev::PlayerConfig(track_id, player) => {
                    let track = &mut this.player.song_mut().config.tracks[track_id];
                    track.player = player;
                    // Mute hides visually; Auto/Human shows
                    let should_be_visible = player != PlayerConfig::Mute;
                    if track.visible != should_be_visible {
                        track.visible = should_be_visible;
                        needs_rebuild = true;
                    }
                    // Stop any notes currently held by this track so they don't hang.
                    if player == PlayerConfig::Mute {
                        this.player.stop_all_output();
                    }
                }
            }
        }

        if needs_rebuild {
            let hidden_tracks: Vec<usize> = this
                .player
                .song()
                .config
                .tracks
                .iter()
                .filter(|t| !t.visible)
                .map(|t| t.track_id)
                .collect();
            let tracks = this.player.song().file.tracks.clone();
            this.waterfall.set_notes(
                &tracks,
                &hidden_tracks,
                &ctx.config,
                this.keyboard.layout().clone(),
            );
            // Sync the keyboard's own song_config so file_midi_events skips hidden tracks.
            this.keyboard
                .set_song_config(this.player.song().config.clone());
            // Clear any keys currently lit by the now-hidden tracks.
            this.keyboard.reset_notes();
            // Keep note-label overlay in sync with the new visible set.
            if let Some(note_labels) = this.note_labels.as_mut() {
                note_labels.set_notes(this.waterfall.notes());
            }
        }
    }

    fn proggress_bar(this: &mut PlayingScene, ctx: &mut Context, ui: &mut nuon::Ui) {
        let h = 45.0;
        let w = ctx.window_state.logical_size.width;

        let render_looper = Self::proggress_bar_looper(this, ctx, ui, w, h);

        Self::proggress_bar_bg(this, ctx, ui, w, h);

        render_looper(ui);
    }

    fn proggress_bar_bg(
        this: &mut PlayingScene,
        ctx: &mut Context,
        ui: &mut nuon::Ui,
        w: f32,
        h: f32,
    ) {
        let progress_w = w * this.player.percentage();

        match nuon::click_area("ProggressBar").size(w, h).build(ui) {
            nuon::ClickAreaEvent::PressStart => {
                if !this.rewind_controller.is_rewinding() {
                    this.rewind_controller.start_mouse_rewind(&mut this.player);

                    let x = ctx.window_state.cursor_logical_position.x;
                    let w = ctx.window_state.logical_size.width;

                    let p = x / w;
                    this.player.set_percentage_time(p);
                    this.keyboard.reset_notes();
                }
            }
            nuon::ClickAreaEvent::PressEnd { .. } => {
                this.rewind_controller.stop_rewind(&mut this.player);
            }
            nuon::ClickAreaEvent::Idle { .. } => {}
        }

        nuon::quad()
            .size(progress_w, h)
            .color(nuon::theme::PRIMARY)
            .build(ui);

        for m in this.player.song().file.measures.iter() {
            let length = this.player.length().as_secs_f32();
            let start = this.player.leed_in().as_secs_f32() / length;
            let measure = m.as_secs_f32() / length;

            let x = (start + measure) * w;

            let light_measure = nuon::theme::TEXT_MUTED;
            let dark_measure = nuon::theme::DIVIDER;

            let color = if x < progress_w {
                light_measure
            } else {
                dark_measure
            };

            nuon::quad().x(x).size(1.0, h).color(color).build(ui);
        }
    }

    fn proggress_bar_looper<'a>(
        this: &mut PlayingScene,
        ctx: &mut Context,
        ui: &mut nuon::Ui,
        w: f32,
        h: f32,
    ) -> impl FnOnce(&mut nuon::Ui) + 'a {
        let loop_start = this.top_bar.loop_start;
        let loop_start = this.player.time_to_percentage(&loop_start) * w;

        let loop_end = this.top_bar.loop_end;
        let loop_end = this.player.time_to_percentage(&loop_end) * w;

        let loop_h = h + 10.0;

        let looper_active = this.top_bar.looper_active;

        let (loop_start_ev, loop_end_ev) = if looper_active {
            let loop_start_ev = nuon::click_area("LooperStart")
                .x(loop_start)
                .width(5.0)
                .height(loop_h)
                .build(ui);
            let loop_end_ev = nuon::click_area("LooperEnd")
                .x(loop_end)
                .width(5.0)
                .height(loop_h)
                .build(ui);
            (loop_start_ev, loop_end_ev)
        } else {
            (nuon::ClickAreaEvent::null(), nuon::ClickAreaEvent::null())
        };

        if loop_start_ev.is_pressed() {
            let x = ctx.window_state.cursor_logical_position.x;
            let w = ctx.window_state.logical_size.width;
            let p = x / w;

            if p * w < loop_end - 10.0 {
                this.top_bar.loop_start = this.player.percentage_to_time(p);
            }
        }

        if loop_end_ev.is_pressed() {
            let x = ctx.window_state.cursor_logical_position.x;
            let w = ctx.window_state.logical_size.width;
            let p = x / w;

            if p * w > loop_start + 10.0 {
                this.top_bar.loop_end = this.player.percentage_to_time(p);
            }
        }

        // render
        move |ui| {
            if !looper_active {
                return;
            }

            let color = nuon::theme::PRIMARY;
            let white = nuon::theme::TEXT;

            nuon::quad()
                .x(loop_start)
                .width(loop_end - loop_start)
                .height(loop_h)
                .color(nuon::theme::PRIMARY_SOFT)
                .build(ui);

            nuon::quad()
                .x(loop_start)
                .width(5.0)
                .height(loop_h)
                .color(
                    if loop_start_ev.is_hovered() || loop_start_ev.is_pressed() {
                        white
                    } else {
                        color
                    },
                )
                .build(ui);

            nuon::quad()
                .x(loop_end)
                .width(5.0)
                .height(loop_h)
                .color(if loop_end_ev.is_hovered() || loop_end_ev.is_pressed() {
                    white
                } else {
                    color
                })
                .build(ui);
        }
    }
}
