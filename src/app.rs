//! The application: UI layout, tool behaviour and input handling.

use crate::canvas::{IRect, Image};
use crate::doc::Doc;
use crate::io;
use crate::raster::{self, BrushShape, Mask, Rng, Spans};
use crate::text::{self, FontKey, Fonts};
use crate::tools::{FillStyle, Tool};
use eframe::egui::{
    self, pos2, vec2, Color32, CursorIcon, Event, Key, Modifiers, PointerButton, Pos2, Rect, Sense, Stroke, StrokeKind,
    TextureHandle, TextureOptions, Ui, Vec2,
};
use std::path::PathBuf;

const MARGIN: f32 = 8.0;
const TOOL_PANEL_WIDTH: f32 = 104.0;
const HANDLE: f32 = 7.0;
const ROUND_RADIUS: f64 = 10.0;
const ZOOMS: [f32; 13] = [0.125, 0.25, 0.5, 1.0, 2.0, 3.0, 4.0, 6.0, 8.0, 12.0, 16.0, 24.0, 32.0];
const CANVAS_BG: Color32 = Color32::from_rgb(196, 202, 212);
const DEFAULT_SIZE: (usize, usize) = (800, 600);

pub struct Options {
    brush_shape: BrushShape,
    brush_size: u32,
    eraser_size: u32,
    spray_radius: u32,
    line_width: u32,
    fill_style: FillStyle,
    tolerance: u8,
    fill_global: bool,
    transparent_sel: bool,
    grid: bool,
    /// Rectangles/ellipses grow from the clicked point as their centre.
    from_center: bool,
    text_family: String,
    text_size: u32,
    text_bold: bool,
    text_italic: bool,
    /// Fill the text box with the secondary colour.
    text_opaque: bool,
    /// Antialiased text (on by default; off = hard pixels that bucket-fill cleanly).
    text_smooth: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            brush_shape: BrushShape::Round,
            brush_size: 5,
            eraser_size: 8,
            spray_radius: 8,
            line_width: 1,
            fill_style: FillStyle::Outline,
            tolerance: 0,
            fill_global: false,
            transparent_sel: false,
            grid: true,
            from_center: false,
            text_family: String::new(),
            text_size: 24,
            text_bold: false,
            text_italic: false,
            text_opaque: false,
            text_smooth: true,
        }
    }
}

/// What the mouse is currently doing on the canvas.
#[derive(Default)]
enum Op {
    #[default]
    None,
    Freehand { last: (i32, i32), sec: bool },
    Spray { pos: (i32, i32), sec: bool },
    /// `center`: `start` is the centre and `end` sets the radius (ellipse / rectangles).
    Shape { start: (i32, i32), end: (i32, i32), center: bool, sec: bool, prev: IRect },
    /// Bezier: stage 0 = dragging the line, 1 = first bend, 2 = second bend.
    Curve { pts: [(i32, i32); 4], stage: u8, sec: bool, prev: IRect },
    Polygon { pts: Vec<(i32, i32)>, cursor: (i32, i32), dragging: bool, sec: bool, prev: IRect },
    Selecting { start: (i32, i32), end: (i32, i32) },
    MoveSel { grab: (i32, i32) },
    /// Typing: re-rendered every frame so font or colour changes show up immediately.
    /// `cursor` is a character index into `text`; `lines` and `caret` come from the last layout.
    Text {
        origin: (i32, i32),
        text: String,
        cursor: usize,
        sec: bool,
        prev: IRect,
        bbox: IRect,
        lines: Vec<text::Line>,
        caret: (i32, i32, i32),
    },
    /// mode bit 1 = width, bit 2 = height
    ResizeCanvas { mode: u8, size: (i32, i32) },
}

struct Selection {
    rect: IRect,
    /// Lifted pixels, once the selection has been moved or transformed.
    float: Option<Image>,
    version: u64,
}

#[derive(Clone)]
enum Pending {
    New,
    Open(Option<PathBuf>),
    Exit,
}

#[derive(Clone, Copy)]
enum ColorTarget {
    Primary,
    Secondary,
    Palette(usize),
}

enum Dialog {
    CanvasSize { w: u32, h: u32 },
    Stretch { h: u32, v: u32 },
    Unsaved(Pending),
    EditColor { target: ColorTarget, color: Color32 },
    About,
    Error(String),
}

enum Post {
    CanvasSize(u32, u32),
    Stretch(u32, u32),
    SaveThen(Pending),
    Do(Pending),
    SetColor(ColorTarget, Color32),
}

pub struct PaintApp {
    doc: Doc,
    tex: Option<TextureHandle>,
    sel_tex: Option<(TextureHandle, (u64, bool, Color32))>,
    hidden_tex: Option<TextureHandle>,
    tool: Tool,
    prev_tool: Tool,
    opts: Options,
    primary: Color32,
    secondary: Color32,
    /// Palette presets (edits to swatches stay with their preset) and the one shown.
    palettes: Vec<crate::palette::Preset>,
    palette_idx: usize,
    op: Op,
    button: Option<PointerButton>,
    sel: Option<Selection>,
    sel_version: u64,
    zoom: f32,
    zoom_accum: f32,
    ppp: f32,
    scale: f32,
    origin: Pos2,
    scroll_offset: Vec2,
    pending_scroll: Option<Vec2>,
    viewport: Rect,
    panning: Option<Pos2>,
    /// Where to paint the magnifier cursor this frame.
    magnifier: Option<Pos2>,
    hover: Option<(i32, i32)>,
    last_press: (f64, Pos2),
    last_paste: f64,
    rng: Rng,
    fonts: Fonts,
    dialog: Option<Dialog>,
    clipboard: Option<arboard::Clipboard>,
    allow_close: bool,
    want_close: bool,
    title: String,
}

fn snap(p: Pos2, ppp: f32) -> Pos2 {
    pos2((p.x * ppp).round() / ppp, (p.y * ppp).round() / ppp)
}

fn sgn(v: i32) -> i32 {
    if v < 0 {
        -1
    } else {
        1
    }
}

/// Snap `b` so the line a-b is horizontal, vertical or 45 degrees.
fn snap45(a: (i32, i32), b: (i32, i32)) -> (i32, i32) {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let (ax, ay) = (dx.abs(), dy.abs());
    if ax > 2 * ay {
        (b.0, a.1)
    } else if ay > 2 * ax {
        (a.0, b.1)
    } else {
        let d = ax.max(ay);
        (a.0 + d * sgn(dx), a.1 + d * sgn(dy))
    }
}

/// Snap `b` so the box a-b is square.
fn snap_square(a: (i32, i32), b: (i32, i32)) -> (i32, i32) {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let d = dx.abs().max(dy.abs());
    (a.0 + d * sgn(dx), a.1 + d * sgn(dy))
}

/// Bounding-box corners for a shape drag. In centre mode the box is `start ± radius`, so its
/// size is always odd and the shape is perfectly symmetric around the centre pixel.
fn shape_corners(start: (i32, i32), end: (i32, i32), center: bool) -> ((i32, i32), (i32, i32)) {
    if center {
        let (rx, ry) = ((end.0 - start.0).abs(), (end.1 - start.1).abs());
        ((start.0 - rx, start.1 - ry), (start.0 + rx, start.1 + ry))
    } else {
        (start, end)
    }
}

fn draw_shape(img: &mut Image, tool: Tool, a: (i32, i32), b: (i32, i32), fg: Color32, bg: Color32, o: &Options) -> IRect {
    if tool == Tool::Line {
        return raster::stroke(img, a, b, &Mask::new(BrushShape::Round, o.line_width), fg);
    }
    let r = IRect::from_corners(a, b);
    let w = o.line_width as i32;
    let make = |r: IRect, shrink: f64| match tool {
        Tool::Ellipse => Spans::ellipse(r),
        Tool::RoundRect => Spans::round_rect(r, ROUND_RADIUS - shrink),
        _ => Spans::rect(r),
    };
    let outer = make(r, 0.0);
    let inner = (w > 1).then(|| make(r.inset(w), w as f64));
    let (outline, fill) = match o.fill_style {
        FillStyle::Outline => (Some(fg), None),
        FillStyle::OutlineFill => (Some(fg), Some(bg)),
        FillStyle::Fill => (None, Some(fg)),
    };
    raster::draw_spans(img, &outer, inner.as_ref(), outline, fill)
}

fn draw_polygon(img: &mut Image, pts: &[(i32, i32)], closed: bool, fg: Color32, bg: Color32, o: &Options) -> IRect {
    let mut r = IRect::EMPTY;
    if closed && pts.len() >= 3 {
        match o.fill_style {
            FillStyle::Outline => {}
            FillStyle::OutlineFill => r = raster::fill_polygon(img, pts, bg),
            FillStyle::Fill => r = raster::fill_polygon(img, pts, fg),
        }
    }
    // A solid polygon still gets a 1px edge in its own colour so its border pixels are exact.
    let width = if closed && o.fill_style == FillStyle::Fill { 1 } else { o.line_width };
    r.union(raster::stroke_polyline(img, pts, &Mask::new(BrushShape::Round, width), fg, closed))
}

fn draw_curve(img: &mut Image, pts: [(i32, i32); 4], c: Color32, o: &Options) -> IRect {
    let p = raster::bezier_points(pts);
    raster::stroke_polyline(img, &p, &Mask::new(BrushShape::Round, o.line_width), c, false)
}

fn curve_set(pts: &mut [(i32, i32); 4], stage: u8, p: (i32, i32)) {
    match stage {
        0 => {
            pts[3] = p;
            pts[1] = pts[0];
            pts[2] = p;
        }
        1 => {
            pts[1] = p;
            pts[2] = p;
        }
        _ => pts[2] = p,
    }
}

impl PaintApp {
    pub fn new(cc: &eframe::CreationContext<'_>, file: Option<PathBuf>) -> Self {
        Self::create(&cc.egui_ctx, file)
    }

    fn create(ctx: &egui::Context, file: Option<PathBuf>) -> Self {
        egui_extras::install_image_loaders(ctx);
        let mut visuals = egui::Visuals::light();
        visuals.panel_fill = Color32::from_rgb(242, 243, 246);
        ctx.set_visuals(visuals);
        ctx.options_mut(|o| o.zoom_with_keyboard = false);
        ctx.style_mut(|s| s.spacing.button_padding = vec2(6.0, 3.0));

        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0x9E37_79B9);
        let mut app = Self {
            doc: Doc::new(Image::new(DEFAULT_SIZE.0, DEFAULT_SIZE.1, Color32::WHITE), None),
            tex: None,
            sel_tex: None,
            hidden_tex: None,
            tool: Tool::Pencil,
            prev_tool: Tool::Pencil,
            opts: Options::default(),
            primary: Color32::BLACK,
            secondary: Color32::WHITE,
            palettes: crate::palette::presets(),
            palette_idx: 0,
            op: Op::None,
            button: None,
            sel: None,
            sel_version: 0,
            zoom: 1.0,
            zoom_accum: 1.0,
            ppp: ctx.pixels_per_point(),
            scale: 1.0,
            origin: Pos2::ZERO,
            scroll_offset: Vec2::ZERO,
            pending_scroll: None,
            viewport: Rect::NOTHING,
            panning: None,
            magnifier: None,
            hover: None,
            last_press: (-1.0, Pos2::ZERO),
            last_paste: -1.0,
            rng: Rng::new(seed),
            fonts: Fonts::default(),
            dialog: None,
            clipboard: arboard::Clipboard::new().ok(),
            allow_close: false,
            want_close: false,
            title: String::new(),
        };
        if let Some(p) = file {
            app.open_path(p);
        }
        app
    }

    // ---- helpers ----------------------------------------------------------------------

    /// (drawing colour, other colour) - the right mouse button swaps them, as in Paint.
    fn colors(&self, sec: bool) -> (Color32, Color32) {
        if sec {
            (self.secondary, self.primary)
        } else {
            (self.primary, self.secondary)
        }
    }

    fn to_px(&self, p: Pos2) -> (i32, i32) {
        (((p.x - self.origin.x) / self.scale).floor() as i32, ((p.y - self.origin.y) / self.scale).floor() as i32)
    }

    fn px_center_screen(&self, p: (i32, i32)) -> Pos2 {
        self.origin + vec2((p.0 as f32 + 0.5) * self.scale, (p.1 as f32 + 0.5) * self.scale)
    }

    fn irect_to_screen(&self, r: IRect) -> Rect {
        let s = self.scale;
        Rect::from_min_max(
            self.origin + vec2(r.x0 as f32 * s, r.y0 as f32 * s),
            self.origin + vec2(r.x1 as f32 * s, r.y1 as f32 * s),
        )
    }

    fn clamp_px(&self, p: (i32, i32)) -> (i32, i32) {
        (p.0.clamp(0, self.doc.img.w as i32 - 1), p.1.clamp(0, self.doc.img.h as i32 - 1))
    }

    fn handle_rects(&self) -> [(u8, Rect); 3] {
        let (w, h) = (self.doc.img.w as f32 * self.scale, self.doc.img.h as f32 * self.scale);
        let o = self.origin;
        let d = HANDLE / 2.0 + 1.0;
        let sz = vec2(HANDLE, HANDLE);
        [
            (1, Rect::from_center_size(o + vec2(w + d, h / 2.0), sz)),
            (2, Rect::from_center_size(o + vec2(w / 2.0, h + d), sz)),
            (3, Rect::from_center_size(o + vec2(w + d, h + d), sz)),
        ]
    }

    fn handle_hit(&self, pos: Pos2) -> Option<u8> {
        self.handle_rects().iter().find(|(_, r)| r.expand(3.0).contains(pos)).map(|(m, _)| *m)
    }

    fn display_name(&self) -> String {
        self.doc
            .path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Untitled".to_owned())
    }

    // ---- zoom -------------------------------------------------------------------------

    fn set_zoom(&mut self, z: f32, anchor: Option<Pos2>) {
        let z = z.clamp(ZOOMS[0], ZOOMS[ZOOMS.len() - 1]);
        if z == self.zoom {
            return;
        }
        let anchor = anchor.filter(|a| self.viewport.contains(*a)).unwrap_or(self.viewport.center());
        let in_view = anchor - self.viewport.min;
        let content = self.pending_scroll.unwrap_or(self.scroll_offset) + in_view;
        let img_pt = (content - vec2(MARGIN, MARGIN)) / self.scale;
        let new_scale = z / self.ppp;
        let new_content = img_pt * new_scale + vec2(MARGIN, MARGIN);
        self.pending_scroll = Some((new_content - in_view).max(Vec2::ZERO));
        self.zoom = z;
        self.scale = new_scale;
    }

    fn zoom_step(&mut self, dir: i32, anchor: Option<Pos2>) {
        if let Some(z) = self.next_zoom(dir) {
            self.set_zoom(z, anchor);
        }
    }

    // ---- operations -------------------------------------------------------------------

    fn set_tool(&mut self, t: Tool) {
        if t == self.tool {
            return;
        }
        self.finish_op();
        if t != Tool::Select {
            self.commit_selection();
        }
        if t == Tool::Picker {
            self.prev_tool = self.tool;
        }
        self.tool = t;
    }

    /// Complete whatever is in progress (used before switching tools, saving, menus...).
    fn finish_op(&mut self) {
        let op = std::mem::take(&mut self.op);
        self.button = None;
        match op {
            Op::Polygon { pts, sec, prev, .. } => self.finalize_polygon(&pts, sec, prev),
            Op::Curve { .. } | Op::Shape { .. } | Op::Freehand { .. } | Op::Spray { .. } => {
                self.doc.commit();
            }
            // Nothing typed: drop the (possibly opaque) empty box instead of keeping it.
            Op::Text { text, .. } if text.is_empty() => self.doc.revert(),
            Op::Text { .. } => {
                self.doc.commit();
            }
            _ => {}
        }
    }

    fn font_key(&mut self) -> FontKey {
        if self.opts.text_family.is_empty() {
            self.opts.text_family = self.fonts.default_family();
        }
        FontKey { family: self.opts.text_family.clone(), bold: self.opts.text_bold, italic: self.opts.text_italic }
    }

    fn cancel_op(&mut self) {
        self.op = Op::None;
        self.button = None;
        self.doc.revert();
    }

    fn finalize_polygon(&mut self, pts: &[(i32, i32)], sec: bool, prev: IRect) {
        self.doc.restore(prev);
        if pts.len() >= 2 {
            let (fg, bg) = self.colors(sec);
            let r = draw_polygon(&mut self.doc.img, pts, true, fg, bg, &self.opts);
            self.doc.mark(r);
        }
        self.doc.commit();
    }

    fn redraw_preview(&mut self, op: &mut Op) {
        match op {
            Op::Text { origin, text, cursor, sec, prev, bbox, lines, caret } => {
                self.doc.restore(*prev);
                let (fg, bg) = self.colors(*sec);
                let key = self.font_key();
                let size = self.opts.text_size as f32;
                let (opaque, smooth) = (self.opts.text_opaque, self.opts.text_smooth);
                if let Some(font) = self.fonts.font(&key) {
                    let l = text::layout(font, size, *origin, text);
                    if opaque {
                        self.doc.img.fill_rect(l.bbox, bg);
                    }
                    let r = text::draw(&mut self.doc.img, font, &l, fg, smooth);
                    self.doc.mark(r);
                    *prev = r;
                    *bbox = l.bbox;
                    *caret = text::caret_pos(&l.lines, text, *cursor).unwrap_or(*caret);
                    *lines = l.lines;
                }
            }
            Op::Shape { start, end, center, sec, prev } => {
                self.doc.restore(*prev);
                let (fg, bg) = self.colors(*sec);
                let (a, b) = shape_corners(*start, *end, *center);
                let r = draw_shape(&mut self.doc.img, self.tool, a, b, fg, bg, &self.opts);
                self.doc.mark(r);
                *prev = r;
            }
            Op::Curve { pts, sec, prev, .. } => {
                self.doc.restore(*prev);
                let (fg, _) = self.colors(*sec);
                let r = draw_curve(&mut self.doc.img, *pts, fg, &self.opts);
                self.doc.mark(r);
                *prev = r;
            }
            Op::Polygon { pts, cursor, dragging, sec, prev } => {
                self.doc.restore(*prev);
                let mut v = pts.clone();
                if *dragging && v.last() != Some(cursor) {
                    v.push(*cursor);
                }
                let (fg, bg) = self.colors(*sec);
                let r = draw_polygon(&mut self.doc.img, &v, false, fg, bg, &self.opts);
                self.doc.mark(r);
                *prev = r;
            }
            _ => {}
        }
    }

    fn freehand_segment(&mut self, a: (i32, i32), b: (i32, i32), sec: bool) -> IRect {
        let (fg, _) = self.colors(sec);
        let img = &mut self.doc.img;
        match self.tool {
            Tool::Pencil => raster::stroke(img, a, b, &Mask::new(BrushShape::Square, 1), fg),
            Tool::Brush => raster::stroke(img, a, b, &Mask::new(self.opts.brush_shape, self.opts.brush_size), fg),
            Tool::Eraser => {
                let m = Mask::new(BrushShape::Square, self.opts.eraser_size);
                if sec {
                    raster::stroke_replace(img, a, b, &m, self.primary, self.secondary)
                } else {
                    raster::stroke(img, a, b, &m, self.secondary)
                }
            }
            _ => IRect::EMPTY,
        }
    }

    fn on_press(&mut self, px: (i32, i32), pos: Pos2, sec: bool, mods: Modifiers, time: f64) {
        let dbl = time - self.last_press.0 < 0.45 && (pos - self.last_press.1).length() < 6.0;
        self.last_press = (time, pos);
        if let Some(mode) = self.handle_hit(pos) {
            // finish_op() forgets the held button; keep it, or the release is ignored
            // and the resize only completes on the *next* click.
            let held = self.button;
            self.finish_op();
            self.button = held;
            self.commit_selection();
            self.op = Op::ResizeCanvas { mode, size: (self.doc.img.w as i32, self.doc.img.h as i32) };
            return;
        }
        let mut op = std::mem::take(&mut self.op);
        match self.tool {
            Tool::Pencil | Tool::Brush | Tool::Eraser => {
                let r = self.freehand_segment(px, px, sec);
                self.doc.mark(r);
                op = Op::Freehand { last: px, sec };
            }
            Tool::Airbrush => op = Op::Spray { pos: px, sec },
            Tool::Text => {
                let inside = matches!(&op, Op::Text { bbox, .. } if bbox.contains(px.0, px.1));
                if let (true, Op::Text { text, cursor, lines, .. }) = (inside, &mut op) {
                    // Click inside the text being typed: move the caret there.
                    *cursor = text::hit(lines, text, px);
                    self.redraw_preview(&mut op);
                } else {
                    // Place the text being typed (if any), then start a new one here.
                    let held = self.button;
                    self.op = std::mem::take(&mut op);
                    self.finish_op();
                    self.button = held;
                    op = Op::Text {
                        origin: px,
                        text: String::new(),
                        cursor: 0,
                        sec,
                        prev: IRect::EMPTY,
                        bbox: IRect::EMPTY,
                        lines: Vec::new(),
                        caret: (px.0, px.1, px.1 + self.opts.text_size as i32),
                    };
                    self.redraw_preview(&mut op);
                }
            }
            Tool::Fill => {
                let (fg, _) = self.colors(sec);
                let r = if self.opts.fill_global {
                    raster::fill_global(&mut self.doc.img, px.0, px.1, fg, self.opts.tolerance)
                } else {
                    raster::flood_fill(&mut self.doc.img, px.0, px.1, fg, self.opts.tolerance)
                };
                self.doc.mark(r);
                self.doc.commit();
            }
            Tool::Picker => {
                if let Some(c) = self.doc.img.get(px.0, px.1) {
                    if sec {
                        self.secondary = c;
                    } else {
                        self.primary = c;
                    }
                    self.tool = if self.prev_tool == Tool::Picker { Tool::Pencil } else { self.prev_tool };
                }
            }
            Tool::Zoom => self.zoom_step(if sec { -1 } else { 1 }, Some(pos)),
            Tool::Line | Tool::Rect | Tool::Ellipse | Tool::RoundRect => {
                let center = self.tool != Tool::Line && (self.opts.from_center != mods.alt);
                op = Op::Shape { start: px, end: px, center, sec, prev: IRect::EMPTY };
                self.redraw_preview(&mut op);
            }
            Tool::Curve => {
                match &mut op {
                    Op::Curve { pts, stage, .. } if *stage >= 1 => curve_set(pts, *stage, px),
                    _ => op = Op::Curve { pts: [px; 4], stage: 0, sec, prev: IRect::EMPTY },
                }
                self.redraw_preview(&mut op);
            }
            Tool::Polygon => {
                let finish = match &op {
                    Op::Polygon { pts, .. } => dbl || (pts.len() >= 3 && (pos - self.px_center_screen(pts[0])).length() < 8.0),
                    _ => false,
                };
                if finish {
                    if let Op::Polygon { pts, sec, prev, .. } = std::mem::take(&mut op) {
                        self.finalize_polygon(&pts, sec, prev);
                    }
                } else {
                    match &mut op {
                        Op::Polygon { cursor, dragging, .. } => {
                            *cursor = px;
                            *dragging = true;
                        }
                        _ => op = Op::Polygon { pts: vec![px], cursor: px, dragging: true, sec, prev: IRect::EMPTY },
                    }
                    self.redraw_preview(&mut op);
                }
            }
            Tool::Select => {
                let inside = self.sel.as_ref().is_some_and(|s| s.rect.contains(px.0, px.1));
                if inside {
                    let had_float = self.sel.as_ref().is_some_and(|s| s.float.is_some());
                    if had_float && mods.command {
                        // Ctrl+drag a floating selection: leave a copy behind ("stamp").
                        let key = self.opts.transparent_sel.then_some(self.secondary);
                        let s = self.sel.as_ref().unwrap();
                        let r = self.doc.img.blit(s.float.as_ref().unwrap(), s.rect.x0, s.rect.y0, key);
                        self.doc.mark(r);
                    }
                    self.lift_selection(mods.command);
                    let r = self.sel.as_ref().unwrap().rect;
                    op = Op::MoveSel { grab: (px.0 - r.x0, px.1 - r.y0) };
                } else {
                    self.commit_selection();
                    let p = self.clamp_px(px);
                    op = Op::Selecting { start: p, end: p };
                }
            }
        }
        self.op = op;
    }

    fn on_move(&mut self, px: (i32, i32), pos: Pos2, mods: Modifiers) {
        if self.button.is_none() {
            return;
        }
        let mut op = std::mem::take(&mut self.op);
        let mut changed = false;
        match &mut op {
            Op::Freehand { last, sec } => {
                let a = *last;
                *last = px;
                let r = self.freehand_segment(a, px, *sec);
                self.doc.mark(r);
            }
            Op::Spray { pos: p, .. } => *p = px,
            Op::Shape { start, end, center, .. } => {
                *center = self.tool != Tool::Line && (self.opts.from_center != mods.alt);
                *end = match (mods.shift, self.tool, *center) {
                    (true, Tool::Line, _) => snap45(*start, px),
                    // Circle / square around the centre: equal radius both ways.
                    (true, _, true) => {
                        let d = (px.0 - start.0).abs().max((px.1 - start.1).abs());
                        (start.0 + d, start.1 + d)
                    }
                    (true, _, false) => snap_square(*start, px),
                    _ => px,
                };
                changed = true;
            }
            Op::Curve { pts, stage, .. } => {
                curve_set(pts, *stage, px);
                changed = true;
            }
            Op::Polygon { pts, cursor, .. } => {
                *cursor = if mods.shift { snap45(*pts.last().unwrap(), px) } else { px };
                changed = true;
            }
            Op::Selecting { end, .. } => *end = self.clamp_px(px),
            Op::MoveSel { grab } => {
                if let Some(s) = self.sel.as_mut() {
                    s.rect = IRect::from_size(px.0 - grab.0, px.1 - grab.1, s.rect.width(), s.rect.height());
                }
            }
            Op::ResizeCanvas { mode, size } => {
                let max = io::MAX_DIM as i32;
                if *mode & 1 != 0 {
                    size.0 = (((pos.x - self.origin.x) / self.scale).round() as i32).clamp(1, max);
                }
                if *mode & 2 != 0 {
                    size.1 = (((pos.y - self.origin.y) / self.scale).round() as i32).clamp(1, max);
                }
            }
            Op::Text { .. } | Op::None => {}
        }
        if changed {
            self.redraw_preview(&mut op);
        }
        self.op = op;
    }

    fn on_release(&mut self) {
        match std::mem::take(&mut self.op) {
            Op::Freehand { .. } | Op::Spray { .. } | Op::Shape { .. } => {
                self.doc.commit();
            }
            Op::Curve { pts, stage, sec, prev } => {
                if stage == 0 && pts[0] == pts[3] {
                    self.doc.revert();
                } else if stage >= 2 {
                    self.doc.commit();
                } else {
                    self.op = Op::Curve { pts, stage: stage + 1, sec, prev };
                }
            }
            Op::Polygon { mut pts, cursor, sec, prev, .. } => {
                if pts.last() != Some(&cursor) {
                    pts.push(cursor);
                }
                let mut op = Op::Polygon { pts, cursor, dragging: false, sec, prev };
                self.redraw_preview(&mut op);
                self.op = op;
            }
            Op::Selecting { start, end } => {
                let r = IRect::from_corners(start, end).intersect(self.doc.img.bounds());
                if start != end && !r.is_empty() {
                    self.sel_version += 1;
                    self.sel = Some(Selection { rect: r, float: None, version: self.sel_version });
                }
            }
            Op::ResizeCanvas { size, .. } => {
                if size != (self.doc.img.w as i32, self.doc.img.h as i32) {
                    self.doc.resize_canvas(size.0 as usize, size.1 as usize, self.secondary);
                    self.doc.commit();
                }
            }
            text @ Op::Text { .. } => self.op = text, // keep typing after the click
            Op::MoveSel { .. } | Op::None => {}
        }
    }

    // ---- selection --------------------------------------------------------------------

    fn lift_selection(&mut self, copy: bool) {
        let Some(s) = self.sel.as_mut() else { return };
        if s.float.is_none() {
            s.float = Some(self.doc.img.crop(s.rect));
            if !copy {
                let r = self.doc.img.fill_rect(s.rect, self.secondary);
                self.doc.mark(r);
            }
            self.sel_version += 1;
            s.version = self.sel_version;
        }
    }

    fn commit_selection(&mut self) {
        if let Some(s) = self.sel.take() {
            if let Some(f) = s.float {
                let key = self.opts.transparent_sel.then_some(self.secondary);
                let r = self.doc.img.blit(&f, s.rect.x0, s.rect.y0, key);
                self.doc.mark(r);
                self.doc.commit();
            }
        }
    }

    fn selection_pixels(&self) -> Option<Image> {
        let s = self.sel.as_ref()?;
        Some(match &s.float {
            Some(f) => f.clone(),
            None => self.doc.img.crop(s.rect),
        })
    }

    fn select_all(&mut self) {
        self.finish_op();
        self.commit_selection();
        self.tool = Tool::Select;
        self.sel_version += 1;
        self.sel = Some(Selection { rect: self.doc.img.bounds(), float: None, version: self.sel_version });
    }

    fn delete_selection(&mut self) {
        self.finish_op();
        if let Some(s) = self.sel.take() {
            if s.float.is_none() {
                let r = self.doc.img.fill_rect(s.rect, self.secondary);
                self.doc.mark(r);
            }
            self.doc.commit();
        }
    }

    fn copy(&mut self) -> bool {
        let Some(img) = self.selection_pixels() else { return false };
        let Some(cb) = self.clipboard.as_mut() else { return false };
        cb.set_image(arboard::ImageData { width: img.w, height: img.h, bytes: img.to_rgba().into() }).is_ok()
    }

    fn cut(&mut self) {
        if self.copy() {
            self.delete_selection();
        }
    }

    fn paste(&mut self) {
        let Some(cb) = self.clipboard.as_mut() else { return };
        let Ok(data) = cb.get_image() else { return };
        let img = Image::from_rgba(data.width, data.height, &data.bytes);
        self.finish_op();
        self.commit_selection();
        if img.w > self.doc.img.w || img.h > self.doc.img.h {
            let (w, h) = (img.w.max(self.doc.img.w), img.h.max(self.doc.img.h));
            self.doc.resize_canvas(w, h, self.secondary);
        }
        // Paste at the top-left of what is currently visible, like Paint.
        let view = (self.scroll_offset - vec2(MARGIN, MARGIN)) / self.scale;
        let x = (view.x.max(0.0) as i32).min(self.doc.img.w as i32 - img.w as i32).max(0);
        let y = (view.y.max(0.0) as i32).min(self.doc.img.h as i32 - img.h as i32).max(0);
        self.tool = Tool::Select;
        self.sel_version += 1;
        self.sel = Some(Selection {
            rect: IRect::from_size(x, y, img.w as i32, img.h as i32),
            float: Some(img),
            version: self.sel_version,
        });
    }

    fn crop_to_selection(&mut self) {
        self.finish_op();
        if let Some(img) = self.selection_pixels() {
            self.sel = None;
            self.doc.discard_hidden();
            self.doc.set_image(img);
            self.doc.commit();
        }
    }

    /// Apply a transform to the selection if there is one, otherwise to the whole picture.
    /// `moves_pixels`: flips and rotations no longer line up with hidden pixels, so those
    /// are discarded; colour-only changes (invert) leave them frozen, like drawing does.
    fn transform(&mut self, moves_pixels: bool, f: impl Fn(&Image) -> Image) {
        self.finish_op();
        if self.sel.is_some() {
            self.lift_selection(false);
            let s = self.sel.as_mut().unwrap();
            let nf = f(s.float.as_ref().unwrap());
            s.rect = IRect::from_size(s.rect.x0, s.rect.y0, nf.w as i32, nf.h as i32);
            s.float = Some(nf);
            self.sel_version += 1;
            s.version = self.sel_version;
        } else {
            let n = f(&self.doc.img);
            if moves_pixels {
                self.doc.discard_hidden();
            }
            self.doc.set_image(n);
            self.doc.commit();
        }
    }

    fn clear_image(&mut self) {
        self.finish_op();
        self.sel = None;
        self.doc.revert();
        let r = self.doc.img.fill_rect(self.doc.img.bounds(), self.secondary);
        self.doc.mark(r);
        self.doc.commit();
    }

    fn undo(&mut self) {
        if !matches!(self.op, Op::None | Op::MoveSel { .. }) {
            self.cancel_op();
            return;
        }
        self.op = Op::None;
        self.commit_selection();
        self.doc.undo();
    }

    fn redo(&mut self) {
        self.finish_op();
        self.commit_selection();
        self.doc.redo();
    }

    fn escape(&mut self) {
        match self.op {
            Op::None | Op::MoveSel { .. } => {
                self.op = Op::None;
                self.commit_selection();
            }
            Op::Selecting { .. } | Op::ResizeCanvas { .. } => self.op = Op::None,
            _ => self.cancel_op(),
        }
    }

    // ---- files ------------------------------------------------------------------------

    fn request(&mut self, p: Pending) {
        if self.doc.modified {
            self.dialog = Some(Dialog::Unsaved(p));
        } else {
            self.do_pending(p);
        }
    }

    fn do_pending(&mut self, p: Pending) {
        match p {
            Pending::New => {
                let (w, h) = (self.doc.img.w, self.doc.img.h);
                self.reset_doc(Doc::new(Image::new(w, h, Color32::WHITE), None));
            }
            Pending::Open(Some(path)) => self.open_path(path),
            Pending::Open(None) => {
                if let Some(path) = rfd::FileDialog::new().add_filter("Pictures", io::OPEN_EXTS).pick_file() {
                    self.open_path(path);
                }
            }
            Pending::Exit => {
                self.allow_close = true;
                self.want_close = true;
            }
        }
    }

    fn reset_doc(&mut self, doc: Doc) {
        self.op = Op::None;
        self.button = None;
        self.sel = None;
        self.doc = doc;
        self.pending_scroll = Some(Vec2::ZERO);
    }

    fn open_path(&mut self, path: PathBuf) {
        match io::load(&path) {
            Ok(img) => self.reset_doc(Doc::new(img, Some(path))),
            Err(e) => self.dialog = Some(Dialog::Error(e)),
        }
    }

    fn save(&mut self) -> bool {
        self.finish_op();
        self.commit_selection();
        match self.doc.path.clone() {
            Some(p) => self.write_to(p),
            None => self.save_as(),
        }
    }

    fn save_as(&mut self) -> bool {
        self.finish_op();
        self.commit_selection();
        let name = self.doc.path.as_ref().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned());
        let dialog = rfd::FileDialog::new()
            .set_file_name(name.unwrap_or_else(|| "untitled.png".into()))
            .add_filter("PNG", &["png"])
            .add_filter("Bitmap", &["bmp"])
            .add_filter("JPEG", &["jpg", "jpeg"])
            .add_filter("GIF", &["gif"])
            .add_filter("TIFF", &["tif", "tiff"])
            .add_filter("WebP", &["webp"]);
        let Some(mut p) = dialog.save_file() else { return false };
        if p.extension().is_none() {
            p.set_extension("png");
        }
        self.write_to(p)
    }

    fn write_to(&mut self, p: PathBuf) -> bool {
        match io::save(&self.doc.img, &p) {
            Ok(()) => {
                self.doc.path = Some(p);
                self.doc.modified = false;
                true
            }
            Err(e) => {
                self.dialog = Some(Dialog::Error(e));
                false
            }
        }
    }

    // ---- input ------------------------------------------------------------------------

    /// Feed keyboard input into the text being typed.
    fn type_text(&mut self, ctx: &egui::Context) {
        let events = ctx.input(|i| i.events.clone());
        let mut escape = false;
        if let Op::Text { text, cursor, lines, .. } = &mut self.op {
            let insert = |text: &mut String, cursor: &mut usize, s: &str| {
                text.insert_str(text::byte_at(text, *cursor), s);
                *cursor += s.chars().count();
            };
            for e in events {
                let len = text.chars().count();
                *cursor = (*cursor).min(len);
                match e {
                    Event::Text(s) => {
                        let s: String = s.chars().filter(|c| !c.is_control()).collect();
                        insert(text, cursor, &s);
                    }
                    Event::Paste(s) => insert(text, cursor, &s.replace("\r\n", "\n")),
                    Event::Key { key, pressed: true, .. } => match key {
                        Key::Enter => insert(text, cursor, "\n"),
                        Key::Escape => escape = true,
                        Key::Backspace if *cursor > 0 => {
                            *cursor -= 1;
                            text.remove(text::byte_at(text, *cursor));
                        }
                        Key::Delete if *cursor < len => {
                            text.remove(text::byte_at(text, *cursor));
                        }
                        Key::ArrowLeft => *cursor = cursor.saturating_sub(1),
                        Key::ArrowRight => *cursor = (*cursor + 1).min(len),
                        Key::Home | Key::End => {
                            let (line, _) = text::line_col(text, *cursor);
                            let col = if key == Key::Home { 0 } else { usize::MAX };
                            *cursor = text::index_of(text, line, col);
                        }
                        Key::ArrowUp | Key::ArrowDown => {
                            // Keep the caret's horizontal position, like a text editor.
                            let (line, col) = text::line_col(text, *cursor);
                            let target = if key == Key::ArrowUp { line.checked_sub(1) } else { Some(line + 1) };
                            if let (Some(t), Some(from)) = (target, lines.get(line)) {
                                if let Some(to) = lines.get(t) {
                                    let x = from.xs.get(col).copied().unwrap_or(0);
                                    *cursor = text::index_of(text, t, text::nearest_col(to, x));
                                }
                            }
                        }
                        _ => {}
                    },
                    _ => {}
                }
            }
        }
        // These keys mean something else outside text mode; keep them from leaking through.
        ctx.input_mut(|i| {
            for key in [
                Key::Enter,
                Key::Escape,
                Key::Delete,
                Key::Backspace,
                Key::ArrowLeft,
                Key::ArrowRight,
                Key::ArrowUp,
                Key::ArrowDown,
                Key::Home,
                Key::End,
            ] {
                i.consume_key(Modifiers::NONE, key);
            }
        });
        if escape {
            self.cancel_op();
        }
    }

    fn shortcuts(&mut self, ctx: &egui::Context) {
        let cmd = Modifiers::COMMAND;
        let cs = Modifiers::COMMAND | Modifiers::SHIFT;
        let none = Modifiers::NONE;
        let k = |m: Modifiers, key: Key| ctx.input_mut(|i| i.consume_key(m, key));

        let typing = matches!(self.op, Op::Text { .. }) && !ctx.wants_keyboard_input();
        if typing {
            self.type_text(ctx);
        }

        if k(cs, Key::Z) || k(cmd, Key::Y) {
            self.redo();
        } else if k(cmd, Key::Z) {
            self.undo();
        }
        if k(cs, Key::S) {
            self.save_as();
        } else if k(cmd, Key::S) {
            self.save();
        }
        if k(cs, Key::N) {
            self.clear_image();
        } else if k(cmd, Key::N) {
            self.request(Pending::New);
        }
        if k(cmd, Key::O) {
            self.request(Pending::Open(None));
        }
        if k(cmd, Key::A) {
            self.select_all();
        }
        if k(cmd, Key::E) {
            self.dialog = Some(Dialog::CanvasSize { w: self.doc.img.w as u32, h: self.doc.img.h as u32 });
        }
        if k(cmd, Key::W) {
            self.dialog = Some(Dialog::Stretch { h: 100, v: 100 });
        }
        if k(cmd, Key::I) {
            self.transform(false, Image::invert);
        }
        if k(cmd, Key::G) {
            self.opts.grid = !self.opts.grid;
        }
        if k(cmd, Key::PageUp) || k(cmd, Key::Plus) || k(cmd, Key::Equals) {
            self.zoom_step(1, ctx.pointer_hover_pos());
        }
        if k(cmd, Key::PageDown) || k(cmd, Key::Minus) {
            self.zoom_step(-1, ctx.pointer_hover_pos());
        }
        if k(cmd, Key::Num0) {
            self.set_zoom(1.0, None);
        }
        if k(none, Key::Delete) {
            self.delete_selection();
        }
        if k(none, Key::Escape) {
            self.escape();
        }
        if k(none, Key::Enter) {
            self.finish_op();
        }

        // egui turns Ctrl+C/X/V into Copy/Cut/Paste events, and drops Ctrl+V entirely when
        // the clipboard holds only an image - so also accept the V key being released.
        let (copy, cut, paste) = ctx.input(|i| {
            let mut r = (false, false, false);
            for e in &i.events {
                match e {
                    Event::Copy => r.0 = true,
                    Event::Cut => r.1 = true,
                    Event::Paste(_) => r.2 = true,
                    Event::Key { key: Key::V, pressed: false, modifiers, .. } if modifiers.command => r.2 = true,
                    _ => {}
                }
            }
            r
        });
        if copy {
            self.copy();
        }
        if cut {
            if ctx.input(|i| i.modifiers.shift) {
                self.crop_to_selection(); // Ctrl+Shift+X
            } else {
                self.cut();
            }
        }
        let now = ctx.input(|i| i.time);
        if paste && !typing && now - self.last_paste > 0.3 {
            self.last_paste = now;
            self.paste();
        }

        if !typing && !ctx.wants_keyboard_input() {
            for t in Tool::ALL {
                if k(none, t.key()) {
                    self.set_tool(t);
                }
            }
            if k(none, Key::X) {
                std::mem::swap(&mut self.primary, &mut self.secondary);
            }
        }
    }

    fn canvas_input(&mut self, ctx: &egui::Context, resp: &egui::Response, clip: Rect) {
        let (events, time) = ctx.input(|i| (i.events.clone(), i.time));
        self.magnifier = None;

        if resp.hovered() {
            let zd = ctx.input(|i| i.zoom_delta());
            if zd != 1.0 {
                self.zoom_accum *= zd;
                let anchor = ctx.pointer_hover_pos();
                if self.zoom_accum > 1.15 {
                    self.zoom_accum = 1.0;
                    self.zoom_step(1, anchor);
                } else if self.zoom_accum < 1.0 / 1.15 {
                    self.zoom_accum = 1.0;
                    self.zoom_step(-1, anchor);
                }
            }
        }

        for ev in &events {
            match ev {
                Event::PointerMoved(pos) => {
                    if let Some(last) = self.panning {
                        let base = self.pending_scroll.unwrap_or(self.scroll_offset);
                        self.pending_scroll = Some((base - (*pos - last)).max(Vec2::ZERO));
                        self.panning = Some(*pos);
                    } else {
                        let px = self.to_px(*pos);
                        self.on_move(px, *pos, ctx.input(|i| i.modifiers));
                    }
                }
                Event::PointerButton { pos, button, pressed: true, modifiers } => {
                    if self.button.is_some() || self.panning.is_some() || !resp.hovered() || !clip.contains(*pos) {
                        continue;
                    }
                    match button {
                        PointerButton::Middle => self.panning = Some(*pos),
                        PointerButton::Primary | PointerButton::Secondary => {
                            self.button = Some(*button);
                            let px = self.to_px(*pos);
                            self.on_press(px, *pos, *button == PointerButton::Secondary, *modifiers, time);
                        }
                        _ => {}
                    }
                }
                Event::PointerButton { pos, button, pressed: false, modifiers } => {
                    if *button == PointerButton::Middle {
                        self.panning = None;
                    } else if self.button == Some(*button) {
                        let px = self.to_px(*pos);
                        self.on_move(px, *pos, *modifiers);
                        self.button = None;
                        self.on_release();
                    }
                }
                _ => {}
            }
        }

        if let Some(p) = ctx.pointer_hover_pos() {
            let px = self.to_px(p);
            self.hover = self.doc.img.in_bounds(px.0, px.1).then_some(px);
        } else {
            self.hover = None;
        }

        // Text being typed is re-rendered each frame, so typing, font and colour
        // changes all show immediately.
        if matches!(self.op, Op::Text { .. }) {
            let mut op = std::mem::take(&mut self.op);
            self.redraw_preview(&mut op);
            self.op = op;
        }

        // Airbrush keeps spraying while the button is held still.
        if let Op::Spray { pos, sec } = self.op {
            let dt = ctx.input(|i| i.stable_dt).min(0.1);
            let r = self.opts.spray_radius as i32;
            let n = ((r * r) as f32 * dt * 6.0).ceil() as usize;
            let (fg, _) = self.colors(sec);
            let rr = raster::spray(&mut self.doc.img, pos, r, n, fg, &mut self.rng);
            self.doc.mark(rr);
            ctx.request_repaint();
        }

        // Cursor
        if resp.hovered() || self.button.is_some() {
            let hp = ctx.pointer_hover_pos();
            let resize_mode = match &self.op {
                Op::ResizeCanvas { mode, .. } => Some(*mode),
                Op::None => hp.and_then(|p| self.handle_hit(p)),
                _ => None,
            };
            let icon = match (&self.op, resize_mode) {
                (_, Some(1)) => CursorIcon::ResizeHorizontal,
                (_, Some(2)) => CursorIcon::ResizeVertical,
                (_, Some(_)) => CursorIcon::ResizeNwSe,
                (Op::MoveSel { .. }, _) => CursorIcon::Grabbing,
                _ if self.panning.is_some() => CursorIcon::Grabbing,
                _ if self.tool == Tool::Select
                    && self.hover.is_some_and(|p| self.sel.as_ref().is_some_and(|s| s.rect.contains(p.0, p.1))) =>
                {
                    CursorIcon::Move
                }
                // There is no zoom cursor on Windows, so hide the system cursor and
                // paint a magnifying glass ourselves (see paint_canvas).
                _ if self.tool == Tool::Text => CursorIcon::Text,
                _ if self.tool == Tool::Zoom => {
                    self.magnifier = hp;
                    CursorIcon::None
                }
                _ => CursorIcon::Crosshair,
            };
            ctx.set_cursor_icon(icon);
        }
    }

    /// The zoom level one step in or out from the current one, if any.
    fn next_zoom(&self, dir: i32) -> Option<f32> {
        if dir > 0 {
            ZOOMS.iter().copied().find(|&z| z > self.zoom)
        } else {
            ZOOMS.iter().rev().copied().find(|&z| z < self.zoom)
        }
    }

    fn paint_magnifier(&self, p: &egui::Painter, at: Pos2) {
        // Area that will fill the view after zooming in (zoom keeps the point under the cursor fixed).
        if let Some(nz) = self.next_zoom(1) {
            let vp = self.viewport;
            if vp.is_positive() {
                let k = self.zoom / nz;
                let r = Rect::from_min_size(at - (at - vp.min) * k, vp.size() * k);
                p.rect_stroke(r, 0.0, Stroke::new(1.0_f32, Color32::from_rgb(40, 90, 200)), StrokeKind::Inside);
            }
        }
        // The lens centre is the hotspot.
        let dark = Color32::from_rgb(30, 30, 30);
        let handle = [at + vec2(5.5, 5.5), at + vec2(13.0, 13.0)];
        p.line_segment(handle, Stroke::new(5.0_f32, Color32::WHITE));
        p.circle(at, 7.0, Color32::from_rgba_unmultiplied(225, 238, 255, 200), Stroke::new(3.5_f32, Color32::WHITE));
        p.line_segment(handle, Stroke::new(3.0_f32, dark));
        p.circle_stroke(at, 7.0, Stroke::new(1.6_f32, dark));
        p.line_segment([at - vec2(3.5, 0.0), at + vec2(3.5, 0.0)], Stroke::new(1.4_f32, dark));
        p.line_segment([at - vec2(0.0, 3.5), at + vec2(0.0, 3.5)], Stroke::new(1.4_f32, dark));
    }

    fn sync_textures(&mut self, ctx: &egui::Context) {
        if self.doc.full_upload || self.tex.is_none() {
            self.tex = Some(ctx.load_texture("canvas", self.doc.img.to_color_image(), TextureOptions::NEAREST));
            self.doc.full_upload = false;
            self.doc.dirty = IRect::EMPTY;
        } else if !self.doc.dirty.is_empty() {
            let r = self.doc.dirty;
            if let Some(t) = self.tex.as_mut() {
                t.set_partial([r.x0 as usize, r.y0 as usize], self.doc.img.region_color_image(r), TextureOptions::NEAREST);
            }
            self.doc.dirty = IRect::EMPTY;
        }
        match self.sel.as_ref().and_then(|s| s.float.as_ref().map(|f| (f, s.version))) {
            Some((f, version)) => {
                let key = (version, self.opts.transparent_sel, self.secondary);
                if self.sel_tex.as_ref().map(|(_, k)| *k) != Some(key) {
                    let img = f.keyed_color_image(self.opts.transparent_sel.then_some(self.secondary));
                    self.sel_tex = Some((ctx.load_texture("selection", img, TextureOptions::NEAREST), key));
                }
            }
            None => self.sel_tex = None,
        }
        // Hidden "used to be" pixels, shown faded only while a resize handle is dragged.
        if matches!(self.op, Op::ResizeCanvas { .. }) {
            if self.hidden_tex.is_none() {
                if let Some(h) = self.doc.hidden() {
                    let img = h.ghost_image((self.doc.img.w, self.doc.img.h));
                    self.hidden_tex = Some(ctx.load_texture("hidden", img, TextureOptions::NEAREST));
                }
            }
        } else {
            self.hidden_tex = None;
        }
    }

    // ---- painting the canvas ----------------------------------------------------------

    fn marquee(p: &egui::Painter, r: Rect) {
        let r = r.shrink(0.5);
        let pts = [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()];
        p.add(egui::Shape::line(pts.to_vec(), Stroke::new(1.0_f32, Color32::WHITE)));
        p.extend(egui::Shape::dashed_line(&pts, Stroke::new(1.0_f32, Color32::from_rgb(20, 20, 20)), 4.0, 4.0));
    }

    fn paint_canvas(&self, ui: &Ui, clip: Rect) {
        let p = ui.painter();
        let img_rect = self.irect_to_screen(self.doc.img.bounds());
        p.rect_filled(img_rect.translate(vec2(2.0, 2.0)), 0.0, Color32::from_black_alpha(45));

        // Resize preview, part 1 (under the picture): what the new area will contain.
        let resizing = match &self.op {
            Op::ResizeCanvas { size, .. } => Some(self.irect_to_screen(IRect::from_size(0, 0, size.0, size.1))),
            _ => None,
        };
        if let Some(new_rect) = resizing {
            let pc = p.with_clip_rect(new_rect.intersect(clip));
            let s = self.secondary;
            pc.rect_filled(new_rect, 0.0, Color32::from_rgba_unmultiplied(s.r(), s.g(), s.b(), 90));
            if let (Some(t), Some(h)) = (&self.hidden_tex, self.doc.hidden()) {
                let r = self.irect_to_screen(h.img.bounds());
                pc.image(t.id(), r, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::from_white_alpha(150));
            }
        }

        if let Some(t) = &self.tex {
            p.image(t.id(), img_rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
        }

        // Resize preview, part 2: dim the part of the picture that will be hidden.
        if let Some(new_rect) = resizing {
            let dim = Color32::from_black_alpha(70);
            let right = Rect::from_min_max(pos2(new_rect.max.x.max(img_rect.min.x), img_rect.min.y), img_rect.max);
            let bottom = Rect::from_min_max(pos2(img_rect.min.x, new_rect.max.y.max(img_rect.min.y)), pos2(new_rect.max.x.min(img_rect.max.x), img_rect.max.y));
            for r in [right, bottom] {
                if r.is_positive() {
                    p.rect_filled(r, 0.0, dim);
                }
            }
        }

        // Pixel grid when zoomed in far enough.
        if self.opts.grid && self.scale * self.ppp >= 6.0 {
            let vis = img_rect.intersect(clip);
            if vis.is_positive() {
                let s = self.scale;
                let stroke = Stroke::new(1.0 / self.ppp, Color32::from_rgba_unmultiplied(128, 128, 128, 110));
                let x0 = ((vis.min.x - self.origin.x) / s).floor() as i32;
                let x1 = ((vis.max.x - self.origin.x) / s).ceil() as i32;
                for x in x0..=x1 {
                    let sx = self.origin.x + x as f32 * s;
                    p.line_segment([pos2(sx, vis.min.y), pos2(sx, vis.max.y)], stroke);
                }
                let y0 = ((vis.min.y - self.origin.y) / s).floor() as i32;
                let y1 = ((vis.max.y - self.origin.y) / s).ceil() as i32;
                for y in y0..=y1 {
                    let sy = self.origin.y + y as f32 * s;
                    p.line_segment([pos2(vis.min.x, sy), pos2(vis.max.x, sy)], stroke);
                }
            }
        }

        if let Some(s) = &self.sel {
            let r = self.irect_to_screen(s.rect);
            if let (Some(_), Some((t, _))) = (&s.float, &self.sel_tex) {
                p.image(t.id(), r, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
            }
            Self::marquee(p, r);
        }
        match &self.op {
            Op::Selecting { start, end } => Self::marquee(p, self.irect_to_screen(IRect::from_corners(*start, *end))),
            Op::Text { bbox, caret, .. } => {
                Self::marquee(p, self.irect_to_screen(bbox.union(IRect::new(caret.0, caret.1, caret.0 + 1, caret.2))).expand(2.0));
                let s = self.scale;
                let x = self.origin.x + caret.0 as f32 * s;
                let (t, b) = (self.origin.y + caret.1 as f32 * s, self.origin.y + caret.2 as f32 * s);
                p.line_segment([pos2(x, t), pos2(x, b)], Stroke::new(2.0_f32, Color32::WHITE));
                p.line_segment([pos2(x, t), pos2(x, b)], Stroke::new(1.0_f32, Color32::BLACK));
            }
            Op::ResizeCanvas { size, .. } => {
                Self::marquee(p, self.irect_to_screen(IRect::from_size(0, 0, size.0, size.1)));
            }
            _ => {}
        }

        // Brush / eraser footprint outline.
        if let (Some(h), Op::None | Op::Freehand { .. }) = (self.hover, &self.op) {
            let m = match self.tool {
                Tool::Eraser => Some(Mask::new(BrushShape::Square, self.opts.eraser_size)),
                Tool::Brush if self.opts.brush_size > 2 => Some(Mask::new(self.opts.brush_shape, self.opts.brush_size)),
                _ => None,
            };
            if let Some(m) = m {
                let r = self.irect_to_screen(m.bounds_at(h.0, h.1));
                p.rect_stroke(r, 0.0, Stroke::new(1.0_f32, Color32::from_gray(90)), StrokeKind::Outside);
            }
        }

        for (_, r) in self.handle_rects() {
            p.rect_filled(r, 1.0, Color32::WHITE);
            p.rect_stroke(r, 1.0, Stroke::new(1.0_f32, Color32::from_rgb(60, 90, 160)), StrokeKind::Inside);
        }

        if let Some(at) = self.magnifier {
            self.paint_magnifier(p, at);
        }
    }

    fn canvas_area(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        self.ppp = ctx.pixels_per_point();
        self.scale = self.zoom / self.ppp;
        let (iw, ih) = (self.doc.img.w as f32, self.doc.img.h as f32);
        let mut sa = egui::ScrollArea::both().auto_shrink([false, false]).drag_to_scroll(false);
        if let Some(off) = self.pending_scroll.take() {
            sa = sa.scroll_offset(off);
        }
        let out = sa.show(ui, |ui| {
            let extra = MARGIN * 2.0 + HANDLE * 2.0;
            let content = vec2(iw * self.scale + extra, ih * self.scale + extra);
            let size = content.max(ui.available_size());
            let (rect, resp) = ui.allocate_exact_size(size, Sense::click_and_drag());
            self.origin = snap(rect.min + vec2(MARGIN, MARGIN), self.ppp);
            let clip = ui.clip_rect();
            self.canvas_input(&ctx, &resp, clip);
            self.sync_textures(&ctx);
            self.paint_canvas(ui, clip);
        });
        self.scroll_offset = out.state.offset;
        self.viewport = out.inner_rect;
    }

    // ---- panels -----------------------------------------------------------------------

    fn menu_bar(&mut self, ctx: &egui::Context) {
        fn item(ui: &mut Ui, label: &str, shortcut: &str, enabled: bool) -> bool {
            let r = ui.add_enabled(enabled, egui::Button::new(label).shortcut_text(shortcut)).clicked();
            if r {
                ui.close_menu();
            }
            r
        }
        egui::TopBottomPanel::top("menu").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if item(ui, "New", "Ctrl+N", true) {
                        self.request(Pending::New);
                    }
                    if item(ui, "Open...", "Ctrl+O", true) {
                        self.request(Pending::Open(None));
                    }
                    if item(ui, "Save", "Ctrl+S", true) {
                        self.save();
                    }
                    if item(ui, "Save as...", "Ctrl+Shift+S", true) {
                        self.save_as();
                    }
                    ui.separator();
                    if item(ui, "Exit", "", true) {
                        self.request(Pending::Exit);
                    }
                });
                ui.menu_button("Edit", |ui| {
                    let has_sel = self.sel.is_some();
                    if item(ui, "Undo", "Ctrl+Z", self.doc.can_undo() || !matches!(self.op, Op::None)) {
                        self.undo();
                    }
                    if item(ui, "Redo", "Ctrl+Y", self.doc.can_redo()) {
                        self.redo();
                    }
                    ui.separator();
                    if item(ui, "Cut", "Ctrl+X", has_sel) {
                        self.cut();
                    }
                    if item(ui, "Copy", "Ctrl+C", has_sel) {
                        self.copy();
                    }
                    if item(ui, "Paste", "Ctrl+V", true) {
                        self.paste();
                    }
                    if item(ui, "Delete selection", "Del", has_sel) {
                        self.delete_selection();
                    }
                    ui.separator();
                    if item(ui, "Select all", "Ctrl+A", true) {
                        self.select_all();
                    }
                });
                ui.menu_button("View", |ui| {
                    if item(ui, "Zoom in", "Ctrl++", true) {
                        self.zoom_step(1, None);
                    }
                    if item(ui, "Zoom out", "Ctrl+-", true) {
                        self.zoom_step(-1, None);
                    }
                    if item(ui, "Actual size", "Ctrl+0", true) {
                        self.set_zoom(1.0, None);
                    }
                    ui.separator();
                    if ui.checkbox(&mut self.opts.grid, "Pixel grid (Ctrl+G)").clicked() {
                        ui.close_menu();
                    }
                });
                ui.menu_button("Image", |ui| {
                    let target = if self.sel.is_some() { " selection" } else { "" };
                    if item(ui, &format!("Flip{target} horizontal"), "", true) {
                        self.transform(true, Image::flip_h);
                    }
                    if item(ui, &format!("Flip{target} vertical"), "", true) {
                        self.transform(true, Image::flip_v);
                    }
                    if item(ui, "Rotate right 90°", "", true) {
                        self.transform(true, Image::rotate_cw);
                    }
                    if item(ui, "Rotate left 90°", "", true) {
                        self.transform(true, Image::rotate_ccw);
                    }
                    if item(ui, "Rotate 180°", "", true) {
                        self.transform(true, Image::rotate_180);
                    }
                    ui.separator();
                    if item(ui, "Invert colors", "Ctrl+I", true) {
                        self.transform(false, Image::invert);
                    }
                    if item(ui, "Stretch...", "Ctrl+W", true) {
                        self.dialog = Some(Dialog::Stretch { h: 100, v: 100 });
                    }
                    if item(ui, "Canvas size...", "Ctrl+E", true) {
                        self.dialog = Some(Dialog::CanvasSize { w: self.doc.img.w as u32, h: self.doc.img.h as u32 });
                    }
                    if item(ui, "Crop to selection", "Ctrl+Shift+X", self.sel.is_some()) {
                        self.crop_to_selection();
                    }
                    ui.separator();
                    if item(ui, "Clear image", "Ctrl+Shift+N", true) {
                        self.clear_image();
                    }
                });
                ui.menu_button("Help", |ui| {
                    if item(ui, "About Paintpot", "", true) {
                        self.dialog = Some(Dialog::About);
                    }
                });
            });
        });
    }

    /// Returns the width the panel actually took (it grows if its content is too wide).
    fn tool_panel(&mut self, ctx: &egui::Context) -> f32 {
        egui::SidePanel::left("tools").resizable(false).exact_width(TOOL_PANEL_WIDTH).show(ctx, |ui| {
            ui.add_space(6.0);
            egui::Grid::new("toolgrid").spacing([4.0, 4.0]).show(ui, |ui| {
                for (i, t) in Tool::ALL.iter().copied().enumerate() {
                    let img = egui::Image::from_bytes(t.icon_uri(), t.icon_svg()).fit_to_exact_size(vec2(26.0, 26.0));
                    let b = ui
                        .add_sized([44.0, 40.0], egui::Button::image(img).selected(self.tool == t))
                        .on_hover_text(format!("{}  ({:?})\n{}", t.name(), t.key(), t.hint()));
                    if b.clicked() {
                        self.set_tool(t);
                    }
                    if i % 2 == 1 {
                        ui.end_row();
                    }
                }
            });
            ui.add_space(4.0);
            ui.separator();
            // On short screens the options scroll instead of being cut off.
            egui::ScrollArea::vertical().auto_shrink([false, true]).show(ui, |ui| {
                egui::Frame::new().inner_margin(2.0).show(ui, |ui| self.tool_options(ui));
            });
        })
        .response
        .rect
        .width()
    }

    fn tool_options(&mut self, ui: &mut Ui) {
        if self.tool == Tool::Text {
            self.font_key(); // picks a default family the first time
        }
        let o = &mut self.opts;
        match self.tool {
            Tool::Brush => {
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut o.brush_shape, BrushShape::Round, "●").on_hover_text("Round brush");
                    ui.selectable_value(&mut o.brush_shape, BrushShape::Square, "■").on_hover_text("Square brush");
                });
                size_picker(ui, &mut o.brush_size, &[1, 2, 3, 5, 8, 12, 18, 26], o.brush_shape == BrushShape::Square);
            }
            Tool::Eraser => size_picker(ui, &mut o.eraser_size, &[4, 6, 8, 10, 16, 24, 32, 48], true),
            Tool::Airbrush => size_picker(ui, &mut o.spray_radius, &[4, 8, 12, 16, 24, 32], false),
            Tool::Fill => {
                let tip = "0 = exact color only (classic).\nHigher values also fill similar colors,\nhandy for photos and JPEGs.";
                ui.label("Tolerance").on_hover_text(tip);
                ui.spacing_mut().slider_width = ui.available_width();
                ui.add(egui::Slider::new(&mut o.tolerance, 0..=128).show_value(false)).on_hover_text(tip);
                ui.add(egui::DragValue::new(&mut o.tolerance).range(0..=128)).on_hover_text(tip);
                ui.add_space(4.0);
                ui.radio_value(&mut o.fill_global, false, "Area").on_hover_text("Fill the connected area you click");
                ui.radio_value(&mut o.fill_global, true, "Everywhere").on_hover_text("Replace that color in the whole picture");
            }
            Tool::Line | Tool::Curve => line_width_picker(ui, &mut o.line_width),
            Tool::Rect | Tool::Ellipse | Tool::RoundRect | Tool::Polygon => {
                fill_style_picker(ui, &mut o.fill_style);
                ui.add_space(4.0);
                line_width_picker(ui, &mut o.line_width);
                if self.tool != Tool::Polygon {
                    ui.add_space(4.0);
                    ui.checkbox(&mut o.from_center, "From center").on_hover_text(
                        "Click the centre, drag out the radius.\nShift: perfect circle / square.\nHold Alt to switch mode while dragging.",
                    );
                }
            }
            Tool::Select => {
                ui.checkbox(&mut o.transparent_sel, "Transparent")
                    .on_hover_text("When moving a selection, pixels of the secondary color are see-through");
            }
            Tool::Zoom => {
                let current = self.zoom;
                for z in [1.0, 2.0, 4.0, 6.0, 8.0, 16.0] {
                    if ui.selectable_label(current == z, format!("{z}x")).clicked() {
                        self.set_zoom(z, None);
                    }
                }
            }
            Tool::Text => {
                ui.label("Font");
                let fonts = &mut self.fonts;
                egui::ComboBox::from_id_salt("font")
                    .width(ui.available_width() - 8.0)
                    .wrap_mode(egui::TextWrapMode::Truncate)
                    .selected_text(o.text_family.as_str())
                    .show_ui(ui, |ui| {
                        for f in fonts.families() {
                            ui.selectable_value(&mut o.text_family, f.clone(), f.as_str());
                        }
                    });
                if fonts.families().is_empty() {
                    ui.label("No fonts found");
                }
                ui.label("Size");
                ui.add(egui::DragValue::new(&mut o.text_size).range(6..=400).suffix(" px"));
                ui.horizontal(|ui| {
                    ui.toggle_value(&mut o.text_bold, egui::RichText::new("B").strong()).on_hover_text("Bold");
                    ui.toggle_value(&mut o.text_italic, egui::RichText::new("I").italics()).on_hover_text("Italic");
                });
                ui.checkbox(&mut o.text_opaque, "Opaque")
                    .on_hover_text("Fill the text box with the secondary color");
                ui.checkbox(&mut o.text_smooth, "Smooth")
                    .on_hover_text("Smooth (antialiased) letter edges.
Turn off for hard pixel edges that the
bucket can fill cleanly.");
            }
            Tool::Pencil | Tool::Picker => {}
        }
    }

    fn color_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("colors").show(ctx, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                // Primary / secondary swatches, overlapping like the classic colour box.
                let (rect, resp) = ui.allocate_exact_size(vec2(48.0, 44.0), Sense::click());
                let sec_r = Rect::from_min_size(rect.min + vec2(18.0, 16.0), vec2(26.0, 26.0));
                let pri_r = Rect::from_min_size(rect.min + vec2(2.0, 2.0), vec2(26.0, 26.0));
                let p = ui.painter();
                let border = Stroke::new(1.0_f32, Color32::from_gray(80));
                p.rect_filled(sec_r, 2.0, self.secondary);
                p.rect_stroke(sec_r, 2.0, border, StrokeKind::Inside);
                p.rect_filled(pri_r, 2.0, self.primary);
                p.rect_stroke(pri_r, 2.0, border, StrokeKind::Inside);
                let resp = resp.on_hover_text("Primary (left button) and secondary (right button) colors.\nClick one to edit it.");
                if resp.clicked() {
                    if let Some(pos) = resp.interact_pointer_pos() {
                        let (target, color) = if pri_r.contains(pos) {
                            (ColorTarget::Primary, self.primary)
                        } else {
                            (ColorTarget::Secondary, self.secondary)
                        };
                        self.dialog = Some(Dialog::EditColor { target, color });
                    }
                }
                let swap_icon = egui::Image::from_bytes("bytes://swap.svg", crate::tools::SWAP.as_bytes())
                    .fit_to_exact_size(vec2(16.0, 16.0));
                if ui.add(egui::Button::image(swap_icon)).on_hover_text("Swap colors (X)").clicked() {
                    std::mem::swap(&mut self.primary, &mut self.secondary);
                }
                ui.add_space(6.0);

                let half = self.palettes[self.palette_idx].colors.len() / 2;
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing = vec2(3.0, 3.0);
                    for row in 0..2 {
                        ui.horizontal(|ui| {
                            for i in row * half..(row + 1) * half {
                                let c = self.palettes[self.palette_idx].colors[i];
                                let (r, resp) = ui.allocate_exact_size(vec2(20.0, 20.0), Sense::click());
                                ui.painter().rect_filled(r, 2.0, c);
                                let hl = if resp.hovered() { Color32::from_rgb(40, 100, 220) } else { Color32::from_gray(110) };
                                ui.painter().rect_stroke(r, 2.0, Stroke::new(1.0_f32, hl), StrokeKind::Inside);
                                if resp.clicked() {
                                    self.primary = c;
                                }
                                if resp.secondary_clicked() {
                                    self.secondary = c;
                                }
                                if resp.double_clicked() {
                                    self.dialog = Some(Dialog::EditColor { target: ColorTarget::Palette(i), color: c });
                                }
                            }
                        });
                    }
                });
                ui.add_space(10.0);
                ui.vertical(|ui| {
                    ui.label("Palette");
                    let names: Vec<&str> = self.palettes.iter().map(|p| p.name).collect();
                    egui::ComboBox::from_id_salt("palette")
                        .selected_text(names[self.palette_idx])
                        .show_ui(ui, |ui| {
                            for (i, name) in names.iter().enumerate() {
                                ui.selectable_value(&mut self.palette_idx, i, *name);
                            }
                        })
                        .response
                        .on_hover_text("Classic: the XP-era Paint colors.
OKLCH: evenly spaced hues at equal brightness,
dark and muted on top, light and saturated below.");
                });
            });
            ui.add_space(2.0);
        });
    }

    fn status_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.horizontal(|ui| {
                let pos = self.hover.map(|(x, y)| format!("{x}, {y} px")).unwrap_or_default();
                ui.add_sized([110.0, 18.0], egui::Label::new(pos));
                ui.separator();
                let info = match &self.op {
                    Op::Shape { start, end, center, .. } => {
                        let (a, b) = shape_corners(*start, *end, *center);
                        let r = IRect::from_corners(a, b);
                        if *center {
                            format!("{} × {} px  (r {}, {})", r.width(), r.height(), r.width() / 2, r.height() / 2)
                        } else {
                            format!("{} × {} px", r.width(), r.height())
                        }
                    }
                    Op::Selecting { start, end } => {
                        let r = IRect::from_corners(*start, *end);
                        format!("{} × {} px", r.width(), r.height())
                    }
                    Op::ResizeCanvas { size, .. } => format!("{} × {} px", size.0, size.1),
                    _ => self.sel.as_ref().map(|s| format!("{} × {} px", s.rect.width(), s.rect.height())).unwrap_or_default(),
                };
                ui.add_sized([110.0, 18.0], egui::Label::new(info));
                ui.separator();
                ui.label(format!("Picture {} × {} px", self.doc.img.w, self.doc.img.h));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("+").clicked() {
                        self.zoom_step(1, None);
                    }
                    ui.label(format!("{}%", (self.zoom * 100.0).round()));
                    if ui.small_button("−").clicked() {
                        self.zoom_step(-1, None);
                    }
                });
            });
        });
    }

    // ---- dialogs ----------------------------------------------------------------------

    fn dialogs(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.dialog.take() else { return };
        let mut close = false;
        let mut post = None;
        let name = self.display_name();
        let resp = egui::Modal::new(egui::Id::new("dialog")).show(ctx, |ui| {
            ui.set_min_width(280.0);
            let max = io::MAX_DIM;
            match &mut d {
                Dialog::CanvasSize { w, h } => {
                    ui.heading("Canvas size");
                    ui.label("New area is filled with the secondary color.");
                    egui::Grid::new("cs").show(ui, |ui| {
                        ui.label("Width");
                        ui.add(egui::DragValue::new(w).range(1..=max).suffix(" px"));
                        ui.end_row();
                        ui.label("Height");
                        ui.add(egui::DragValue::new(h).range(1..=max).suffix(" px"));
                        ui.end_row();
                    });
                    if ok_cancel(ui, &mut close) {
                        post = Some(Post::CanvasSize(*w, *h));
                    }
                }
                Dialog::Stretch { h, v } => {
                    ui.heading("Stretch");
                    egui::Grid::new("st").show(ui, |ui| {
                        ui.label("Horizontal");
                        ui.add(egui::DragValue::new(h).range(1..=1000).suffix(" %"));
                        ui.end_row();
                        ui.label("Vertical");
                        ui.add(egui::DragValue::new(v).range(1..=1000).suffix(" %"));
                        ui.end_row();
                    });
                    if ok_cancel(ui, &mut close) {
                        post = Some(Post::Stretch(*h, *v));
                    }
                }
                Dialog::Unsaved(p) => {
                    ui.heading("Save changes?");
                    ui.label(format!("Do you want to save changes to {name}?"));
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button("Save").clicked() {
                            post = Some(Post::SaveThen(p.clone()));
                            close = true;
                        }
                        if ui.button("Don't save").clicked() {
                            post = Some(Post::Do(p.clone()));
                            close = true;
                        }
                        if ui.button("Cancel").clicked() {
                            close = true;
                        }
                    });
                }
                Dialog::EditColor { target, color } => {
                    ui.heading("Edit color");
                    egui::color_picker::color_picker_color32(ui, color, egui::color_picker::Alpha::Opaque);
                    ui.label(format!("#{:02X}{:02X}{:02X}", color.r(), color.g(), color.b()));
                    if ok_cancel(ui, &mut close) {
                        post = Some(Post::SetColor(*target, *color));
                    }
                }
                Dialog::About => {
                    ui.heading("Paintpot");
                    ui.label(format!("Version {}", env!("CARGO_PKG_VERSION")));
                    ui.label("A simple, pixel-perfect paint program.");
                    if ui.button("OK").clicked() {
                        close = true;
                    }
                }
                Dialog::Error(msg) => {
                    ui.heading("Something went wrong");
                    ui.label(msg.as_str());
                    if ui.button("OK").clicked() {
                        close = true;
                    }
                }
            }
        });
        if resp.should_close() {
            close = true;
        }
        if !close {
            self.dialog = Some(d);
        }
        if let Some(p) = post {
            self.run_post(p);
        }
    }

    fn run_post(&mut self, p: Post) {
        match p {
            Post::CanvasSize(w, h) => {
                self.finish_op();
                self.commit_selection();
                self.doc.resize_canvas(w as usize, h as usize, self.secondary);
                self.doc.commit();
            }
            Post::Stretch(h, v) => {
                self.finish_op();
                self.commit_selection();
                let max = io::MAX_DIM as usize;
                let w = (self.doc.img.w * h as usize / 100).clamp(1, max);
                let hh = (self.doc.img.h * v as usize / 100).clamp(1, max);
                let img = self.doc.img.stretched(w, hh);
                self.doc.discard_hidden();
                self.doc.set_image(img);
                self.doc.commit();
            }
            Post::SaveThen(p) => {
                if self.save() {
                    self.do_pending(p);
                }
            }
            Post::Do(p) => self.do_pending(p),
            Post::SetColor(t, c) => match t {
                ColorTarget::Primary => self.primary = c,
                ColorTarget::Secondary => self.secondary = c,
                ColorTarget::Palette(i) => {
                    self.palettes[self.palette_idx].colors[i] = c;
                    self.primary = c;
                }
            },
        }
    }
}

/// OK / Cancel row. Returns true when OK was pressed.
fn ok_cancel(ui: &mut Ui, close: &mut bool) -> bool {
    let mut ok = false;
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        if ui.button("OK").clicked() || ui.input(|i| i.key_pressed(Key::Enter)) {
            ok = true;
            *close = true;
        }
        if ui.button("Cancel").clicked() {
            *close = true;
        }
    });
    ok
}

/// Clickable size swatches (dots or squares) plus a precise number box.
fn size_picker(ui: &mut Ui, value: &mut u32, sizes: &[u32], square: bool) {
    ui.label("Size");
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(2.0, 2.0);
        for &s in sizes {
            let (rect, resp) = ui.allocate_exact_size(vec2(22.0, 22.0), Sense::click());
            let selected = *value == s;
            let vis = ui.visuals();
            if selected {
                ui.painter().rect_filled(rect, 3.0, vis.selection.bg_fill);
            } else if resp.hovered() {
                ui.painter().rect_filled(rect, 3.0, vis.widgets.hovered.weak_bg_fill);
            }
            let d = (s as f32 * 0.6).clamp(2.0, 18.0);
            let c = if selected { Color32::WHITE } else { Color32::from_gray(40) };
            if square {
                ui.painter().rect_filled(Rect::from_center_size(rect.center(), vec2(d, d)), 0.0, c);
            } else {
                ui.painter().circle_filled(rect.center(), d / 2.0, c);
            }
            if resp.on_hover_text(format!("{s} px")).clicked() {
                *value = s;
            }
        }
    });
    ui.add(egui::DragValue::new(value).range(1..=64).suffix(" px"));
}

fn line_width_picker(ui: &mut Ui, value: &mut u32) {
    let title = format!("Width · {value}");
    collapsible(ui, "line_width", &title, |ui| line_width_rows(ui, value));
}

fn line_width_rows(ui: &mut Ui, value: &mut u32) {
    for w in [1u32, 2, 3, 4, 5, 8, 12] {
        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 16.0), Sense::click());
        let selected = *value == w;
        if selected {
            ui.painter().rect_filled(rect, 3.0, ui.visuals().selection.bg_fill);
        } else if resp.hovered() {
            ui.painter().rect_filled(rect, 3.0, ui.visuals().widgets.hovered.weak_bg_fill);
        }
        let c = if selected { Color32::WHITE } else { Color32::from_gray(40) };
        let r = Rect::from_center_size(rect.center(), vec2(rect.width() - 14.0, (w as f32).min(12.0)));
        ui.painter().rect_filled(r, 0.0, c);
        if resp.on_hover_text(format!("{w} px")).clicked() {
            *value = w;
        }
    }
}

fn fill_style_picker(ui: &mut Ui, value: &mut FillStyle) {
    let name = match value {
        FillStyle::Outline => "line",
        FillStyle::OutlineFill => "both",
        FillStyle::Fill => "solid",
    };
    let title = format!("Style · {name}");
    collapsible(ui, "fill_style", &title, |ui| fill_style_rows(ui, value));
}

/// A section of the tool options that can be folded away to save vertical space. Its open
/// state is remembered by `id`, so it stays folded when switching between tools.
fn collapsible(ui: &mut Ui, id: &str, title: &str, body: impl FnOnce(&mut Ui)) {
    // Unindented body: the panel is narrow, so the rows keep its full width. (Shrinking
    // `spacing.indent` instead would also move the title on top of the arrow.)
    egui::CollapsingHeader::new(title).id_salt(id).default_open(true).show_unindented(ui, body);
}

fn fill_style_rows(ui: &mut Ui, value: &mut FillStyle) {
    for (style, tip) in [
        (FillStyle::Outline, "Outline only"),
        (FillStyle::OutlineFill, "Outline (primary) filled with secondary"),
        (FillStyle::Fill, "Solid, primary color"),
    ] {
        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::click());
        let selected = *value == style;
        if selected {
            ui.painter().rect_filled(rect, 3.0, ui.visuals().selection.bg_fill);
        } else if resp.hovered() {
            ui.painter().rect_filled(rect, 3.0, ui.visuals().widgets.hovered.weak_bg_fill);
        }
        let fg = if selected { Color32::WHITE } else { Color32::from_gray(40) };
        let r = Rect::from_center_size(rect.center(), vec2(40.0, 14.0));
        match style {
            FillStyle::Outline => {
                ui.painter().rect_stroke(r, 0.0, Stroke::new(1.5_f32, fg), StrokeKind::Inside);
            }
            FillStyle::OutlineFill => {
                ui.painter().rect_filled(r, 0.0, Color32::from_gray(160));
                ui.painter().rect_stroke(r, 0.0, Stroke::new(1.5_f32, fg), StrokeKind::Inside);
            }
            FillStyle::Fill => {
                ui.painter().rect_filled(r, 0.0, fg);
            }
        }
        if resp.on_hover_text(tip).clicked() {
            *value = style;
        }
    }
}

impl eframe::App for PaintApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        egui::Rgba::from(CANVAS_BG).to_array()
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Ask before closing with unsaved changes.
        if ctx.input(|i| i.viewport().close_requested()) && !self.allow_close && self.doc.modified {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.dialog = Some(Dialog::Unsaved(Pending::Exit));
        }

        if let Some(path) = ctx.input(|i| i.raw.dropped_files.iter().find_map(|f| f.path.clone())) {
            self.request(Pending::Open(Some(path)));
        }

        if self.dialog.is_none() {
            self.shortcuts(ctx);
        }

        self.menu_bar(ctx);
        self.status_bar(ctx);
        self.color_bar(ctx);
        self.tool_panel(ctx);
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(CANVAS_BG))
            .show(ctx, |ui| self.canvas_area(ui));
        self.dialogs(ctx);

        if self.want_close {
            self.want_close = false;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }

        let title = format!("{}{} - Paintpot", if self.doc.modified { "*" } else { "" }, self.display_name());
        if title != self.title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title = title;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const W: Color32 = Color32::WHITE;
    const K: Color32 = Color32::BLACK;
    const R: Color32 = Color32::RED;
    /// Far away from the canvas resize handles.
    const NOWHERE: Pos2 = pos2(-500.0, -500.0);

    fn app() -> PaintApp {
        PaintApp::create(&egui::Context::default(), None)
    }

    /// Press at `a`, move through `path`, release - like a real mouse drag.
    fn drag(app: &mut PaintApp, a: (i32, i32), path: &[(i32, i32)], time: f64) {
        app.button = Some(PointerButton::Primary);
        app.on_press(a, NOWHERE, false, Modifiers::NONE, time);
        for &p in path {
            app.on_move(p, NOWHERE, Modifiers::NONE);
        }
        app.button = None;
        app.on_release();
    }

    fn click(app: &mut PaintApp, p: (i32, i32), time: f64) {
        drag(app, p, &[], time);
    }

    fn px(app: &PaintApp, x: i32, y: i32) -> Color32 {
        app.doc.img.get(x, y).unwrap()
    }

    #[test]
    fn ellipse_then_fill_stays_inside_and_undoes() {
        let mut a = app();
        a.set_tool(Tool::Ellipse);
        // Wobble through several sizes: the preview must not leave old outlines behind.
        drag(&mut a, (100, 100), &[(600, 500), (150, 130), (400, 300)], 0.0);
        assert_eq!(px(&a, 550, 450), W, "stale preview pixels left behind");
        assert_eq!(px(&a, 100, 200), K, "left edge of the ellipse");

        a.set_tool(Tool::Fill);
        a.primary = R;
        click(&mut a, (250, 200), 1.0);
        assert_eq!(px(&a, 250, 200), R);
        assert_eq!(px(&a, 10, 10), W, "fill leaked outside the ellipse");
        assert_eq!(px(&a, 101, 101), W, "fill leaked into a corner outside the curve");
        assert!(a.doc.modified);

        a.undo();
        assert_eq!(px(&a, 250, 200), W, "undo removes the fill");
        assert_eq!(px(&a, 100, 200), K, "but keeps the ellipse");
        a.undo();
        assert_eq!(px(&a, 100, 200), W);
        a.redo();
        assert_eq!(px(&a, 100, 200), K);
    }

    fn key(key: Key) -> Event {
        Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }
    }

    /// Run one frame of keyboard input through the real shortcut/typing handler.
    fn keys(a: &mut PaintApp, ctx: &egui::Context, events: Vec<Event>) {
        let _ = ctx.run(egui::RawInput { events, ..Default::default() }, |ctx| a.shortcuts(ctx));
    }

    #[test]
    fn text_tool_types_places_and_undoes() {
        let ctx = egui::Context::default();
        let mut a = PaintApp::create(&ctx, None);
        if a.fonts.families().is_empty() {
            return; // no system fonts on this machine
        }
        a.set_tool(Tool::Text);
        a.opts.text_size = 32;
        click(&mut a, (50, 50), 0.0);
        assert!(matches!(a.op, Op::Text { .. }));

        // "Hix" + Backspace + Enter + "ok": letters must not switch tools (I = picker, etc).
        keys(&mut a, &ctx, vec![Event::Text("Hix".into()), key(Key::Backspace), key(Key::Enter), Event::Text("ok".into())]);
        assert_eq!(a.tool, Tool::Text);
        // The app re-renders typed text once per frame (in canvas_input); do that here.
        let mut op = std::mem::take(&mut a.op);
        a.redraw_preview(&mut op);
        a.op = op;
        let Op::Text { text, bbox, .. } = &a.op else { panic!("still typing") };
        assert_eq!(text, "Hi\nok");
        let bbox = *bbox;
        assert!(bbox.height() >= 60, "two lines");
        let ink = |a: &PaintApp| (bbox.y0..bbox.y1).flat_map(|y| (bbox.x0..bbox.x1).map(move |x| (x, y))).filter(|&(x, y)| px(a, x, y) == K).count();
        assert!(ink(&a) > 50, "text is drawn while typing");

        // Clicking elsewhere places it and starts a new, empty text.
        click(&mut a, (400, 400), 1.0);
        assert!(a.doc.can_undo());
        assert!(ink(&a) > 50, "placed text stays");
        // Switching tools with nothing typed leaves no trace (not even an opaque box).
        a.opts.text_opaque = true;
        a.set_tool(Tool::Pencil);
        assert_eq!(px(&a, 401, 401), W);

        a.undo();
        assert_eq!(ink(&a), 0, "undo removes the text");
    }

    fn text_of(a: &PaintApp) -> (String, usize) {
        match &a.op {
            Op::Text { text, cursor, .. } => (text.clone(), *cursor),
            _ => panic!("not typing"),
        }
    }

    /// Re-layout, as the app does every frame (up/down use the laid-out lines).
    fn frame(a: &mut PaintApp) {
        let mut op = std::mem::take(&mut a.op);
        a.redraw_preview(&mut op);
        a.op = op;
    }

    #[test]
    fn text_caret_moves_and_edits_in_place() {
        let ctx = egui::Context::default();
        let mut a = PaintApp::create(&ctx, None);
        if a.fonts.families().is_empty() {
            return;
        }
        a.set_tool(Tool::Text);
        click(&mut a, (50, 50), 0.0);
        keys(&mut a, &ctx, vec![Event::Text("Hello".into())]);
        keys(&mut a, &ctx, vec![key(Key::ArrowLeft), key(Key::ArrowLeft), Event::Text("X".into())]);
        assert_eq!(text_of(&a), ("HelXlo".into(), 4));
        keys(&mut a, &ctx, vec![key(Key::Home), Event::Text("A".into())]);
        assert_eq!(text_of(&a), ("AHelXlo".into(), 1));
        keys(&mut a, &ctx, vec![key(Key::Delete), key(Key::End), key(Key::Backspace)]);
        assert_eq!(text_of(&a), ("AelXl".into(), 5));
        keys(&mut a, &ctx, vec![key(Key::ArrowLeft), key(Key::ArrowLeft), key(Key::Backspace)]);
        assert_eq!(text_of(&a), ("AeXl".into(), 2));

        // Two lines: Up/Down keep the horizontal position.
        keys(&mut a, &ctx, vec![key(Key::End), key(Key::Enter), Event::Text("wxyz".into())]);
        assert_eq!(text_of(&a), ("AeXl\nwxyz".into(), 9));
        frame(&mut a);
        keys(&mut a, &ctx, vec![key(Key::ArrowLeft), key(Key::ArrowLeft)]); // between x and y
        frame(&mut a);
        keys(&mut a, &ctx, vec![key(Key::ArrowUp)]);
        let (_, c) = text_of(&a);
        assert!((1..=3).contains(&c), "Up lands near the same x on line 1 (got {c})");
        frame(&mut a);
        keys(&mut a, &ctx, vec![key(Key::ArrowDown)]);
        assert_eq!(text_of(&a).1, 7, "Down returns to the same spot");
        keys(&mut a, &ctx, vec![key(Key::ArrowUp), key(Key::ArrowUp), key(Key::ArrowDown), key(Key::ArrowDown)]);
        assert_eq!(text_of(&a).0, "AeXl\nwxyz", "moving never edits");

        // Clicking inside the text box moves the caret there (to the very start here).
        frame(&mut a);
        let Op::Text { lines, .. } = &a.op else { unreachable!() };
        let start = (lines[0].xs[0], lines[0].top + 2);
        click(&mut a, start, 5.0);
        assert_eq!(text_of(&a).1, 0);
        assert!(matches!(a.op, Op::Text { .. }), "still typing after clicking inside");
    }

    #[test]
    fn escape_cancels_text() {
        let ctx = egui::Context::default();
        let mut a = PaintApp::create(&ctx, None);
        if a.fonts.families().is_empty() {
            return;
        }
        a.set_tool(Tool::Text);
        click(&mut a, (50, 50), 0.0);
        keys(&mut a, &ctx, vec![Event::Text("Hello".into())]);
        keys(&mut a, &ctx, vec![key(Key::Escape)]);
        assert!(matches!(a.op, Op::None));
        assert!(a.doc.img.px.iter().all(|p| *p == W), "cancelled text leaves nothing");
        assert!(!a.doc.can_undo());
    }

    #[test]
    fn ellipse_from_center_is_symmetric() {
        let mut a = app();
        a.set_tool(Tool::Ellipse);
        a.opts.from_center = true;
        drag(&mut a, (100, 100), &[(110, 105)], 0.0);
        // Box is centre ± radius: x 90..=110, y 95..=105.
        for p in [(90, 100), (110, 100), (100, 95), (100, 105)] {
            assert_eq!(px(&a, p.0, p.1), K, "edge pixel {p:?}");
        }
        assert_eq!(px(&a, 89, 100), W);
        assert_eq!(px(&a, 111, 100), W);
        for dy in -6..=6 {
            for dx in -12..=12 {
                assert_eq!(px(&a, 100 + dx, 100 + dy), px(&a, 100 - dx, 100 - dy), "not symmetric at ({dx},{dy})");
            }
        }

        // Shift = circle with the larger radius; Alt flips back to corner mode.
        let shift = Modifiers { shift: true, ..Modifiers::NONE };
        a.button = Some(PointerButton::Primary);
        a.on_press((300, 300), NOWHERE, false, Modifiers::NONE, 1.0);
        a.on_move((330, 310), NOWHERE, shift);
        a.button = None;
        a.on_release();
        for p in [(270, 300), (330, 300), (300, 270), (300, 330)] {
            assert_eq!(px(&a, p.0, p.1), K, "circle edge {p:?}");
        }

        let alt = Modifiers { alt: true, ..Modifiers::NONE };
        a.button = Some(PointerButton::Primary);
        a.on_press((500, 400), NOWHERE, false, alt, 2.0);
        a.on_move((520, 420), NOWHERE, alt);
        a.button = None;
        a.on_release();
        assert_eq!(px(&a, 500, 410), K, "Alt: corner mode, left edge at the press point");
        assert_eq!(px(&a, 485, 410), W);
    }

    #[test]
    fn freehand_pencil_strokes_are_connected() {
        let mut a = app();
        a.set_tool(Tool::Pencil);
        drag(&mut a, (0, 0), &[(60, 30), (90, 599)], 0.0); // edge to edge
        // A 4-connected fill on one side of the stroke must not reach the other side.
        a.set_tool(Tool::Fill);
        a.primary = R;
        click(&mut a, (700, 10), 1.0);
        assert_eq!(px(&a, 700, 10), R);
        assert_eq!(px(&a, 5, 500), W, "fill crossed the pencil line");
    }

    #[test]
    fn move_selection_clears_source_with_secondary() {
        let mut a = app();
        a.opts.fill_style = FillStyle::Fill;
        a.set_tool(Tool::Rect);
        drag(&mut a, (10, 10), &[(19, 19)], 0.0);
        assert_eq!(px(&a, 15, 15), K);

        a.set_tool(Tool::Select);
        drag(&mut a, (5, 5), &[(25, 25)], 1.0); // select around the square
        drag(&mut a, (15, 15), &[(115, 65)], 2.0); // move it by (100, 50)
        a.escape(); // drop the selection
        assert_eq!(px(&a, 15, 15), W, "source cleared");
        assert_eq!(px(&a, 115, 65), K, "pixels moved");
        assert!(!a.doc.can_redo());
        a.undo();
        assert_eq!(px(&a, 15, 15), K, "one undo restores the whole move");
        assert_eq!(px(&a, 115, 65), W);
    }

    #[test]
    fn polygon_closes_on_double_click() {
        let mut a = app();
        a.set_tool(Tool::Polygon);
        drag(&mut a, (100, 100), &[(300, 100)], 0.0); // first side
        click(&mut a, (300, 300), 1.0);
        click(&mut a, (100, 300), 2.0);
        click(&mut a, (100, 300), 2.1); // double-click finishes
        assert!(matches!(a.op, Op::None));
        assert_eq!(px(&a, 100, 200), K, "closing edge drawn");
        a.set_tool(Tool::Fill);
        a.primary = R;
        click(&mut a, (200, 200), 5.0);
        assert_eq!(px(&a, 200, 200), R);
        assert_eq!(px(&a, 50, 50), W);
    }

    #[test]
    fn curve_takes_three_drags() {
        let mut a = app();
        a.set_tool(Tool::Curve);
        drag(&mut a, (100, 300), &[(500, 300)], 0.0);
        assert!(matches!(a.op, Op::Curve { stage: 1, .. }));
        drag(&mut a, (300, 100), &[(300, 100)], 1.0);
        assert!(matches!(a.op, Op::Curve { stage: 2, .. }));
        drag(&mut a, (300, 100), &[(300, 100)], 2.0);
        assert!(matches!(a.op, Op::None));
        assert!(a.doc.can_undo());
        assert_eq!(px(&a, 300, 300), W, "the curve bent away from the straight line");
    }

    /// Dragging the bottom-right handle resizes on release, with every tool selected.
    #[test]
    fn resize_handle_is_click_and_drag() {
        for tool in Tool::ALL {
            let mut a = app();
            a.set_tool(tool);
            // Test app: origin (0,0), 1 screen point per pixel, picture 800x600.
            let handle = a.handle_rects()[2].1.center();
            a.button = Some(PointerButton::Primary);
            a.on_press(a.to_px(handle), handle, false, Modifiers::NONE, 0.0);
            let target = pos2(900.0, 700.0);
            a.on_move(a.to_px(target), target, Modifiers::NONE);
            // Release exactly the way canvas_input does it.
            if a.button == Some(PointerButton::Primary) {
                a.button = None;
                a.on_release();
            }
            assert_eq!((a.doc.img.w, a.doc.img.h), (900, 700), "{tool:?}: resize did not finish on release");
            assert!(matches!(a.op, Op::None), "{tool:?}: still resizing after release");
        }
    }

    /// Drag the bottom-right handle so the picture becomes `to` pixels in size.
    fn drag_corner(a: &mut PaintApp, to: (i32, i32)) {
        let handle = a.handle_rects()[2].1.center();
        a.button = Some(PointerButton::Primary);
        a.on_press(a.to_px(handle), handle, false, Modifiers::NONE, 0.0);
        let target = a.origin + vec2(to.0 as f32 * a.scale, to.1 as f32 * a.scale);
        a.on_move(a.to_px(target), target, Modifiers::NONE);
        a.button = None;
        a.on_release();
        assert_eq!((a.doc.img.w as i32, a.doc.img.h as i32), to);
    }

    #[test]
    fn resize_handle_keeps_hidden_pixels() {
        let mut a = app();
        a.doc.img.set(700, 500, R);
        a.doc.commit();
        drag_corner(&mut a, (300, 200));
        a.set_tool(Tool::Fill);
        a.primary = Color32::GREEN;
        click(&mut a, (10, 10), 1.0); // fill is bounded to the small picture
        drag_corner(&mut a, (800, 600));
        assert_eq!(px(&a, 700, 500), R, "hidden pixel came back");
        assert_eq!(px(&a, 10, 10), Color32::GREEN, "fill while small is kept");
        assert_eq!(px(&a, 400, 300), W, "fill did not reach hidden pixels");

        // Rotating moves pixels, so the hidden ones are discarded.
        drag_corner(&mut a, (300, 200));
        a.transform(true, Image::rotate_cw);
        a.transform(true, Image::rotate_ccw);
        drag_corner(&mut a, (800, 600));
        assert_eq!(px(&a, 700, 500), a.secondary, "gone after rotating");
    }

    #[test]
    fn palettes_switch_and_keep_their_own_edits() {
        let mut a = app();
        assert_eq!(a.palettes[a.palette_idx].name, "Classic");
        a.run_post(Post::SetColor(ColorTarget::Palette(3), Color32::from_rgb(1, 2, 3)));
        assert_eq!(a.primary, Color32::from_rgb(1, 2, 3), "editing a swatch also picks it");
        a.palette_idx = 1;
        assert_eq!(a.palettes[1].name, "OKLCH");
        assert_ne!(a.palettes[1].colors[3], Color32::from_rgb(1, 2, 3), "other preset untouched");
        a.palette_idx = 0;
        assert_eq!(a.palettes[0].colors[3], Color32::from_rgb(1, 2, 3), "edit kept when switching back");
    }

    #[test]
    fn canvas_resize_and_transform() {
        let mut a = app();
        a.doc.img.set(0, 0, R);
        a.doc.commit();
        a.transform(true, Image::rotate_cw);
        assert_eq!((a.doc.img.w, a.doc.img.h), (600, 800));
        assert_eq!(px(&a, 599, 0), R);
        a.run_post(Post::CanvasSize(50, 40));
        assert_eq!((a.doc.img.w, a.doc.img.h), (50, 40));
        a.undo();
        a.undo();
        assert_eq!((a.doc.img.w, a.doc.img.h), (800, 600));
        assert_eq!(px(&a, 0, 0), R);
    }
}

#[cfg(test)]
mod ui_tests {
    use super::*;

    /// Lay out the toolbox for every tool and check its options never widen the panel
    /// (a wider panel pushes into the canvas area and shows up as a grey strip).
    #[test]
    fn tool_options_fit_the_panel() {
        let ctx = egui::Context::default();
        let mut a = PaintApp::create(&ctx, None);
        a.opts.line_width = 12; // widest section titles: "Width · 12", "Style · solid"
        a.opts.fill_style = FillStyle::Fill;
        for t in Tool::ALL {
            a.tool = t;
            let mut width = 0.0;
            for _ in 0..3 {
                let input = egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1100.0, 760.0))),
                    ..Default::default()
                };
                let _ = ctx.run(input, |ctx| width = a.tool_panel(ctx));
            }
            // The panel adds its own 4px frame margin around exact_width; nothing more.
            assert!(width <= TOOL_PANEL_WIDTH + 4.5, "{t:?} options widen the tool panel to {width}");
        }
    }
}
