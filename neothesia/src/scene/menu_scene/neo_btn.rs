#[derive(Clone, Copy, Default)]
enum NeoBtnVariant {
    Primary,
    Danger,
    #[default]
    Secondary,
}

#[derive(Clone, Copy, Default)]
enum NeoBtnAlignment {
    #[default]
    Leading,
    Centered,
}

#[derive(Clone, Copy)]
struct NeoBtnPalette {
    border: nuon::Color,
    fill: nuon::Color,
    fill_hover: nuon::Color,
    chip: nuon::Color,
    chip_hover: nuon::Color,
    icon: nuon::Color,
    title: nuon::Color,
    subtitle: nuon::Color,
    meta_fill: nuon::Color,
    meta_text: nuon::Color,
}

impl NeoBtnVariant {
    fn palette(&self) -> NeoBtnPalette {
        match self {
            NeoBtnVariant::Primary => NeoBtnPalette {
                border: nuon::Color::new_u8(197, 214, 242, 1.0),
                fill: nuon::theme::PRIMARY_SOFT,
                fill_hover: nuon::Color::new_u8(236, 243, 253, 1.0),
                chip: nuon::theme::PRIMARY,
                chip_hover: nuon::theme::PRIMARY_HOVER,
                icon: nuon::Color::WHITE,
                title: nuon::Color::new_u8(19, 43, 89, 1.0),
                subtitle: nuon::Color::new_u8(71, 95, 137, 1.0),
                meta_fill: nuon::Color::new_u8(243, 247, 255, 1.0),
                meta_text: nuon::theme::PRIMARY,
            },
            NeoBtnVariant::Danger => NeoBtnPalette {
                border: nuon::Color::new_u8(236, 209, 215, 1.0),
                fill: nuon::theme::DANGER_SOFT,
                fill_hover: nuon::Color::new_u8(252, 240, 243, 1.0),
                chip: nuon::Color::new_u8(255, 245, 247, 1.0),
                chip_hover: nuon::Color::new_u8(255, 241, 244, 1.0),
                icon: nuon::theme::DANGER,
                title: nuon::Color::new_u8(104, 37, 49, 1.0),
                subtitle: nuon::Color::new_u8(132, 74, 84, 1.0),
                meta_fill: nuon::Color::new_u8(255, 244, 246, 1.0),
                meta_text: nuon::theme::DANGER,
            },
            NeoBtnVariant::Secondary => NeoBtnPalette {
                border: nuon::theme::DIVIDER,
                fill: nuon::theme::SURFACE,
                fill_hover: nuon::theme::SURFACE_HOVER,
                chip: nuon::theme::SURFACE_ELEVATED,
                chip_hover: nuon::Color::new_u8(236, 242, 249, 1.0),
                icon: nuon::theme::PRIMARY,
                title: nuon::theme::TEXT,
                subtitle: nuon::theme::TEXT_MUTED,
                meta_fill: nuon::theme::SURFACE_ELEVATED,
                meta_text: nuon::theme::TEXT_MUTED,
            },
        }
    }
}

pub struct NeoBtn {
    id: Option<nuon::Id>,
    size: nuon::Size,
    label: String,
    subtitle: String,
    meta: String,
    icon: String,
    variant: NeoBtnVariant,
    alignment: NeoBtnAlignment,
}

impl NeoBtn {
    pub fn new() -> Self {
        Self {
            id: None,
            size: Default::default(),
            label: Default::default(),
            subtitle: Default::default(),
            meta: Default::default(),
            icon: Default::default(),
            variant: Default::default(),
            alignment: Default::default(),
        }
    }

    #[allow(unused)]
    pub fn id(mut self, id: impl Into<nuon::Id>) -> Self {
        self.id = Some(id.into());
        self
    }

    pub fn size(mut self, width: f32, height: f32) -> Self {
        self.size = (width, height).into();
        self
    }

    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    #[allow(dead_code)]
    pub fn subtitle(mut self, subtitle: impl Into<String>) -> Self {
        self.subtitle = subtitle.into();
        self
    }

    pub fn meta(mut self, meta: impl Into<String>) -> Self {
        self.meta = meta.into();
        self
    }

    pub fn icon(mut self, icon: impl Into<String>) -> Self {
        self.icon = icon.into();
        self
    }

    pub fn primary(mut self) -> Self {
        self.variant = NeoBtnVariant::Primary;
        self
    }

    pub fn danger(mut self) -> Self {
        self.variant = NeoBtnVariant::Danger;
        self
    }

    pub fn centered(mut self) -> Self {
        self.alignment = NeoBtnAlignment::Centered;
        self
    }

    fn text_justify(&self) -> nuon::TextJustify {
        match self.alignment {
            NeoBtnAlignment::Leading => nuon::TextJustify::Left,
            NeoBtnAlignment::Centered => nuon::TextJustify::Center,
        }
    }

    pub fn build(&self, ui: &mut nuon::Ui) -> bool {
        let w = self.size.width;
        let h = self.size.height;
        let compact = h < 74.0 || w < 250.0;
        let radius = if compact { 15.0 } else { 18.0 };
        let palette = self.variant.palette();

        let id = if let Some(id) = self.id {
            id
        } else if self.icon.is_empty() {
            nuon::Id::hash(&self.label)
        } else {
            nuon::Id::hash((&self.label, &self.icon, &self.meta))
        };

        let event = nuon::click_area(id).size(w, h).build(ui);
        let hovered = event.is_hovered() || event.is_pressed();

        let fill = if hovered {
            palette.fill_hover
        } else {
            palette.fill
        };
        let chip = if hovered {
            palette.chip_hover
        } else {
            palette.chip
        };

        nuon::quad()
            .size(w, h)
            .color(palette.border)
            .border_radius([radius; 4])
            .build(ui);
        nuon::quad()
            .pos(1.0, 1.0)
            .size(w - 2.0, h - 2.0)
            .color(fill)
            .border_radius([radius - 1.0; 4])
            .build(ui);

        if self.label.is_empty() {
            let chip_size = (w.min(h) - 16.0).max(22.0);

            nuon::quad()
                .size(chip_size, chip_size)
                .pos(nuon::center_x(w, chip_size), nuon::center_y(h, chip_size))
                .color(chip)
                .border_radius([14.0; 4])
                .build(ui);

            nuon::label()
                .size(self.size.width, self.size.height)
                .font_size((chip_size * 0.42).max(16.0))
                .color(palette.icon)
                .icon(&self.icon)
                .build(ui);

            return event.is_clicked();
        }

        let pad_x = if compact { 16.0 } else { 18.0 };
        let pad_y = if compact { 12.0 } else { 14.0 };
        let icon_size = if compact { 32.0 } else { 36.0 };
        let mut content_x = pad_x;

        if !self.icon.is_empty() {
            nuon::quad()
                .size(icon_size, icon_size)
                .pos(content_x, nuon::center_y(h, icon_size))
                .color(chip)
                .border_radius([13.0; 4])
                .build(ui);
            nuon::label()
                .pos(content_x, 0.0)
                .size(icon_size, h)
                .font_size(if compact { 16.0 } else { 18.0 })
                .color(palette.icon)
                .icon(&self.icon)
                .build(ui);
            content_x += icon_size + if compact { 10.0 } else { 12.0 };
        }

        let meta_w = if self.meta.is_empty() {
            0.0
        } else if compact {
            48.0
        } else {
            54.0
        };
        let meta_h = if compact { 22.0 } else { 24.0 };
        let meta_x = w - pad_x - meta_w;
        let text_right = if self.meta.is_empty() {
            w - pad_x
        } else {
            meta_x - 12.0
        };
        let text_w = (text_right - content_x).max(96.0);

        if !self.meta.is_empty() {
            nuon::quad()
                .size(meta_w, meta_h)
                .pos(meta_x, nuon::center_y(h, meta_h))
                .color(palette.meta_fill)
                .border_radius([10.0; 4])
                .build(ui);
            nuon::label()
                .pos(meta_x, nuon::center_y(h, meta_h))
                .size(meta_w, meta_h)
                .font_size(if compact { 10.5 } else { 11.0 })
                .bold(true)
                .color(palette.meta_text)
                .text(&self.meta)
                .build(ui);
        }

        let has_subtitle = !self.subtitle.is_empty();
        let title_y = if has_subtitle { pad_y } else { 0.0 };
        let title_h = if has_subtitle {
            if compact { 20.0 } else { 24.0 }
        } else {
            h
        };
        let title_x = if matches!(self.alignment, NeoBtnAlignment::Centered) {
            0.0
        } else {
            content_x
        };
        let title_w = if matches!(self.alignment, NeoBtnAlignment::Centered) {
            w
        } else {
            text_w
        };
        let justify = self.text_justify();

        nuon::label()
            .pos(title_x, title_y)
            .size(title_w, title_h)
            .font_size(if has_subtitle {
                if compact { 15.0 } else { 18.0 }
            } else if compact {
                16.0
            } else {
                18.5
            })
            .bold(true)
            .text_justify(justify)
            .color(palette.title)
            .text(&self.label)
            .build(ui);

        if has_subtitle {
            let subtitle_x = if matches!(self.alignment, NeoBtnAlignment::Centered) {
                0.0
            } else {
                content_x
            };
            let subtitle_w = if matches!(self.alignment, NeoBtnAlignment::Centered) {
                w
            } else {
                text_w
            };
            nuon::label()
                .pos(
                    subtitle_x,
                    if compact { pad_y + 20.0 } else { pad_y + 24.0 },
                )
                .size(subtitle_w, if compact { 15.0 } else { 16.0 })
                .font_size(if compact { 10.5 } else { 11.5 })
                .text_justify(justify)
                .color(palette.subtitle)
                .text(&self.subtitle)
                .build(ui);
        }

        event.is_clicked()
    }
}

pub fn neo_btn() -> NeoBtn {
    NeoBtn::new()
}

pub fn neo_btn_icon(ui: &mut nuon::Ui, w: f32, h: f32, icon: &str) -> bool {
    NeoBtn::new().size(w, h).icon(icon).build(ui)
}
