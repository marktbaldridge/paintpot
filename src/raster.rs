//! Aliased raster primitives. Every primitive writes whole pixels of exactly one colour,
//! and every closed outline is guaranteed to be "4-tight": a 4-connected flood fill
//! started inside can never leak out. That property is what makes the bucket tool
//! reliable, and the tests at the bottom of this file check it for many sizes.

use crate::canvas::{IRect, Image};
use eframe::egui::Color32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BrushShape {
    Round,
    Square,
}

/// A brush footprint: the pixel offsets painted around each point of a stroke.
pub struct Mask {
    pub offs: Vec<(i32, i32)>,
    pub lo: i32,
    pub hi: i32,
}

impl Mask {
    pub fn new(shape: BrushShape, size: u32) -> Mask {
        let s = size.clamp(1, 256) as i32;
        let lo = -(s / 2);
        let hi = lo + s - 1;
        let c = (lo + hi) as f32 / 2.0;
        let r = s as f32 / 2.0;
        let r2 = r * r - 0.25;
        let mut offs = Vec::with_capacity((s * s) as usize);
        for dy in lo..=hi {
            for dx in lo..=hi {
                let keep = match shape {
                    BrushShape::Square => true,
                    BrushShape::Round => {
                        let (fx, fy) = (dx as f32 - c, dy as f32 - c);
                        s <= 2 || fx * fx + fy * fy < r2
                    }
                };
                if keep {
                    offs.push((dx, dy));
                }
            }
        }
        Mask { offs, lo, hi }
    }

    pub fn bounds_at(&self, x: i32, y: i32) -> IRect {
        IRect::new(x + self.lo, y + self.lo, x + self.hi + 1, y + self.hi + 1)
    }
}

/// Bresenham line: visits an 8-connected run of pixels from `a` to `b`, both included.
pub fn line_points(a: (i32, i32), b: (i32, i32), mut f: impl FnMut(i32, i32)) {
    let (mut x0, mut y0) = a;
    let (x1, y1) = b;
    let dx = (x1 - x0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let dy = -(y1 - y0).abs();
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut err = dx + dy;
    loop {
        f(x0, y0);
        if x0 == x1 && y0 == y1 {
            break;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x0 += sx;
        }
        if e2 <= dx {
            err += dx;
            y0 += sy;
        }
    }
}

/// Stamp `m` along the line `a`-`b`.
pub fn stroke(img: &mut Image, a: (i32, i32), b: (i32, i32), m: &Mask, c: Color32) -> IRect {
    if m.offs.len() == 1 {
        let (ox, oy) = m.offs[0];
        line_points(a, b, |x, y| img.set(x + ox, y + oy, c));
    } else {
        line_points(a, b, |x, y| {
            for &(dx, dy) in &m.offs {
                img.set(x + dx, y + dy, c);
            }
        });
    }
    m.bounds_at(a.0, a.1).union(m.bounds_at(b.0, b.1)).intersect(img.bounds())
}

/// Like [`stroke`] but only replaces pixels that are exactly `from` (the colour eraser).
pub fn stroke_replace(img: &mut Image, a: (i32, i32), b: (i32, i32), m: &Mask, from: Color32, to: Color32) -> IRect {
    line_points(a, b, |x, y| {
        for &(dx, dy) in &m.offs {
            if img.get(x + dx, y + dy) == Some(from) {
                img.set(x + dx, y + dy, to);
            }
        }
    });
    m.bounds_at(a.0, a.1).union(m.bounds_at(b.0, b.1)).intersect(img.bounds())
}

pub fn stroke_polyline(img: &mut Image, pts: &[(i32, i32)], m: &Mask, c: Color32, closed: bool) -> IRect {
    let mut r = IRect::EMPTY;
    match pts.len() {
        0 => {}
        1 => r = stroke(img, pts[0], pts[0], m, c),
        _ => {
            for w in pts.windows(2) {
                r = r.union(stroke(img, w[0], w[1], m, c));
            }
            if closed {
                r = r.union(stroke(img, pts[pts.len() - 1], pts[0], m, c));
            }
        }
    }
    r
}

// ---- span shapes (rectangle, ellipse, rounded rectangle) ------------------------------

/// A shape described as one horizontal run per row: `rows[i] = (x_start, x_end_exclusive)`.
pub struct Spans {
    y0: i32,
    rows: Vec<(i32, i32)>,
}

impl Spans {
    /// `f(row_centre_y)` returns the shape's left/right edge (continuous coordinates) on
    /// that row. A pixel is inside when its centre lies between the edges. Rows that would
    /// round to nothing keep their centre pixel(s), so outlines never have gaps.
    fn from_edges(r: IRect, f: impl Fn(f64) -> Option<(f64, f64)>) -> Spans {
        let mut rows = Vec::with_capacity(r.height() as usize);
        for y in r.y0..r.y1 {
            let span = match f(y as f64 + 0.5) {
                Some((l, rr)) => {
                    let xl = (l - 0.5).ceil() as i32;
                    let xr = (rr - 0.5).floor() as i32 + 1;
                    if xr > xl {
                        (xl, xr)
                    } else {
                        let m = (l + rr) / 2.0 - 0.5;
                        (m.floor() as i32, m.ceil() as i32 + 1)
                    }
                }
                None => (0, 0),
            };
            rows.push(span);
        }
        Spans { y0: r.y0, rows }
    }

    pub fn rect(r: IRect) -> Spans {
        Self::from_edges(r, |_| Some((r.x0 as f64, r.x1 as f64)))
    }

    pub fn ellipse(r: IRect) -> Spans {
        if r.is_empty() {
            return Spans { y0: 0, rows: Vec::new() };
        }
        let a = r.width() as f64 / 2.0;
        let b = r.height() as f64 / 2.0;
        let (cx, cy) = (r.x0 as f64 + a, r.y0 as f64 + b);
        Self::from_edges(r, move |yc| {
            let d = (yc - cy) / b;
            let t = 1.0 - d * d;
            if t < 0.0 {
                None
            } else {
                let hw = a * t.sqrt();
                Some((cx - hw, cx + hw))
            }
        })
    }

    pub fn round_rect(r: IRect, radius: f64) -> Spans {
        let rad = radius.max(0.0).min(r.width() as f64 / 2.0).min(r.height() as f64 / 2.0);
        let (x0, x1) = (r.x0 as f64, r.x1 as f64);
        let (top, bot) = (r.y0 as f64 + rad, r.y1 as f64 - rad);
        Self::from_edges(r, move |yc| {
            let dy = if yc < top {
                top - yc
            } else if yc > bot {
                yc - bot
            } else {
                0.0
            };
            let inset = if dy > 0.0 { rad - (rad * rad - dy * dy).max(0.0).sqrt() } else { 0.0 };
            Some((x0 + inset, x1 - inset))
        })
    }

    #[inline]
    fn row(&self, y: i32) -> (i32, i32) {
        let i = y - self.y0;
        if i < 0 || i as usize >= self.rows.len() {
            (0, 0)
        } else {
            self.rows[i as usize]
        }
    }
    #[inline]
    pub fn contains(&self, x: i32, y: i32) -> bool {
        let (a, b) = self.row(y);
        x >= a && x < b
    }
    /// Inside pixel with at least one 4-neighbour outside.
    #[inline]
    fn is_boundary(&self, x: i32, y: i32) -> bool {
        !self.contains(x - 1, y) || !self.contains(x + 1, y) || !self.contains(x, y - 1) || !self.contains(x, y + 1)
    }
    pub fn bbox(&self) -> IRect {
        let mut r = IRect::EMPTY;
        for (i, &(a, b)) in self.rows.iter().enumerate() {
            if b > a {
                let y = self.y0 + i as i32;
                r = r.union(IRect::new(a, y, b, y + 1));
            }
        }
        r
    }
}

/// Draw a span shape. The outline is every pixel of `outer` that touches the outside
/// (always closed and 4-tight) plus, for thick outlines, everything not inside `inner`.
pub fn draw_spans(img: &mut Image, outer: &Spans, inner: Option<&Spans>, outline: Option<Color32>, fill: Option<Color32>) -> IRect {
    let (w, h) = (img.w as i32, img.h as i32);
    for (i, &(a, b)) in outer.rows.iter().enumerate() {
        let y = outer.y0 + i as i32;
        if y < 0 || y >= h {
            continue;
        }
        for x in a.max(0)..b.min(w) {
            let ring = outer.is_boundary(x, y) || inner.is_some_and(|s| !s.contains(x, y));
            let c = if ring { outline.or(fill) } else { fill };
            if let Some(c) = c {
                img.px[y as usize * img.w + x as usize] = c;
            }
        }
    }
    outer.bbox().intersect(img.bounds())
}

/// Even-odd scanline polygon fill; pixel centres inside the polygon are painted.
pub fn fill_polygon(img: &mut Image, pts: &[(i32, i32)], c: Color32) -> IRect {
    if pts.len() < 3 {
        return IRect::EMPTY;
    }
    let miny = pts.iter().map(|p| p.1).min().unwrap().max(0);
    let maxy = pts.iter().map(|p| p.1).max().unwrap().min(img.h as i32 - 1);
    let n = pts.len();
    let mut xs: Vec<f64> = Vec::new();
    let mut bb = IRect::EMPTY;
    for y in miny..=maxy {
        let yc = y as f64 + 0.5;
        xs.clear();
        for i in 0..n {
            let (x0, y0) = pts[i];
            let (x1, y1) = pts[(i + 1) % n];
            let (fy0, fy1) = (y0 as f64 + 0.5, y1 as f64 + 0.5);
            if (fy0 <= yc && fy1 > yc) || (fy1 <= yc && fy0 > yc) {
                let t = (yc - fy0) / (fy1 - fy0);
                xs.push(x0 as f64 + 0.5 + t * (x1 - x0) as f64);
            }
        }
        xs.sort_by(|a, b| a.total_cmp(b));
        for pair in xs.chunks_exact(2) {
            let xl = (pair[0] - 0.5).ceil() as i32;
            let xr = (pair[1] - 0.5).floor() as i32 + 1;
            if xr > xl {
                img.hline(y, xl, xr, c);
                bb = bb.union(IRect::new(xl, y, xr, y + 1));
            }
        }
    }
    bb.intersect(img.bounds())
}

/// Points of a cubic Bezier, rounded to pixels (consecutive duplicates removed).
pub fn bezier_points(p: [(i32, i32); 4]) -> Vec<(i32, i32)> {
    let f = |i: usize| (p[i].0 as f64, p[i].1 as f64);
    let d = |a: (f64, f64), b: (f64, f64)| ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt();
    let (p0, p1, p2, p3) = (f(0), f(1), f(2), f(3));
    let len = d(p0, p1) + d(p1, p2) + d(p2, p3);
    let n = (len / 2.0).ceil().clamp(4.0, 4000.0) as usize;
    let mut out: Vec<(i32, i32)> = Vec::with_capacity(n + 1);
    for k in 0..=n {
        let t = k as f64 / n as f64;
        let mt = 1.0 - t;
        let (a, b, c, e) = (mt * mt * mt, 3.0 * mt * mt * t, 3.0 * mt * t * t, t * t * t);
        let x = a * p0.0 + b * p1.0 + c * p2.0 + e * p3.0;
        let y = a * p0.1 + b * p1.1 + c * p2.1 + e * p3.1;
        let q = (x.round() as i32, y.round() as i32);
        if out.last() != Some(&q) {
            out.push(q);
        }
    }
    out
}

// ---- fills -----------------------------------------------------------------------------

#[inline]
pub fn close(a: Color32, b: Color32, tol: u8) -> bool {
    a.r().abs_diff(b.r()) <= tol && a.g().abs_diff(b.g()) <= tol && a.b().abs_diff(b.b()) <= tol
}

/// Scanline flood fill, 4-connected (diagonal gaps do NOT leak, exactly like MS Paint).
/// `tol` = maximum per-channel difference from the clicked colour; 0 = exact match.
pub fn flood_fill(img: &mut Image, x: i32, y: i32, c: Color32, tol: u8) -> IRect {
    let Some(target) = img.get(x, y) else { return IRect::EMPTY };
    if tol == 0 && target == c {
        return IRect::EMPTY;
    }
    let (w, h) = (img.w as i32, img.h as i32);
    let mut done = vec![false; img.w * img.h];
    let matches = |img: &Image, done: &[bool], x: i32, y: i32| {
        let i = y as usize * img.w + x as usize;
        !done[i] && close(img.px[i], target, tol)
    };
    let mut stack = vec![(x, y)];
    let mut bb = IRect::EMPTY;
    while let Some((sx, sy)) = stack.pop() {
        if !matches(img, &done, sx, sy) {
            continue;
        }
        let mut l = sx;
        while l > 0 && matches(img, &done, l - 1, sy) {
            l -= 1;
        }
        let mut r = sx;
        while r + 1 < w && matches(img, &done, r + 1, sy) {
            r += 1;
        }
        let row = sy as usize * img.w;
        img.px[row + l as usize..=row + r as usize].fill(c);
        done[row + l as usize..=row + r as usize].fill(true);
        bb = bb.union(IRect::new(l, sy, r + 1, sy + 1));
        for ny in [sy - 1, sy + 1] {
            if ny < 0 || ny >= h {
                continue;
            }
            let mut run = false;
            for i in l..=r {
                if matches(img, &done, i, ny) {
                    if !run {
                        stack.push((i, ny));
                        run = true;
                    }
                } else {
                    run = false;
                }
            }
        }
    }
    bb
}

/// Replace every pixel matching the clicked colour, connected or not.
pub fn fill_global(img: &mut Image, x: i32, y: i32, c: Color32, tol: u8) -> IRect {
    let Some(target) = img.get(x, y) else { return IRect::EMPTY };
    let mut bb = IRect::EMPTY;
    for yy in 0..img.h {
        let mut row_hit = (usize::MAX, 0usize);
        for xx in 0..img.w {
            let p = &mut img.px[yy * img.w + xx];
            if close(*p, target, tol) {
                *p = c;
                row_hit = (row_hit.0.min(xx), xx + 1);
            }
        }
        if row_hit.0 != usize::MAX {
            bb = bb.union(IRect::new(row_hit.0 as i32, yy as i32, row_hit.1 as i32, yy as i32 + 1));
        }
    }
    bb
}

// ---- airbrush --------------------------------------------------------------------------

/// Tiny xorshift RNG - plenty for spray paint, and avoids a dependency.
pub struct Rng(u64);
impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng(seed | 1)
    }
    pub fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    pub fn range(&mut self, lo: i32, hi: i32) -> i32 {
        lo + (self.next() % (hi - lo + 1) as u64) as i32
    }
}

pub fn spray(img: &mut Image, c: (i32, i32), radius: i32, n: usize, col: Color32, rng: &mut Rng) -> IRect {
    let r = radius.max(1);
    for _ in 0..n {
        let (dx, dy) = (rng.range(-r, r), rng.range(-r, r));
        if dx * dx + dy * dy <= r * r {
            img.set(c.0 + dx, c.1 + dy, col);
        }
    }
    IRect::new(c.0 - r, c.1 - r, c.0 + r + 1, c.1 + r + 1).intersect(img.bounds())
}

#[cfg(test)]
mod tests {
    use super::*;

    const W: Color32 = Color32::WHITE;
    const K: Color32 = Color32::BLACK;
    const R: Color32 = Color32::RED;

    /// Draw an outline-only shape, fill inside it, and check nothing outside changed.
    fn assert_no_leak(make: impl Fn(IRect) -> Spans, inner_make: impl Fn(IRect, i32) -> Spans) {
        for w in 1..=40 {
            for h in 1..=40 {
                for lw in [1, 2, 3, 5] {
                    let mut img = Image::new(w as usize + 4, h as usize + 4, W);
                    let r = IRect::from_size(2, 2, w, h);
                    let outer = make(r);
                    let inner = (lw > 1).then(|| inner_make(r.inset(lw), lw));
                    draw_spans(&mut img, &outer, inner.as_ref(), Some(K), None);
                    let (cx, cy) = (2 + w / 2, 2 + h / 2);
                    if img.get(cx, cy) == Some(W) {
                        flood_fill(&mut img, cx, cy, R, 0);
                    }
                    for (x, y) in [(0, 0), (w + 3, 0), (0, h + 3), (w + 3, h + 3), (1, 2 + h / 2)] {
                        assert_eq!(img.get(x, y), Some(W), "leak: {w}x{h} lw={lw} at ({x},{y})");
                    }
                }
            }
        }
    }

    #[test]
    fn ellipse_outlines_are_fill_tight() {
        assert_no_leak(Spans::ellipse, |r, _| Spans::ellipse(r));
    }

    #[test]
    fn rect_outlines_are_fill_tight() {
        assert_no_leak(Spans::rect, |r, _| Spans::rect(r));
    }

    #[test]
    fn round_rect_outlines_are_fill_tight() {
        assert_no_leak(|r| Spans::round_rect(r, 10.0), |r, lw| Spans::round_rect(r, 10.0 - lw as f64));
    }

    #[test]
    fn thin_lines_block_four_connected_fill() {
        // Any 1px Bresenham line from edge to edge must split the canvas in two.
        for (a, b) in [((0, 0), (39, 29)), ((0, 29), (39, 0)), ((0, 5), (39, 24)), ((7, 0), (31, 29))] {
            let mut img = Image::new(40, 30, W);
            stroke(&mut img, a, b, &Mask::new(BrushShape::Round, 1), K);
            // seed on one side, test a point on the other side
            let above = (39, 0);
            let below = (0, 29);
            let (seed, other) = if img.get(above.0, above.1) == Some(W) { (above, below) } else { ((39, 29), (0, 0)) };
            flood_fill(&mut img, seed.0, seed.1, R, 0);
            assert_eq!(img.get(other.0, other.1), Some(W), "line {a:?}-{b:?} leaked");
        }
    }

    #[test]
    fn closed_curve_outline_is_tight() {
        let mut img = Image::new(60, 60, W);
        let m = Mask::new(BrushShape::Round, 1);
        let mut pts = bezier_points([(5, 30), (5, -10), (55, -10), (55, 30)]);
        pts.extend(bezier_points([(55, 30), (55, 70), (5, 70), (5, 30)]));
        stroke_polyline(&mut img, &pts, &m, K, true);
        flood_fill(&mut img, 30, 30, R, 0);
        assert_eq!(img.get(30, 30), Some(R));
        assert_eq!(img.get(0, 0), Some(W));
        assert_eq!(img.get(59, 59), Some(W));
    }

    #[test]
    fn fill_tolerance() {
        let mut img = Image::new(10, 1, W);
        img.set(5, 0, Color32::from_rgb(250, 250, 250));
        let mut exact = img.clone();
        flood_fill(&mut exact, 0, 0, R, 0);
        assert_eq!(exact.get(9, 0), Some(W), "exact fill must stop at a near-white pixel");
        flood_fill(&mut img, 0, 0, R, 8);
        assert_eq!(img.get(9, 0), Some(R), "tolerant fill crosses it");
    }

    #[test]
    fn polygon_fill_covers_interior() {
        let mut img = Image::new(20, 20, W);
        let pts = [(2, 2), (17, 2), (17, 17), (2, 17)];
        fill_polygon(&mut img, &pts, R);
        stroke_polyline(&mut img, &pts, &Mask::new(BrushShape::Round, 1), K, true);
        assert_eq!(img.get(10, 10), Some(R));
        assert_eq!(img.get(2, 10), Some(K));
        assert_eq!(img.get(1, 10), Some(W));
    }

    #[test]
    fn masks() {
        assert_eq!(Mask::new(BrushShape::Round, 1).offs, vec![(0, 0)]);
        assert_eq!(Mask::new(BrushShape::Round, 3).offs.len(), 5); // plus sign
        assert_eq!(Mask::new(BrushShape::Square, 4).offs.len(), 16);
    }
}
