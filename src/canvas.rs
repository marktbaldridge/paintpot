//! The pixel buffer and integer rectangles. Everything here is plain, opaque RGB
//! (stored as `Color32` with alpha 255) - like classic Paint there is no alpha channel,
//! which keeps flood fill and colour matching exact.

use eframe::egui::{Color32, ColorImage};

/// Integer rectangle: `x0,y0` inclusive, `x1,y1` exclusive.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct IRect {
    pub x0: i32,
    pub y0: i32,
    pub x1: i32,
    pub y1: i32,
}

impl IRect {
    pub const EMPTY: IRect = IRect { x0: 0, y0: 0, x1: 0, y1: 0 };

    pub const fn new(x0: i32, y0: i32, x1: i32, y1: i32) -> Self {
        Self { x0, y0, x1, y1 }
    }
    pub fn from_size(x: i32, y: i32, w: i32, h: i32) -> Self {
        Self::new(x, y, x + w, y + h)
    }
    /// Rectangle covering both pixels `a` and `b` (inclusive corners).
    pub fn from_corners(a: (i32, i32), b: (i32, i32)) -> Self {
        Self::new(a.0.min(b.0), a.1.min(b.1), a.0.max(b.0) + 1, a.1.max(b.1) + 1)
    }
    pub fn is_empty(&self) -> bool {
        self.x1 <= self.x0 || self.y1 <= self.y0
    }
    pub fn width(&self) -> i32 {
        (self.x1 - self.x0).max(0)
    }
    pub fn height(&self) -> i32 {
        (self.y1 - self.y0).max(0)
    }
    pub fn union(self, o: IRect) -> IRect {
        if self.is_empty() {
            return o;
        }
        if o.is_empty() {
            return self;
        }
        IRect::new(self.x0.min(o.x0), self.y0.min(o.y0), self.x1.max(o.x1), self.y1.max(o.y1))
    }
    pub fn intersect(self, o: IRect) -> IRect {
        let r = IRect::new(self.x0.max(o.x0), self.y0.max(o.y0), self.x1.min(o.x1), self.y1.min(o.y1));
        if r.is_empty() {
            IRect::EMPTY
        } else {
            r
        }
    }
    pub fn inset(self, n: i32) -> IRect {
        let r = IRect::new(self.x0 + n, self.y0 + n, self.x1 - n, self.y1 - n);
        if r.is_empty() {
            IRect::EMPTY
        } else {
            r
        }
    }
    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x0 && x < self.x1 && y >= self.y0 && y < self.y1
    }
}

#[derive(Clone, PartialEq, Debug)]
pub struct Image {
    pub w: usize,
    pub h: usize,
    pub px: Vec<Color32>,
}

impl Image {
    pub fn new(w: usize, h: usize, c: Color32) -> Self {
        let (w, h) = (w.max(1), h.max(1));
        Self { w, h, px: vec![c; w * h] }
    }

    /// Build from straight RGBA bytes, compositing any transparency onto white.
    pub fn from_rgba(w: usize, h: usize, rgba: &[u8]) -> Self {
        let mut px = Vec::with_capacity(w * h);
        for p in rgba.chunks_exact(4).take(w * h) {
            let a = p[3] as u32;
            let mix = |c: u8| ((c as u32 * a + 255 * (255 - a) + 127) / 255) as u8;
            px.push(Color32::from_rgb(mix(p[0]), mix(p[1]), mix(p[2])));
        }
        px.resize(w * h, Color32::WHITE);
        Self { w: w.max(1), h: h.max(1), px }
    }

    pub fn to_rgba(&self) -> Vec<u8> {
        self.px.iter().flat_map(|c| [c.r(), c.g(), c.b(), 255]).collect()
    }
    pub fn to_rgb(&self) -> Vec<u8> {
        self.px.iter().flat_map(|c| [c.r(), c.g(), c.b()]).collect()
    }

    pub fn bounds(&self) -> IRect {
        IRect::new(0, 0, self.w as i32, self.h as i32)
    }
    #[inline]
    pub fn in_bounds(&self, x: i32, y: i32) -> bool {
        x >= 0 && y >= 0 && (x as usize) < self.w && (y as usize) < self.h
    }
    #[inline]
    pub fn get(&self, x: i32, y: i32) -> Option<Color32> {
        if self.in_bounds(x, y) {
            Some(self.px[y as usize * self.w + x as usize])
        } else {
            None
        }
    }
    #[inline]
    pub fn set(&mut self, x: i32, y: i32, c: Color32) {
        if self.in_bounds(x, y) {
            self.px[y as usize * self.w + x as usize] = c;
        }
    }

    /// Horizontal run `[x0, x1)` on row `y`, clipped.
    pub fn hline(&mut self, y: i32, x0: i32, x1: i32, c: Color32) {
        if y < 0 || y as usize >= self.h {
            return;
        }
        let a = x0.max(0) as usize;
        let b = x1.min(self.w as i32).max(0) as usize;
        if a < b {
            let row = y as usize * self.w;
            self.px[row + a..row + b].fill(c);
        }
    }

    pub fn fill_rect(&mut self, r: IRect, c: Color32) -> IRect {
        let r = r.intersect(self.bounds());
        for y in r.y0..r.y1 {
            self.hline(y, r.x0, r.x1, c);
        }
        r
    }

    /// Copy of the pixels in `r` (pixels outside the image come out white).
    pub fn crop(&self, r: IRect) -> Image {
        let mut out = Image::new(r.width() as usize, r.height() as usize, Color32::WHITE);
        out.blit(self, -r.x0, -r.y0, None);
        out
    }

    /// Draw `src` with its top-left at `(x, y)`. Pixels equal to `key` are skipped
    /// (used for "transparent" selections). Returns the touched rectangle.
    pub fn blit(&mut self, src: &Image, x: i32, y: i32, key: Option<Color32>) -> IRect {
        let dst = IRect::from_size(x, y, src.w as i32, src.h as i32).intersect(self.bounds());
        if dst.is_empty() {
            return dst;
        }
        let n = dst.width() as usize;
        for dy in dst.y0..dst.y1 {
            let s = (dy - y) as usize * src.w + (dst.x0 - x) as usize;
            let d = dy as usize * self.w + dst.x0 as usize;
            let srow = &src.px[s..s + n];
            let drow = &mut self.px[d..d + n];
            match key {
                None => drow.copy_from_slice(srow),
                Some(k) => {
                    for (dp, sp) in drow.iter_mut().zip(srow) {
                        if *sp != k {
                            *dp = *sp;
                        }
                    }
                }
            }
        }
        dst
    }

    /// Copy rectangle `r` from a same-sized image.
    pub fn copy_rect_from(&mut self, src: &Image, r: IRect) {
        debug_assert!(src.w == self.w && src.h == self.h);
        let r = r.intersect(self.bounds());
        for y in r.y0..r.y1 {
            let s = y as usize * self.w + r.x0 as usize;
            let e = s + r.width() as usize;
            self.px[s..e].copy_from_slice(&src.px[s..e]);
        }
    }

    pub fn to_color_image(&self) -> ColorImage {
        ColorImage { size: [self.w, self.h], pixels: self.px.clone() }
    }
    pub fn region_color_image(&self, r: IRect) -> ColorImage {
        let c = self.crop(r);
        ColorImage { size: [c.w, c.h], pixels: c.px }
    }
    /// Colour image where pixels equal to `key` become fully transparent.
    pub fn keyed_color_image(&self, key: Option<Color32>) -> ColorImage {
        let pixels = self
            .px
            .iter()
            .map(|&p| if Some(p) == key { Color32::TRANSPARENT } else { p })
            .collect();
        ColorImage { size: [self.w, self.h], pixels }
    }

    // ---- whole-image transforms -------------------------------------------------

    pub fn flip_h(&self) -> Image {
        let mut o = self.clone();
        for row in o.px.chunks_exact_mut(self.w) {
            row.reverse();
        }
        o
    }
    pub fn flip_v(&self) -> Image {
        let mut px = Vec::with_capacity(self.px.len());
        for row in self.px.chunks_exact(self.w).rev() {
            px.extend_from_slice(row);
        }
        Image { w: self.w, h: self.h, px }
    }
    pub fn rotate_cw(&self) -> Image {
        let (nw, nh) = (self.h, self.w);
        let mut px = vec![Color32::WHITE; nw * nh];
        for y in 0..self.h {
            for x in 0..self.w {
                px[x * nw + (self.h - 1 - y)] = self.px[y * self.w + x];
            }
        }
        Image { w: nw, h: nh, px }
    }
    pub fn rotate_ccw(&self) -> Image {
        let (nw, nh) = (self.h, self.w);
        let mut px = vec![Color32::WHITE; nw * nh];
        for y in 0..self.h {
            for x in 0..self.w {
                px[(self.w - 1 - x) * nw + y] = self.px[y * self.w + x];
            }
        }
        Image { w: nw, h: nh, px }
    }
    pub fn rotate_180(&self) -> Image {
        let mut o = self.clone();
        o.px.reverse();
        o
    }
    pub fn invert(&self) -> Image {
        let px = self
            .px
            .iter()
            .map(|c| Color32::from_rgb(255 - c.r(), 255 - c.g(), 255 - c.b()))
            .collect();
        Image { w: self.w, h: self.h, px }
    }
    /// Nearest-neighbour stretch (no smoothing, so colours stay exact).
    pub fn stretched(&self, w: usize, h: usize) -> Image {
        let (w, h) = (w.max(1), h.max(1));
        let mut px = Vec::with_capacity(w * h);
        for y in 0..h {
            let sy = y * self.h / h;
            for x in 0..w {
                let sx = x * self.w / w;
                px.push(self.px[sy * self.w + sx]);
            }
        }
        Image { w, h, px }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Image {
        let mut img = Image::new(5, 3, Color32::WHITE);
        for (i, p) in img.px.iter_mut().enumerate() {
            *p = Color32::from_rgb(i as u8, (i * 7) as u8, 3);
        }
        img
    }

    #[test]
    fn rotations_round_trip() {
        let a = sample();
        assert_eq!(a.rotate_cw().rotate_ccw(), a);
        assert_eq!(a.rotate_cw().rotate_cw(), a.rotate_180());
        assert_eq!(a.flip_h().flip_h(), a);
        assert_eq!(a.flip_v().flip_v(), a);
        assert_eq!(a.flip_h().flip_v(), a.rotate_180());
        let r = a.rotate_cw();
        assert_eq!((r.w, r.h), (3, 5));
        // top-left of the original ends up top-right after a clockwise turn
        assert_eq!(r.get(2, 0), a.get(0, 0));
    }

    #[test]
    fn crop_and_blit() {
        let a = sample();
        let c = a.crop(IRect::new(1, 1, 4, 3));
        assert_eq!((c.w, c.h), (3, 2));
        assert_eq!(c.get(0, 0), a.get(1, 1));
        let mut b = Image::new(5, 3, Color32::BLACK);
        b.blit(&c, 1, 1, None);
        assert_eq!(b.get(3, 2), a.get(3, 2));
        assert_eq!(b.get(0, 0), Some(Color32::BLACK));
    }
}
