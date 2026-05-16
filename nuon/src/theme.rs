use crate::Color;

#[derive(Clone, Copy, Eq, PartialEq)]
pub enum FormFactor {
    Phone,
    Tablet,
    Desktop,
}

impl FormFactor {
    pub fn from_logical_width(w: f32) -> Self {
        match w {
            w if w < 700.0 => Self::Phone,
            w if w < 1100.0 => Self::Tablet,
            _ => Self::Desktop,
        }
    }

    pub fn from_logical_size(w: f32, h: f32) -> Self {
        Self::from_logical_width(w.min(h))
    }

    pub fn metrics(self) -> Metrics {
        match self {
            Self::Phone => Metrics {
                btn_h: 56.0,
                btn_w: 120.0,
                gutter: 12.0,
                title: 22.0,
                body: 15.0,
                icon: 28.0,
                touch: 48.0,
            },
            Self::Tablet => Metrics {
                btn_h: 64.0,
                btn_w: 160.0,
                gutter: 16.0,
                title: 26.0,
                body: 16.0,
                icon: 32.0,
                touch: 48.0,
            },
            Self::Desktop => Metrics {
                btn_h: 56.0,
                btn_w: 180.0,
                gutter: 20.0,
                title: 28.0,
                body: 16.0,
                icon: 36.0,
                touch: 32.0,
            },
        }
    }

    pub fn is_phone(self) -> bool {
        matches!(self, Self::Phone)
    }
}

pub struct Metrics {
    pub btn_h: f32,
    pub btn_w: f32,
    pub gutter: f32,
    pub title: f32,
    pub body: f32,
    pub icon: f32,
    pub touch: f32,
}

pub const PANEL: Color = Color::new_u8(249, 251, 253, 0.98);
pub const SURFACE: Color = Color::new_u8(255, 255, 255, 1.0);
pub const SURFACE_ELEVATED: Color = Color::new_u8(248, 250, 252, 1.0);
pub const SURFACE_HOVER: Color = Color::new_u8(243, 247, 251, 1.0);
pub const SURFACE_PRESSED: Color = Color::new_u8(236, 241, 247, 1.0);

pub const DIVIDER: Color = Color::new_u8(221, 228, 236, 1.0);

pub const TEXT: Color = Color::new_u8(15, 23, 42, 1.0);
pub const TEXT_MUTED: Color = Color::new_u8(100, 116, 139, 1.0);

pub const PRIMARY: Color = Color::new_u8(24, 64, 135, 1.0);
pub const PRIMARY_HOVER: Color = Color::new_u8(20, 54, 118, 1.0);
pub const PRIMARY_PRESSED: Color = Color::new_u8(15, 44, 96, 1.0);
pub const PRIMARY_SOFT: Color = Color::new_u8(229, 238, 251, 1.0);

pub const DANGER: Color = Color::new_u8(162, 60, 76, 1.0);
pub const DANGER_SOFT: Color = Color::new_u8(250, 234, 238, 1.0);
