//! The one palette of the project. The web page's CSS tokens are generated from here
//! ([`css_tokens`]), so the chart pixels and the HTML around them cannot drift apart.

/// An sRGB colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    /// "#rrggbb".
    pub fn hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.0, self.1, self.2)
    }
}

/// Colours of one theme.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Theme {
    pub dark: bool,
    /// Page background.
    pub bg: Rgb,
    /// Chart and card background.
    pub surface: Rgb,
    /// Primary text and ensemble lines.
    pub ink: Rgb,
    /// Secondary text, thresholds, axis labels.
    pub muted: Rgb,
    /// Grid lines and borders.
    pub rule: Rgb,
    /// Buttons and focus.
    pub accent: Rgb,
    /// The three highlighted players (categorical slots 1-3, validated for colour-vision deficiency).
    pub players: [Rgb; 3],
    /// Density ramp: one player (lo) to the fullest cell (hi).
    pub dens_lo: Rgb,
    pub dens_hi: Rgb,
}

/// Light theme.
pub const LIGHT: Theme = Theme {
    dark: false,
    bg: Rgb(0xfb, 0xfb, 0xfa),
    surface: Rgb(0xff, 0xff, 0xff),
    ink: Rgb(0x16, 0x18, 0x1d),
    muted: Rgb(0x5b, 0x5f, 0x68),
    rule: Rgb(0xe3, 0xe4, 0xe6),
    accent: Rgb(0x2a, 0x78, 0xd6),
    players: [Rgb(0x2a, 0x78, 0xd6), Rgb(0xeb, 0x68, 0x34), Rgb(0x1b, 0xaf, 0x7a)],
    dens_lo: Rgb(0xd0, 0xce, 0xc8),
    dens_hi: Rgb(0x33, 0x32, 0x2f),
};

/// Dark theme.
pub const DARK: Theme = Theme {
    dark: true,
    bg: Rgb(0x13, 0x14, 0x17),
    surface: Rgb(0x1a, 0x1b, 0x1f),
    ink: Rgb(0xec, 0xee, 0xf2),
    muted: Rgb(0xa3, 0xa7, 0xb0),
    rule: Rgb(0x2c, 0x2e, 0x33),
    accent: Rgb(0x39, 0x87, 0xe5),
    players: [Rgb(0x39, 0x87, 0xe5), Rgb(0xd9, 0x59, 0x26), Rgb(0x19, 0x9e, 0x70)],
    dens_lo: Rgb(0x3e, 0x3f, 0x45),
    dens_hi: Rgb(0xe0, 0xdf, 0xda),
};

// ---------- OKLab, for a perceptually even density ramp ----------

fn to_linear(c: u8) -> f64 {
    let c = f64::from(c) / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn to_srgb(c: f64) -> u8 {
    let c = c.clamp(0.0, 1.0);
    let v = if c <= 0.003_130_8 { 12.92 * c } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 };
    (v * 255.0).round() as u8
}

fn oklab(c: Rgb) -> [f64; 3] {
    let (r, g, b) = (to_linear(c.0), to_linear(c.1), to_linear(c.2));
    let l = (0.412_221_470_8 * r + 0.536_332_536_3 * g + 0.051_445_992_9 * b).cbrt();
    let m = (0.211_903_498_2 * r + 0.680_699_545_1 * g + 0.107_396_956_6 * b).cbrt();
    let s = (0.088_302_461_9 * r + 0.281_718_837_6 * g + 0.629_978_700_5 * b).cbrt();
    [
        0.210_454_255_3 * l + 0.793_617_785 * m - 0.004_072_046_8 * s,
        1.977_998_495_1 * l - 2.428_592_205 * m + 0.450_593_709_9 * s,
        0.025_904_037_1 * l + 0.782_771_766_2 * m - 0.808_675_766 * s,
    ]
}

fn from_oklab(lab: [f64; 3]) -> Rgb {
    let l = (lab[0] + 0.396_337_777_4 * lab[1] + 0.215_803_757_3 * lab[2]).powi(3);
    let m = (lab[0] - 0.105_561_345_8 * lab[1] - 0.063_854_172_8 * lab[2]).powi(3);
    let s = (lab[0] - 0.089_484_177_5 * lab[1] - 1.291_485_548 * lab[2]).powi(3);
    Rgb(
        to_srgb(4.076_741_662_1 * l - 3.307_711_591_3 * m + 0.230_969_929_2 * s),
        to_srgb(-1.268_438_004_6 * l + 2.609_757_401_1 * m - 0.341_319_396_5 * s),
        to_srgb(-0.004_196_086_3 * l - 0.703_418_614_7 * m + 1.707_614_701 * s),
    )
}

impl Theme {
    /// Density colour at position `s` in [0, 1] of the ramp (OKLab interpolation).
    pub fn ramp(&self, s: f64) -> Rgb {
        let (a, b) = (oklab(self.dens_lo), oklab(self.dens_hi));
        let s = s.clamp(0.0, 1.0);
        from_oklab([a[0] + (b[0] - a[0]) * s, a[1] + (b[1] - a[1]) * s, a[2] + (b[2] - a[2]) * s])
    }
}

/// CSS custom properties for both themes, in the shape the page expects:
/// light on `:root`, dark under `prefers-color-scheme` and under `[data-theme="dark"]`.
pub fn css_tokens() -> String {
    fn block(t: &Theme) -> String {
        let mut s = format!(
            "--bg: {}; --surface: {}; --ink: {}; --muted: {}; --rule: {}; --accent: {}; ",
            t.bg.hex(),
            t.surface.hex(),
            t.ink.hex(),
            t.muted.hex(),
            t.rule.hex(),
            t.accent.hex()
        );
        for (i, c) in t.players.iter().enumerate() {
            s.push_str(&format!("--p{}: {}; ", i + 1, c.hex()));
        }
        let stops: Vec<String> = (0..=8).map(|i| t.ramp(f64::from(i) / 8.0).hex()).collect();
        s.push_str(&format!("--ramp: linear-gradient(90deg, {});", stops.join(", ")));
        s
    }
    format!(
        ":root {{ {} }}\n@media (prefers-color-scheme: dark) {{ :root:not([data-theme=\"light\"]) {{ {} color-scheme: dark; }} }}\n:root[data-theme=\"dark\"] {{ {} color-scheme: dark; }}\n",
        block(&LIGHT),
        block(&DARK),
        block(&DARK)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn luminance(c: Rgb) -> f64 {
        0.2126 * to_linear(c.0) + 0.7152 * to_linear(c.1) + 0.0722 * to_linear(c.2)
    }

    #[test]
    fn ramp_ends_are_the_theme_colours_and_the_ramp_is_monotone() {
        for t in [LIGHT, DARK] {
            assert_eq!(t.ramp(0.0), t.dens_lo);
            assert_eq!(t.ramp(1.0), t.dens_hi);
            let lum: Vec<f64> = (0..=100).map(|i| luminance(t.ramp(f64::from(i) / 100.0))).collect();
            let rising = lum.windows(2).all(|w| w[1] >= w[0]);
            let falling = lum.windows(2).all(|w| w[1] <= w[0]);
            assert!(if t.dark { rising } else { falling }, "more players must always be further from the surface");
        }
    }

    #[test]
    fn one_player_is_visible_against_the_surface() {
        for t in [LIGHT, DARK] {
            let (a, b) = (luminance(t.dens_lo), luminance(t.surface));
            let contrast = (a.max(b) + 0.05) / (a.min(b) + 0.05);
            // One player (an outlier) must be clearly visible.
            assert!(contrast >= 1.5, "lowest density step too close to the surface: {contrast:.2}");
        }
    }
}
