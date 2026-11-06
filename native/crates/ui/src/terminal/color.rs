//! ANSI colour → RGBA, using the same palette the old terminal used for the classic 16.

use gpui_kit::Rgba;
use neohub_core::logs::AnsiColor;

/// Classic 16-colour palette (xterm defaults, tuned slightly for dark backgrounds).
pub const ANSI_16: [(u8, u8, u8); 16] = [
    (0x1a, 0x1b, 0x26),
    (0xf7, 0x76, 0x8e),
    (0x9e, 0xce, 0x6a),
    (0xe0, 0xaf, 0x68),
    (0x7a, 0xa2, 0xf7),
    (0xbb, 0x9a, 0xf7),
    (0x7d, 0xcf, 0xff),
    (0xc0, 0xca, 0xf5),
    (0x41, 0x48, 0x68),
    (0xff, 0x9e, 0x64),
    (0xa6, 0xe3, 0xa1),
    (0xff, 0xe6, 0xa6),
    (0x9a, 0xb8, 0xff),
    (0xd0, 0xb0, 0xff),
    (0xa4, 0xe6, 0xff),
    (0xff, 0xff, 0xff),
];

/// Resolve one colour, falling back to `default` for `AnsiColor::Default`.
pub fn ansi_to_rgba(color: AnsiColor, default: Rgba) -> Rgba {
    match color {
        AnsiColor::Default => default,
        AnsiColor::Indexed(index) => {
            let (r, g, b) = indexed_rgb(index);
            rgba8(r, g, b)
        }
        AnsiColor::Rgb(r, g, b) => rgba8(r, g, b),
    }
}

/// 256-colour palette lookup: 16 classic, 216 cube, 24 greys.
pub fn indexed_rgb(index: u8) -> (u8, u8, u8) {
    match index {
        0..=15 => ANSI_16[index as usize],
        16..=231 => {
            let value = index - 16;
            let r = value / 36;
            let g = (value % 36) / 6;
            let b = value % 6;
            let level = |component: u8| {
                if component == 0 {
                    0
                } else {
                    55 + component * 40
                }
            };
            (level(r), level(g), level(b))
        }
        _ => {
            let level = 8 + (index - 232) * 10;
            (level, level, level)
        }
    }
}

fn rgba8(r: u8, g: u8, b: u8) -> Rgba {
    Rgba {
        r: r as f32 / 255.0,
        g: g as f32 / 255.0,
        b: b as f32 / 255.0,
        a: 1.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::rgb;

    #[test]
    fn default_colour_falls_back_to_the_theme_foreground() {
        let default = rgb(0xededf0);
        assert_eq!(ansi_to_rgba(AnsiColor::Default, default), default);
    }

    #[test]
    fn the_classic_sixteen_map_to_their_palette_entries() {
        assert_eq!(indexed_rgb(1), ANSI_16[1], "red");
        assert_eq!(indexed_rgb(2), ANSI_16[2], "green");
        assert_eq!(indexed_rgb(9), ANSI_16[9], "bright red");
    }

    #[test]
    fn the_colour_cube_and_greyscale_are_computed() {
        assert_eq!(indexed_rgb(16), (0, 0, 0), "cube origin");
        assert_eq!(indexed_rgb(231), (255, 255, 255), "cube corner");
        assert_eq!(indexed_rgb(232), (8, 8, 8), "first grey");
        assert_eq!(indexed_rgb(255), (238, 238, 238), "last grey");
    }

    #[test]
    fn truecolor_passes_through() {
        let colour = ansi_to_rgba(AnsiColor::Rgb(1, 2, 3), rgb(0xffffff));
        assert!((colour.r - 1.0 / 255.0).abs() < 1e-6);
        assert!((colour.b - 3.0 / 255.0).abs() < 1e-6);
        assert_eq!(colour.a, 1.0);
    }
}
