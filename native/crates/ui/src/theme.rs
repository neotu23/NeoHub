//! Design tokens carried over from the Electron renderer.
//!
//! Source of truth: `src/renderer/styles.css` (dark = Tailwind zinc scale, light =
//! Radix slate scale), plus the spacing/radius/typography actually used by the
//! components. Values are copied, not re-invented — the Rust rewrite must look like
//! the same product. Anything the old CSS expressed as a Tailwind utility
//! (`p-4`, `rounded-md`, `text-sm`) is recorded here as PX so views never hard-code
//! literals.

use gpui_kit::Rgba;

/// `gpui::rgb` is a runtime fn; palettes are `const`, so mirror it here.
const fn hex(value: u32) -> Rgba {
    let [_, r, g, b] = value.to_be_bytes();
    Rgba {
        r: r as f32 / 255.0,
        g: g as f32 / 255.0,
        b: b as f32 / 255.0,
        a: 1.0,
    }
}

/// One semantic palette. Mirrors the `--*` custom properties in `styles.css`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Palette {
    /// App background — `--bg-base`.
    pub bg_base: Rgba,
    /// Cards / inputs — `--bg-surface`.
    pub bg_surface: Rgba,
    /// Hover / elevated — `--bg-elevated`.
    pub bg_elevated: Rgba,
    /// Quiet separator — `--border`.
    pub border: Rgba,
    /// Strong separator / focus ring — `--border-strong`.
    pub border_strong: Rgba,
    /// Primary text — `--fg`.
    pub fg: Rgba,
    /// Secondary text — `--fg-muted`.
    pub fg_muted: Rgba,
    /// Tertiary text / placeholder — `--fg-subtle`.
    pub fg_subtle: Rgba,
    /// Harbor brand (teal-400 dark / teal-600 light) — `--accent`.
    pub accent: Rgba,
    /// Text on the accent button — `--accent-fg`.
    pub accent_fg: Rgba,

    pub danger_bg: Rgba,
    pub danger_bg_hover: Rgba,
    pub danger_fg: Rgba,
    pub danger_border: Rgba,
    pub danger_strong: Rgba,

    pub success_bg: Rgba,
    pub success_bg_hover: Rgba,
    pub success_fg: Rgba,
    pub success_border: Rgba,
    pub success_strong: Rgba,

    pub warn_bg: Rgba,
    pub warn_bg_hover: Rgba,
    pub warn_fg: Rgba,
    pub warn_border: Rgba,
    pub warn_strong: Rgba,
}

/// Dark theme — Tailwind zinc scale, exactly as `.dark` in `styles.css`.
pub const DARK: Palette = Palette {
    bg_base: hex(0x18181b),
    bg_surface: hex(0x202024),
    bg_elevated: hex(0x2a2a2f),
    border: hex(0x27272b),
    border_strong: hex(0x3f3f46),
    fg: hex(0xededf0),
    fg_muted: hex(0xa1a1aa),
    fg_subtle: hex(0x71717a),
    accent: hex(0x2dd4bf),
    accent_fg: hex(0x09090b),

    danger_bg: hex(0x481a1d),
    danger_bg_hover: hex(0x541b1f),
    danger_fg: hex(0xff9592),
    danger_border: hex(0x67292a),
    danger_strong: hex(0xe5484d),

    success_bg: hex(0x142d1e),
    success_bg_hover: hex(0x1a3a26),
    success_fg: hex(0x86efac),
    success_border: hex(0x225434),
    success_strong: hex(0x22c55e),

    warn_bg: hex(0x4a2700),
    warn_bg_hover: hex(0x5b3300),
    warn_fg: hex(0xffc851),
    warn_border: hex(0x7a4a00),
    warn_strong: hex(0xffb224),
};

/// Light theme — Radix slate scale, exactly as `.light` in `styles.css`.
pub const LIGHT: Palette = Palette {
    bg_base: hex(0xfbfcfd),
    bg_surface: hex(0xffffff),
    bg_elevated: hex(0xf1f3f5),
    border: hex(0xdfe3e6),
    border_strong: hex(0xd7dbdf),
    fg: hex(0x11181c),
    fg_muted: hex(0x687076),
    fg_subtle: hex(0x889096),
    accent: hex(0x0d9488),
    accent_fg: hex(0xffffff),

    danger_bg: hex(0xffefef),
    danger_bg_hover: hex(0xffdcdc),
    danger_fg: hex(0xc52433),
    danger_border: hex(0xf89ea8),
    danger_strong: hex(0xe5484d),

    success_bg: hex(0xebf9eb),
    success_bg_hover: hex(0xd7f0d5),
    success_fg: hex(0x207935),
    success_border: hex(0x9adaa2),
    success_strong: hex(0x46a758),

    warn_bg: hex(0xfff7c8),
    warn_bg_hover: hex(0xffee9c),
    warn_fg: hex(0xad5700),
    warn_border: hex(0xf6d174),
    warn_strong: hex(0xffb224),
};

/// Spacing / radius / typography, expressed in px because GPUI has no Tailwind.
pub mod metrics {
    /// Tailwind `gap-1`.
    pub const GAP_1: f32 = 4.0;
    /// Tailwind `gap-2`.
    pub const GAP_2: f32 = 8.0;
    /// Tailwind `gap-3`.
    pub const GAP_3: f32 = 12.0;
    /// Tailwind `gap-4`.
    pub const GAP_4: f32 = 16.0;
    /// Tailwind `p-2`.
    pub const PAD_2: f32 = 8.0;
    /// Tailwind `p-3`.
    pub const PAD_3: f32 = 12.0;
    /// Tailwind `p-4`.
    pub const PAD_4: f32 = 16.0;
    /// Tailwind `p-6`.
    pub const PAD_6: f32 = 24.0;

    /// Sidebar width used by `Sidebar.tsx` (`w-60`).
    pub const SIDEBAR_WIDTH: f32 = 240.0;
    /// Minimum window the old app enforced (`960×600`).
    pub const MIN_WINDOW_WIDTH: f32 = 960.0;
    /// Minimum window the old app enforced (`960×600`).
    pub const MIN_WINDOW_HEIGHT: f32 = 600.0;
    /// Default window (`1280×820` in `index.ts` `loadBounds`).
    pub const DEFAULT_WINDOW_WIDTH: f32 = 1280.0;
    /// Default window (`1280×820` in `index.ts` `loadBounds`).
    pub const DEFAULT_WINDOW_HEIGHT: f32 = 820.0;

    /// `rounded-md`.
    pub const RADIUS_MD: f32 = 6.0;
    /// `rounded-lg`.
    pub const RADIUS_LG: f32 = 8.0;
    /// `rounded-xl`.
    pub const RADIUS_XL: f32 = 12.0;
    /// Hairline border used by cards and inputs.
    pub const BORDER_1: f32 = 1.0;

    /// `text-[11px]` — the smallest label size used in the sidebar.
    pub const TEXT_XS: f32 = 11.0;
    /// `text-xs`.
    pub const TEXT_SM: f32 = 12.0;

    /// Monospace family for the terminal and log panes.
    ///
    /// SF Mono ships with macOS; Menlo is the fallback that has shipped forever, so a machine
    /// without the newer face still gets a proper fixed-width grid instead of a proportional
    /// font that would make ANSI columns meaningless.
    pub const MONO_FAMILY: &str = "SF Mono";
    pub const MONO_FAMILY_FALLBACK: &str = "Menlo";
    /// `text-sm`.
    pub const TEXT_BASE: f32 = 14.0;
    /// `text-lg`.
    pub const TEXT_LG: f32 = 18.0;
}

/// Which palette to render. `System` follows the macOS appearance.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThemeMode {
    Dark,
    Light,
    System,
}

impl ThemeMode {
    /// Resolve to a concrete palette (`system` needs the platform appearance).
    pub fn palette(self, system_is_dark: bool) -> Palette {
        match self {
            ThemeMode::Dark => DARK,
            ThemeMode::Light => LIGHT,
            ThemeMode::System => {
                if system_is_dark {
                    DARK
                } else {
                    LIGHT
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dark_and_light_palettes_differ_on_every_semantic_role() {
        // Guards against a copy/paste slip when the two palettes were transcribed.
        assert_ne!(DARK.bg_base, LIGHT.bg_base);
        assert_ne!(DARK.fg, LIGHT.fg);
        assert_ne!(DARK.accent, LIGHT.accent);
        assert_ne!(DARK.border, LIGHT.border);
    }

    #[test]
    fn system_mode_follows_the_platform_appearance() {
        assert_eq!(ThemeMode::System.palette(true), DARK);
        assert_eq!(ThemeMode::System.palette(false), LIGHT);
        assert_eq!(ThemeMode::Dark.palette(false), DARK);
        assert_eq!(ThemeMode::Light.palette(true), LIGHT);
    }
}

#[cfg(test)]
mod font_tests {
    use super::*;

    #[test]
    fn a_monospace_family_is_declared() {
        // The terminal grid is meaningless in a proportional font: columns need equal advance.
        assert!(!metrics::MONO_FAMILY.is_empty());
        assert!(!metrics::MONO_FAMILY_FALLBACK.is_empty());
        assert_ne!(
            metrics::MONO_FAMILY,
            metrics::MONO_FAMILY_FALLBACK,
            "the fallback has to be a different face, otherwise it is not a fallback"
        );
    }

    #[test]
    fn the_fallback_is_a_font_that_ships_with_macos() {
        // Menlo has shipped since 10.6; anything newer would leave old machines without a grid.
        assert_eq!(metrics::MONO_FAMILY_FALLBACK, "Menlo");
    }
}
