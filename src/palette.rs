//! Colour palette presets, including one generated in OKLCH (the polar form of Björn
//! Ottosson's OKLab), a perceptually uniform colour space. Both OKLCH rows use the same 12
//! evenly spaced hues. The dark row keeps one fixed lightness and a low chroma, so it is
//! muted and evenly bright. The light row is for fun, vivid colours: each hue moves most of
//! the way to its own most vivid point, so yellow ends up lighter and blue darker.

use eframe::egui::Color32;

pub struct Preset {
    pub name: &'static str,
    /// Two rows, top row first.
    pub colors: Vec<Color32>,
}

/// Swatches per row, matching the classic palette.
#[cfg(test)]
pub const ROW: usize = 14;

/// The classic 28-colour palette from XP-era Paint.
const CLASSIC: [[u8; 3]; 28] = [
    [0, 0, 0], [128, 128, 128], [128, 0, 0], [128, 128, 0], [0, 128, 0], [0, 128, 128], [0, 0, 128],
    [128, 0, 128], [128, 128, 64], [0, 64, 64], [0, 128, 255], [0, 64, 128], [64, 0, 255], [128, 64, 0],
    [255, 255, 255], [192, 192, 192], [255, 0, 0], [255, 255, 0], [0, 255, 0], [0, 255, 255], [0, 0, 255],
    [255, 0, 255], [255, 255, 128], [0, 255, 128], [128, 255, 255], [128, 128, 255], [255, 0, 128], [255, 128, 64],
];

/// OKLCH dark row: (lightness, target chroma).
pub const DARK_ROW: (f32, f32) = (0.50, 0.10);
/// OKLCH light row: start lightness, and how far (0..1) each hue moves toward its own
/// most vivid lightness. 1.0 would be pure neon primaries, close to the Classic palette.
pub const LIGHT_BASE: f32 = 0.74;
pub const LIGHT_VIVID: f32 = 0.75;
/// OKLCH gray lightness for each row.
pub const DARK_GRAY: f32 = 0.45;
pub const LIGHT_GRAY: f32 = 0.80;
/// First hue in degrees; 12 hues follow, 30° apart. ~29° is OKLCH's pure red.
pub const FIRST_HUE: f32 = 29.0;
pub const HUES: usize = 12;

pub fn presets() -> Vec<Preset> {
    vec![
        Preset { name: "Classic", colors: CLASSIC.iter().map(|c| Color32::from_rgb(c[0], c[1], c[2])).collect() },
        Preset { name: "OKLCH", colors: oklch_palette() },
    ]
}

fn oklch_palette() -> Vec<Color32> {
    let hue = |i: usize| FIRST_HUE + i as f32 * 360.0 / HUES as f32;
    let mut top = vec![Color32::BLACK, oklch_clipped(DARK_GRAY, 0.0, 0.0)];
    top.extend((0..HUES).map(|i| oklch_clipped(DARK_ROW.0, DARK_ROW.1, hue(i))));
    let mut bottom = vec![Color32::WHITE, oklch_clipped(LIGHT_GRAY, 0.0, 0.0)];
    bottom.extend((0..HUES).map(|i| vivid(hue(i))));
    top.extend(bottom);
    top
}

// ---- OKLab conversions (https://bottosson.github.io/posts/oklab/) ---------------------

/// OKLab -> linear sRGB.
fn oklab_to_linear(l: f32, a: f32, b: f32) -> [f32; 3] {
    let l_ = l + 0.396_337_78 * a + 0.215_803_76 * b;
    let m_ = l - 0.105_561_346 * a - 0.063_854_17 * b;
    let s_ = l - 0.089_484_18 * a - 1.291_485_5 * b;
    let (l3, m3, s3) = (l_ * l_ * l_, m_ * m_ * m_, s_ * s_ * s_);
    [
        4.076_741_7 * l3 - 3.307_711_6 * m3 + 0.230_969_94 * s3,
        -1.268_438 * l3 + 2.609_757_4 * m3 - 0.341_319_38 * s3,
        -0.004_196_086_3 * l3 - 0.703_418_6 * m3 + 1.707_614_7 * s3,
    ]
}

/// Linear sRGB -> OKLab.
#[cfg(test)]
pub fn linear_to_oklab(c: [f32; 3]) -> [f32; 3] {
    let l = 0.412_221_46 * c[0] + 0.536_332_55 * c[1] + 0.051_445_995 * c[2];
    let m = 0.211_903_5 * c[0] + 0.680_699_5 * c[1] + 0.107_396_96 * c[2];
    let s = 0.088_302_46 * c[0] + 0.281_718_85 * c[1] + 0.629_978_7 * c[2];
    let (l_, m_, s_) = (l.cbrt(), m.cbrt(), s.cbrt());
    [
        0.210_454_26 * l_ + 0.793_617_8 * m_ - 0.004_072_047 * s_,
        1.977_998_5 * l_ - 2.428_592_2 * m_ + 0.450_593_7 * s_,
        0.025_904_037 * l_ + 0.782_771_77 * m_ - 0.808_675_77 * s_,
    ]
}

fn encode(x: f32) -> u8 {
    let v = if x <= 0.003_130_8 { 12.92 * x } else { 1.055 * x.powf(1.0 / 2.4) - 0.055 };
    (v.clamp(0.0, 1.0) * 255.0).round() as u8
}

#[cfg(test)]
pub fn decode(v: u8) -> f32 {
    let x = v as f32 / 255.0;
    if x <= 0.040_45 {
        x / 12.92
    } else {
        ((x + 0.055) / 1.055).powf(2.4)
    }
}

/// OKLCH colour if it fits in sRGB.
fn oklch(l: f32, c: f32, h_deg: f32) -> Option<Color32> {
    let h = h_deg.to_radians();
    let rgb = oklab_to_linear(l, c * h.cos(), c * h.sin());
    let eps = 1e-4;
    rgb.iter()
        .all(|v| (-eps..=1.0 + eps).contains(v))
        .then(|| Color32::from_rgb(encode(rgb[0]), encode(rgb[1]), encode(rgb[2])))
}

/// Light-row colour for a hue: lightness moved toward the hue's most vivid point, then the
/// most chroma sRGB can show there.
fn vivid(h: f32) -> Color32 {
    let l = LIGHT_BASE + (cusp_lightness(h) - LIGHT_BASE) * LIGHT_VIVID;
    oklch_clipped(l, max_chroma(l, h), h)
}

/// Largest chroma at this lightness and hue that sRGB can display.
fn max_chroma(l: f32, h: f32) -> f32 {
    let (mut lo, mut hi) = (0.0f32, 0.4f32);
    for _ in 0..24 {
        let mid = (lo + hi) / 2.0;
        if oklch(l, mid, h).is_some() {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    lo
}

/// Lightness at which this hue reaches its highest displayable chroma.
fn cusp_lightness(h: f32) -> f32 {
    let (mut best_l, mut best_c) = (0.5, 0.0);
    let mut l = 0.30;
    while l < 0.99 {
        let c = max_chroma(l, h);
        if c > best_c {
            (best_l, best_c) = (l, c);
        }
        l += 0.005;
    }
    best_l
}

/// OKLCH colour, lowering only the chroma (never hue or lightness) until it fits in sRGB.
pub fn oklch_clipped(l: f32, c: f32, h_deg: f32) -> Color32 {
    if let Some(col) = oklch(l, c, h_deg) {
        return col;
    }
    let (mut lo, mut hi) = (0.0, c);
    for _ in 0..24 {
        let mid = (lo + hi) / 2.0;
        if oklch(l, mid, h_deg).is_some() {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    oklch(l, lo, h_deg).unwrap_or(Color32::BLACK)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lab(c: Color32) -> [f32; 3] {
        linear_to_oklab([decode(c.r()), decode(c.g()), decode(c.b())])
    }

    #[test]
    fn conversion_matches_reference_values() {
        // sRGB red is OKLab (0.62796, 0.22486, 0.12585) in Ottosson's reference.
        let r = lab(Color32::from_rgb(255, 0, 0));
        assert!((r[0] - 0.62796).abs() < 1e-3 && (r[1] - 0.22486).abs() < 1e-3 && (r[2] - 0.12585).abs() < 1e-3);
        assert!((lab(Color32::WHITE)[0] - 1.0).abs() < 1e-3);
        // Round trip.
        let c = oklch_clipped(0.7, 0.1, 140.0);
        let l = lab(c);
        assert!((l[0] - 0.7).abs() < 0.01);
    }

    #[test]
    fn oklch_palette_rows() {
        let p = presets().into_iter().find(|p| p.name == "OKLCH").unwrap();
        assert_eq!(p.colors.len(), 2 * ROW);
        assert_eq!(p.colors[0], Color32::BLACK);
        assert_eq!(p.colors[ROW], Color32::WHITE);
        let dark = &p.colors[2..ROW];
        let light = &p.colors[ROW + 2..];
        let chroma = |c: Color32| {
            let l = lab(c);
            (l[1] * l[1] + l[2] * l[2]).sqrt()
        };

        // Dark row: muted and evenly bright (one OKLab lightness, within rounding).
        for &c in dark {
            assert!((lab(c)[0] - DARK_ROW.0).abs() < 0.01, "dark {c:?} has L {}", lab(c)[0]);
        }
        // Light row: every colour is (nearly) as saturated as the screen allows at its
        // lightness, i.e. vivid, while staying light enough to read as a light row.
        for (i, &c) in light.iter().enumerate() {
            let [l, ..] = lab(c);
            let hue = FIRST_HUE + i as f32 * 30.0;
            // Vivid blue is naturally dark (its most vivid point is near L 0.46), hence 0.50.
            assert!((0.50..0.97).contains(&l), "light {i} has L {l}");
            assert!(chroma(c) > max_chroma(l, hue) - 0.02, "light {i} is not vivid ({} of {})", chroma(c), max_chroma(l, hue));
        }
        // Both rows: all swatches are clearly different (OKLab distance).
        for row in [dark, light] {
            for i in 0..row.len() {
                for j in i + 1..row.len() {
                    let (a, b) = (lab(row[i]), lab(row[j]));
                    let d = ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt();
                    assert!(d > 0.02, "swatches {i} and {j} look alike (dE {d})");
                }
            }
        }
        // The light row is far more saturated than the muted dark row.
        let avg = |s: &[Color32]| s.iter().map(|&c| chroma(c)).sum::<f32>() / s.len() as f32;
        assert!(avg(light) > 2.0 * avg(dark), "light {} vs dark {}", avg(light), avg(dark));
    }

    #[test]
    fn classic_is_unchanged() {
        let p = &presets()[0];
        assert_eq!(p.colors.len(), 28);
        assert_eq!(p.colors[16], Color32::RED);
    }
}
