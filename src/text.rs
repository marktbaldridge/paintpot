//! Text: system fonts (found with fontdb) rasterised with ab_glyph. Text is the one tool
//! that is antialiased by default, because it reads much better. With smoothing off, each
//! pixel is either fully painted or left alone, so the letters can be bucket-filled cleanly.

use crate::canvas::{IRect, Image};
use ab_glyph::{point, Font, FontVec, Glyph, PxScale, ScaleFont};
use eframe::egui::Color32;

/// Unsmoothed text: coverage at or above this paints the pixel. Slightly under half keeps
/// thin strokes of small text from breaking up.
const COVERAGE: f32 = 0.45;

/// Families tried, in order, when the user has not chosen one.
const PREFERRED: &[&str] = &["Arial", "Segoe UI", "DejaVu Sans", "Liberation Sans", "Noto Sans", "Cantarell", "Ubuntu"];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FontKey {
    pub family: String,
    pub bold: bool,
    pub italic: bool,
}

/// System font list, loaded the first time the text tool needs it.
#[derive(Default)]
pub struct Fonts {
    db: Option<fontdb::Database>,
    families: Vec<String>,
    cache: Option<(FontKey, Option<FontVec>)>,
}

impl Fonts {
    fn db(&mut self) -> &fontdb::Database {
        if self.db.is_none() {
            let mut db = fontdb::Database::new();
            db.load_system_fonts();
            let mut fams: Vec<String> =
                db.faces().filter_map(|f| f.families.first().map(|(name, _)| name.clone())).collect();
            fams.sort_by_key(|s| s.to_lowercase());
            fams.dedup();
            self.families = fams;
            self.db = Some(db);
        }
        self.db.as_ref().unwrap()
    }

    pub fn families(&mut self) -> &[String] {
        self.db();
        &self.families
    }

    pub fn default_family(&mut self) -> String {
        self.db();
        PREFERRED
            .iter()
            .find(|p| self.families.iter().any(|f| f == *p))
            .map(|s| s.to_string())
            .or_else(|| self.families.first().cloned())
            .unwrap_or_default()
    }

    /// The font for `key`, falling back to the closest weight/style the family has.
    pub fn font(&mut self, key: &FontKey) -> Option<&FontVec> {
        if self.cache.as_ref().map(|(k, _)| k) != Some(key) {
            let db = self.db();
            let query = fontdb::Query {
                families: &[fontdb::Family::Name(&key.family), fontdb::Family::SansSerif],
                weight: if key.bold { fontdb::Weight::BOLD } else { fontdb::Weight::NORMAL },
                stretch: fontdb::Stretch::Normal,
                style: if key.italic { fontdb::Style::Italic } else { fontdb::Style::Normal },
            };
            let font = db
                .query(&query)
                .and_then(|id| db.with_face_data(id, |data, index| FontVec::try_from_vec_and_index(data.to_vec(), index).ok()))
                .flatten();
            self.cache = Some((key.clone(), font));
        }
        self.cache.as_ref().and_then(|(_, f)| f.as_ref())
    }
}

/// One laid-out line: its vertical extent and the x position of every caret stop
/// (`xs.len()` is the number of characters on the line + 1).
#[derive(Clone, Debug, Default)]
pub struct Line {
    pub top: i32,
    pub bottom: i32,
    pub xs: Vec<i32>,
}

pub struct Layout {
    glyphs: Vec<Glyph>,
    /// The text box (lines x widest line); the background fill for opaque text.
    pub bbox: IRect,
    pub lines: Vec<Line>,
}

pub fn layout(font: &FontVec, size: f32, origin: (i32, i32), text: &str) -> Layout {
    let f = font.as_scaled(PxScale::from(size));
    let line_h = (f.ascent() - f.descent() + f.line_gap()).ceil().max(1.0);
    let x0 = origin.0 as f32;
    let mut glyphs = Vec::new();
    let mut lines = Vec::new();
    let mut widest = 0.0f32;
    for (i, line) in text.split('\n').enumerate() {
        let top = origin.1 as f32 + i as f32 * line_h;
        let baseline = (top + f.ascent()).round();
        let mut x = x0;
        let mut xs = vec![origin.0];
        let mut prev = None;
        for c in line.chars() {
            let mut g = f.scaled_glyph(c);
            if let Some(p) = prev {
                x += f.kern(p, g.id);
            }
            g.position = point(x, baseline);
            x += f.h_advance(g.id);
            prev = Some(g.id);
            glyphs.push(g);
            xs.push(x.round() as i32);
        }
        widest = widest.max(x - x0);
        lines.push(Line { top: top as i32, bottom: (top + line_h) as i32, xs });
    }
    let height = (lines.len() as f32 * line_h) as i32;
    let bbox = IRect::from_size(origin.0, origin.1, widest.ceil().max(1.0) as i32 + 1, height);
    Layout { glyphs, bbox, lines }
}

/// Paint the glyphs; returns everything touched (the text box plus any ink outside it,
/// e.g. italic overhang). `smooth` blends edge pixels over whatever is underneath.
pub fn draw(img: &mut Image, font: &FontVec, l: &Layout, color: Color32, smooth: bool) -> IRect {
    let mut touched = l.bbox;
    for g in &l.glyphs {
        if let Some(og) = font.outline_glyph(g.clone()) {
            let b = og.px_bounds();
            let (bx, by) = (b.min.x as i32, b.min.y as i32);
            og.draw(|x, y, c| {
                let (px, py) = (bx + x as i32, by + y as i32);
                if smooth {
                    if let Some(under) = img.get(px, py) {
                        img.set(px, py, blend(under, color, c));
                    }
                } else if c >= COVERAGE {
                    img.set(px, py, color);
                }
            });
            touched = touched.union(IRect::new(bx, by, b.max.x.ceil() as i32 + 1, b.max.y.ceil() as i32 + 1));
        }
    }
    touched.intersect(img.bounds())
}

fn blend(under: Color32, over: Color32, coverage: f32) -> Color32 {
    let a = coverage.clamp(0.0, 1.0);
    let mix = |u: u8, o: u8| (u as f32 + (o as f32 - u as f32) * a).round() as u8;
    Color32::from_rgb(mix(under.r(), over.r()), mix(under.g(), over.g()), mix(under.b(), over.b()))
}

// ---- caret ------------------------------------------------------------------------------
// The caret is a character index into the text (0 ..= number of characters). A newline
// counts as one character, so line `i` starts right after the i-th newline.

/// (line, column) of caret index `cursor`.
pub fn line_col(text: &str, cursor: usize) -> (usize, usize) {
    let mut line = 0;
    let mut col = 0;
    for c in text.chars().take(cursor) {
        if c == '\n' {
            line += 1;
            col = 0;
        } else {
            col += 1;
        }
    }
    (line, col)
}

/// Caret index of (line, column), with the column clamped to the line's length.
pub fn index_of(text: &str, line: usize, col: usize) -> usize {
    let mut idx = 0;
    for (i, l) in text.split('\n').enumerate() {
        let n = l.chars().count();
        if i == line {
            return idx + col.min(n);
        }
        idx += n + 1;
    }
    text.chars().count()
}

/// Where to draw the caret: (x, top, bottom).
pub fn caret_pos(lines: &[Line], text: &str, cursor: usize) -> Option<(i32, i32, i32)> {
    let (li, col) = line_col(text, cursor);
    let line = lines.get(li)?;
    Some((*line.xs.get(col).or(line.xs.last())?, line.top, line.bottom))
}

/// Column on `line` whose caret stop is nearest to x.
pub fn nearest_col(line: &Line, x: i32) -> usize {
    line.xs.iter().enumerate().min_by_key(|(_, &cx)| (cx - x).abs()).map(|(i, _)| i).unwrap_or(0)
}

/// Caret index nearest to a clicked pixel.
pub fn hit(lines: &[Line], text: &str, p: (i32, i32)) -> usize {
    let li = lines.iter().position(|l| p.1 < l.bottom).unwrap_or(lines.len().saturating_sub(1));
    match lines.get(li) {
        Some(line) => index_of(text, li, nearest_col(line, p.0)),
        None => 0,
    }
}

/// Byte offset of caret index `cursor`, for inserting into / removing from the String.
pub fn byte_at(text: &str, cursor: usize) -> usize {
    text.char_indices().nth(cursor).map(|(b, _)| b).unwrap_or(text.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_font() -> Option<(Fonts, FontKey)> {
        let mut fonts = Fonts::default();
        let key = FontKey { family: fonts.default_family(), bold: false, italic: false };
        fonts.font(&key)?;
        Some((fonts, key))
    }

    #[test]
    fn unsmoothed_text_is_aliased() {
        let Some((mut fonts, key)) = default_font() else { return };
        let font = fonts.font(&key).unwrap();
        let mut img = Image::new(200, 100, Color32::WHITE);
        let l = layout(font, 40.0, (10, 10), "Hi O\nab");
        assert!(l.bbox.height() >= 72, "two lines of 40px text");
        draw(&mut img, font, &l, Color32::BLACK, false);
        let inked = img.px.iter().filter(|p| **p == Color32::BLACK).count();
        assert!(inked > 100, "text drew something ({inked} px)");
        assert!(img.px.iter().all(|p| *p == Color32::BLACK || *p == Color32::WHITE), "no blended pixels");
    }

    #[test]
    fn smoothed_text_blends_with_the_background() {
        let Some((mut fonts, key)) = default_font() else { return };
        let font = fonts.font(&key).unwrap();
        let mut img = Image::new(200, 60, Color32::from_rgb(0, 0, 255));
        let l = layout(font, 40.0, (10, 5), "Ag");
        draw(&mut img, font, &l, Color32::from_rgb(255, 0, 0), true);
        let blended = img.px.iter().filter(|p| p.r() > 0 && p.r() < 255).count();
        assert!(blended > 20, "edge pixels mix text and background colours");
        // Only ever a mix of the red text and the blue background.
        assert!(img.px.iter().all(|p| p.g() == 0 && (p.r() as u32 + p.b() as u32) >= 250));
    }

    #[test]
    fn caret_positions() {
        let Some((mut fonts, key)) = default_font() else { return };
        let font = fonts.font(&key).unwrap();
        let text = "ab\ncde";
        let l = layout(font, 20.0, (0, 0), text);
        assert_eq!(l.lines.len(), 2);
        assert_eq!(l.lines[0].xs.len(), 3);
        assert_eq!(l.lines[1].xs.len(), 4);
        assert_eq!(line_col(text, 0), (0, 0));
        assert_eq!(line_col(text, 2), (0, 2));
        assert_eq!(line_col(text, 3), (1, 0));
        assert_eq!(line_col(text, 6), (1, 3));
        assert_eq!(index_of(text, 1, 2), 5);
        assert_eq!(index_of(text, 0, 9), 2, "clamped to the line end");
        let (x, top, _) = caret_pos(&l.lines, text, 3).unwrap();
        assert_eq!((x, top), (0, l.lines[1].top));
        assert_eq!(hit(&l.lines, text, (500, l.lines[1].top + 2)), 6, "click right of line 2 = its end");
        assert_eq!(byte_at("h\u{e9}llo", 2), 3, "multi-byte characters");
    }
}
