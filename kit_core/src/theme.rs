use core::time::Duration;
use peniko::Color;

fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::from_rgba8(r, g, b, 255)
}
fn rgba(r: u8, g: u8, b: u8, a: u8) -> Color {
    Color::from_rgba8(r, g, b, a)
}

/// Player colour-vision setting; selects the friend/foe pair so team identity never relies on red/green alone.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum ColorVision {
    Normal,
    Protanopia,
    Deuteranopia,
    Tritanopia,
}

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum MotionPref {
    Full,
    Reduced,
}

#[derive(Copy, Clone, PartialEq, Debug)]
pub struct Palette {
    pub surface: Color,
    pub surface_raised: Color,
    pub surface_inset: Color,
    pub text: Color,
    pub text_muted: Color,
    pub text_disabled: Color,
    pub accent: Color,
    pub danger: Color,
    pub warning: Color,
    pub success: Color,
    pub friend: Color,
    pub foe: Color,
    pub hover_overlay: Color,
    pub pressed_overlay: Color,
    pub focus_ring: Color,
    pub field_fill: Color,
    pub field_border: Color,
    /// Behind selected text.
    pub selection: Color,
    pub scrim: Color,
}

#[derive(Copy, Clone, PartialEq, Debug)]
pub struct Spacing {
    pub xs: f64,
    pub sm: f64,
    pub md: f64,
    pub lg: f64,
    pub xl: f64,
}

#[derive(Copy, Clone, PartialEq, Debug)]
pub struct Radii {
    pub sm: f64,
    pub md: f64,
    pub lg: f64,
    /// Corner radius used for hover/press/focus overlays on interactive nodes.
    pub control: f64,
}

#[derive(Copy, Clone, PartialEq, Debug)]
pub struct TypeScale {
    pub caption: f64,
    pub body: f64,
    pub label: f64,
    pub title: f64,
}

/// Button size roles: a control picks a role, never pixels, so screens do not drift apart. Width and height in logical px.
#[derive(Copy, Clone, PartialEq, Debug)]
pub struct ControlSizes {
    /// A corner chip on a tile (drop cargo).
    pub mini: (f64, f64),
    /// Toolbar, status and talent tiles; the height of every text-bearing button.
    pub compact: (f64, f64),
    /// The standard tile: plan steps, inventory, shop cards, weapon pickers.
    pub tile: (f64, f64),
    /// The one action a screen is about: ready to sail, launch.
    pub hero: (f64, f64),
}

/// Motion durations. Under `MotionPref::Reduced` every accessor returns zero, so animations snap.
#[derive(Copy, Clone, PartialEq, Debug)]
pub struct Motion {
    pub pref: MotionPref,
    fast: Duration,
    normal: Duration,
    slow: Duration,
}

impl Motion {
    fn scaled(&self, d: Duration) -> Duration {
        match self.pref {
            MotionPref::Full => d,
            MotionPref::Reduced => Duration::ZERO,
        }
    }
    pub fn fast(&self) -> Duration {
        self.scaled(self.fast)
    }
    pub fn normal(&self) -> Duration {
        self.scaled(self.normal)
    }
    pub fn slow(&self) -> Duration {
        self.scaled(self.slow)
    }
}

#[derive(Clone, PartialEq, Debug)]
pub struct Theme {
    pub vision: ColorVision,
    pub colors: Palette,
    pub spacing: Spacing,
    pub radii: Radii,
    pub type_scale: TypeScale,
    pub controls: ControlSizes,
    pub motion: Motion,
}

impl Theme {
    pub fn new(vision: ColorVision, motion: MotionPref) -> Self {
        // Okabe-Ito based pairs; each pair separates on the axis that vision type can still see.
        let (friend, foe) = match vision {
            ColorVision::Normal => (rgb(70, 170, 100), rgb(225, 70, 70)),
            ColorVision::Protanopia | ColorVision::Deuteranopia => {
                (rgb(0, 114, 178), rgb(230, 159, 0))
            }
            ColorVision::Tritanopia => (rgb(0, 158, 115), rgb(213, 94, 0)),
        };
        Self {
            vision,
            colors: Palette {
                surface: rgb(15, 38, 56),
                surface_raised: rgb(29, 63, 88),
                surface_inset: rgb(7, 20, 32),
                text: rgb(232, 238, 242),
                text_muted: rgb(150, 168, 180),
                text_disabled: rgb(91, 105, 115),
                accent: rgb(111, 91, 40),
                danger: rgb(200, 60, 60),
                warning: rgb(220, 170, 50),
                success: rgb(80, 180, 110),
                friend,
                foe,
                hover_overlay: rgba(255, 255, 255, 28),
                pressed_overlay: rgba(0, 0, 0, 60),
                focus_ring: rgb(255, 214, 102),
                field_fill: rgb(7, 20, 32),
                field_border: rgb(60, 90, 112),
                selection: rgba(70, 130, 220, 120),
                scrim: rgba(0, 0, 0, 140),
            },
            spacing: Spacing {
                xs: 2.0,
                sm: 4.0,
                md: 8.0,
                lg: 16.0,
                xl: 24.0,
            },
            radii: Radii {
                sm: 3.0,
                md: 6.0,
                lg: 12.0,
                control: 6.0,
            },
            type_scale: TypeScale {
                caption: 11.0,
                body: 14.0,
                label: 13.0,
                title: 18.0,
            },
            controls: ControlSizes {
                mini: (20.0, 20.0),
                compact: (44.0, 48.0),
                tile: (56.0, 60.0),
                hero: (68.0, 64.0),
            },
            motion: Motion {
                pref: motion,
                fast: Duration::from_millis(120),
                normal: Duration::from_millis(220),
                slow: Duration::from_millis(400),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reduced_motion_zeroes_every_duration_and_vision_changes_team_pair() {
        let t = Theme::new(ColorVision::Deuteranopia, MotionPref::Reduced);
        assert_eq!(t.motion.slow(), Duration::ZERO);
        assert_eq!(t.motion.fast(), Duration::ZERO);
        let normal = Theme::new(ColorVision::Normal, MotionPref::Full);
        assert_ne!(normal.colors.friend, t.colors.friend);
        assert!(normal.motion.fast() > Duration::ZERO);
    }
}
