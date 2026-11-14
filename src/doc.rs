//! The document: the live image, a copy of the last committed state ("base"), and
//! region-based undo. Tools draw straight into `img`; `commit()` diffs it against `base`
//! and stores only the changed rectangle, so undo stays cheap even for large pictures.

use crate::canvas::{IRect, Image};
use std::path::PathBuf;

const MAX_UNDO_BYTES: usize = 768 * 1024 * 1024;
const MAX_UNDO_STEPS: usize = 500;

enum Entry {
    Region { r: IRect, before: Image, after: Image },
    Full { before: Image, after: Image },
}

impl Entry {
    fn bytes(&self) -> usize {
        match self {
            Entry::Region { before, after, .. } | Entry::Full { before, after } => (before.px.len() + after.px.len()) * 4,
        }
    }
}

pub struct Doc {
    pub img: Image,
    base: Image,
    undo: Vec<Entry>,
    redo: Vec<Entry>,
    undo_bytes: usize,
    /// Area of `img` changed since the texture was last uploaded.
    pub dirty: IRect,
    /// Size changed (or new image): re-create the whole texture.
    pub full_upload: bool,
    pub path: Option<PathBuf>,
    pub modified: bool,
    /// Pixels cut off by shrinking the canvas, kept so growing it again brings them back.
    hidden: Option<Hidden>,
}

/// The "used to be" pixels. Only canvas resizes read or write this; every tool works on
/// `img` alone, so fills, selections and saving are naturally limited to the visible size.
/// Frozen: drawing never reaches it, and the visible pixels always win when it is refreshed.
pub struct Hidden {
    pub img: Image,
    /// Which pixels of `img` were ever real (the extents seen can form an L shape).
    known: Vec<bool>,
}

impl Hidden {
    fn is_known(&self, x: usize, y: usize) -> bool {
        x < self.img.w && y < self.img.h && self.known[y * self.img.w + x]
    }

    /// Hidden pixels as a texture image: transparent where unknown or where `visible` covers.
    pub fn ghost_image(&self, visible: (usize, usize)) -> eframe::egui::ColorImage {
        let (w, h) = (self.img.w, self.img.h);
        let pixels = (0..w * h)
            .map(|i| {
                let (x, y) = (i % w, i / w);
                if self.known[i] && (x >= visible.0 || y >= visible.1) {
                    self.img.px[i]
                } else {
                    eframe::egui::Color32::TRANSPARENT
                }
            })
            .collect();
        eframe::egui::ColorImage { size: [w, h], pixels }
    }
}

impl Doc {
    pub fn new(img: Image, path: Option<PathBuf>) -> Self {
        Self {
            base: img.clone(),
            img,
            undo: Vec::new(),
            redo: Vec::new(),
            undo_bytes: 0,
            dirty: IRect::EMPTY,
            full_upload: true,
            path,
            modified: false,
            hidden: None,
        }
    }

    /// Hidden pixels outside the current picture, if any.
    pub fn hidden(&self) -> Option<&Hidden> {
        self.hidden.as_ref()
    }

    /// Forget the hidden pixels (rotate, flip, stretch, crop: they no longer line up).
    pub fn discard_hidden(&mut self) {
        self.hidden = None;
    }

    /// Change the canvas size without losing anything: pixels cut off now are kept hidden,
    /// and growing brings back hidden pixels before falling back to `fill`.
    /// (The caller commits, so this is one undo step like any resize.)
    pub fn resize_canvas(&mut self, w: usize, h: usize, fill: eframe::egui::Color32) {
        let (w, h) = (w.max(1), h.max(1));
        // 1. Refresh the hidden layer: old hidden pixels, then the current picture on top.
        let (old_w, old_h) = self.hidden.as_ref().map_or((0, 0), |hd| (hd.img.w, hd.img.h));
        let (hw, hh) = (old_w.max(self.img.w), old_h.max(self.img.h));
        let mut img = Image::new(hw, hh, fill);
        let mut known = vec![false; hw * hh];
        if let Some(old) = &self.hidden {
            for y in 0..old.img.h {
                for x in 0..old.img.w {
                    if old.known[y * old.img.w + x] {
                        img.px[y * hw + x] = old.img.px[y * old.img.w + x];
                        known[y * hw + x] = true;
                    }
                }
            }
        }
        img.blit(&self.img, 0, 0, None);
        for y in 0..self.img.h {
            known[y * hw..y * hw + self.img.w].fill(true);
        }
        let hidden = Hidden { img, known };

        // 2. The new picture: hidden pixels where we have them, `fill` beyond.
        let mut out = Image::new(w, h, fill);
        for y in 0..h.min(hh) {
            for x in 0..w.min(hw) {
                if hidden.is_known(x, y) {
                    out.px[y * w + x] = hidden.img.px[y * hw + x];
                }
            }
        }
        // Keep the layer only while something is actually hidden.
        self.hidden = (hw > w || hh > h).then_some(hidden);
        self.set_image(out);
    }

    pub fn mark(&mut self, r: IRect) {
        self.dirty = self.dirty.union(r.intersect(self.img.bounds()));
    }

    fn same_size(&self) -> bool {
        self.img.w == self.base.w && self.img.h == self.base.h
    }

    /// Put back committed pixels in `r` (used to redraw shape previews).
    pub fn restore(&mut self, r: IRect) {
        if self.same_size() {
            let r = r.intersect(self.img.bounds());
            self.img.copy_rect_from(&self.base, r);
            self.mark(r);
        }
    }

    /// Throw away everything drawn since the last commit.
    pub fn revert(&mut self) {
        if self.same_size() {
            let d = self.diff_rect();
            self.img.copy_rect_from(&self.base, d);
            self.mark(d);
        } else {
            self.img = self.base.clone();
            self.full_upload = true;
        }
    }

    pub fn set_image(&mut self, img: Image) {
        if img.w != self.img.w || img.h != self.img.h {
            self.full_upload = true;
        } else {
            self.dirty = img.bounds();
        }
        self.img = img;
    }

    fn diff_rect(&self) -> IRect {
        let w = self.img.w;
        let mut r = IRect::EMPTY;
        for y in 0..self.img.h {
            let a = &self.img.px[y * w..(y + 1) * w];
            let b = &self.base.px[y * w..(y + 1) * w];
            if a == b {
                continue;
            }
            let first = a.iter().zip(b).position(|(p, q)| p != q).unwrap();
            let last = w - 1 - a.iter().rev().zip(b.iter().rev()).position(|(p, q)| p != q).unwrap();
            r = r.union(IRect::new(first as i32, y as i32, last as i32 + 1, y as i32 + 1));
        }
        r
    }

    /// Record everything drawn since the last commit as one undo step.
    pub fn commit(&mut self) -> bool {
        let entry = if !self.same_size() {
            let before = std::mem::replace(&mut self.base, self.img.clone());
            Entry::Full { before, after: self.img.clone() }
        } else {
            let r = self.diff_rect();
            if r.is_empty() {
                return false;
            }
            let before = self.base.crop(r);
            let after = self.img.crop(r);
            self.base.copy_rect_from(&self.img, r);
            Entry::Region { r, before, after }
        };
        self.push_undo(entry);
        self.redo.clear();
        self.modified = true;
        true
    }

    fn push_undo(&mut self, e: Entry) {
        self.undo_bytes += e.bytes();
        self.undo.push(e);
        while (self.undo_bytes > MAX_UNDO_BYTES || self.undo.len() > MAX_UNDO_STEPS) && self.undo.len() > 1 {
            let old = self.undo.remove(0);
            self.undo_bytes -= old.bytes();
        }
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn undo(&mut self) -> bool {
        self.revert();
        let Some(e) = self.undo.pop() else { return false };
        self.undo_bytes -= e.bytes();
        self.apply(&e, true);
        self.redo.push(e);
        self.modified = true;
        true
    }

    pub fn redo(&mut self) -> bool {
        self.revert();
        let Some(e) = self.redo.pop() else { return false };
        self.apply(&e, false);
        self.push_undo(e);
        self.modified = true;
        true
    }

    fn apply(&mut self, e: &Entry, undo: bool) {
        match e {
            Entry::Region { r, before, after } => {
                let src = if undo { before } else { after };
                self.img.blit(src, r.x0, r.y0, None);
                self.base.blit(src, r.x0, r.y0, None);
                self.mark(*r);
            }
            Entry::Full { before, after } => {
                let src = if undo { before } else { after };
                self.img = src.clone();
                self.base = src.clone();
                self.full_upload = true;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::Color32;

    #[test]
    fn undo_redo_region_and_resize() {
        let mut d = Doc::new(Image::new(8, 8, Color32::WHITE), None);
        let original = d.img.clone();
        d.img.set(3, 4, Color32::RED);
        assert!(d.commit());
        let painted = d.img.clone();
        d.resize_canvas(12, 5, Color32::WHITE);
        assert!(d.commit());
        assert_eq!((d.img.w, d.img.h), (12, 5));

        assert!(d.undo());
        assert_eq!(d.img, painted);
        assert!(d.undo());
        assert_eq!(d.img, original);
        assert!(!d.undo());
        assert!(d.redo());
        assert!(d.redo());
        assert_eq!((d.img.w, d.img.h), (12, 5));
        assert_eq!(d.img.get(3, 4), Some(Color32::RED));
    }

    const R: Color32 = Color32::RED;
    const G: Color32 = Color32::GREEN;
    const B: Color32 = Color32::BLUE;
    const W: Color32 = Color32::WHITE;

    fn doc_10x10() -> Doc {
        let mut d = Doc::new(Image::new(10, 10, W), None);
        d.img.set(9, 9, R); // bottom-right corner
        d.img.set(9, 0, G); // top-right corner
        d.commit();
        d
    }

    #[test]
    fn shrinking_then_growing_brings_pixels_back() {
        let mut d = doc_10x10();
        d.resize_canvas(5, 5, B);
        d.commit();
        assert_eq!((d.img.w, d.img.h), (5, 5));
        assert!(d.hidden().is_some());
        d.resize_canvas(10, 10, B);
        d.commit();
        assert_eq!(d.img.get(9, 9), Some(R), "hidden corner came back");
        assert_eq!(d.img.get(9, 0), Some(G));
        assert!(d.hidden().is_none(), "nothing hidden any more");
    }

    #[test]
    fn drawing_after_shrink_wins_and_new_area_uses_fill() {
        let mut d = doc_10x10();
        d.resize_canvas(5, 5, B);
        d.commit();
        d.img.fill_rect(d.img.bounds(), G); // e.g. bucket-fill the smaller picture
        d.commit();
        d.resize_canvas(12, 10, B);
        d.commit();
        assert_eq!(d.img.get(2, 2), Some(G), "what was drawn while small is kept");
        assert_eq!(d.img.get(9, 9), Some(R), "frozen hidden pixels return unchanged");
        assert_eq!(d.img.get(7, 2), Some(W), "old white background returns too");
        assert_eq!(d.img.get(11, 5), Some(B), "beyond anything ever seen: fill colour");
    }

    #[test]
    fn l_shaped_history_fills_the_never_seen_corner() {
        let mut d = Doc::new(Image::new(10, 2, R), None);
        d.resize_canvas(2, 10, G); // now 2 wide, 10 tall: new rows are green
        d.commit();
        d.resize_canvas(10, 10, B);
        d.commit();
        assert_eq!(d.img.get(9, 0), Some(R), "from the wide picture");
        assert_eq!(d.img.get(0, 9), Some(G), "from the tall picture");
        assert_eq!(d.img.get(9, 9), Some(B), "corner never existed: current fill colour");
    }

    #[test]
    fn undo_redo_across_hidden_resizes() {
        let mut d = doc_10x10();
        d.resize_canvas(5, 5, B);
        d.commit();
        assert!(d.undo());
        assert_eq!((d.img.w, d.img.h), (10, 10));
        assert_eq!(d.img.get(9, 9), Some(R));
        assert!(d.redo());
        assert_eq!((d.img.w, d.img.h), (5, 5));
        d.resize_canvas(10, 10, B);
        d.commit();
        assert_eq!(d.img.get(9, 9), Some(R));
        d.discard_hidden();
        d.resize_canvas(4, 4, B);
        d.commit();
        d.discard_hidden(); // e.g. a rotate happened
        d.resize_canvas(10, 10, B);
        d.commit();
        assert_eq!(d.img.get(9, 9), Some(B), "discarded pixels are gone for good");
    }

    #[test]
    fn empty_commit_is_ignored() {
        let mut d = Doc::new(Image::new(4, 4, Color32::WHITE), None);
        assert!(!d.commit());
        assert!(!d.can_undo());
    }
}
