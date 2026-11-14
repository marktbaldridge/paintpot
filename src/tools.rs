//! Tool list and their (original, hand-written) SVG icons.

use eframe::egui::Key;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Select,
    Eraser,
    Fill,
    Picker,
    Zoom,
    Pencil,
    Brush,
    Airbrush,
    Text,
    Line,
    Curve,
    Rect,
    Polygon,
    Ellipse,
    RoundRect,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FillStyle {
    Outline,
    OutlineFill,
    Fill,
}

impl Tool {
    /// Two-column order, following the classic Paint toolbox.
    pub const ALL: [Tool; 15] = [
        Tool::Select,
        Tool::Eraser,
        Tool::Fill,
        Tool::Picker,
        Tool::Zoom,
        Tool::Pencil,
        Tool::Brush,
        Tool::Airbrush,
        Tool::Text,
        Tool::Line,
        Tool::Curve,
        Tool::Rect,
        Tool::Polygon,
        Tool::Ellipse,
        Tool::RoundRect,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Tool::Select => "Select",
            Tool::Eraser => "Eraser",
            Tool::Fill => "Fill with color",
            Tool::Picker => "Pick color",
            Tool::Zoom => "Magnifier",
            Tool::Pencil => "Pencil",
            Tool::Brush => "Brush",
            Tool::Airbrush => "Airbrush",
            Tool::Text => "Text",
            Tool::Line => "Line",
            Tool::Curve => "Curve",
            Tool::Rect => "Rectangle",
            Tool::Polygon => "Polygon",
            Tool::Ellipse => "Ellipse",
            Tool::RoundRect => "Rounded rectangle",
        }
    }

    pub fn key(self) -> Key {
        match self {
            Tool::Select => Key::S,
            Tool::Eraser => Key::E,
            Tool::Fill => Key::F,
            Tool::Picker => Key::I,
            Tool::Zoom => Key::Z,
            Tool::Pencil => Key::P,
            Tool::Brush => Key::B,
            Tool::Airbrush => Key::A,
            Tool::Text => Key::T,
            Tool::Line => Key::L,
            Tool::Curve => Key::C,
            Tool::Rect => Key::R,
            Tool::Polygon => Key::G,
            Tool::Ellipse => Key::O,
            Tool::RoundRect => Key::U,
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            Tool::Select => "Drag to select. Drag the selection to move it; Ctrl+drag copies it.",
            Tool::Eraser => "Left: paint with the secondary color. Right: replace only the primary color.",
            Tool::Fill => "Click an area to fill it. Right-click fills with the secondary color.",
            Tool::Picker => "Left-click picks the primary color, right-click the secondary.",
            Tool::Zoom => "Click to zoom in, right-click to zoom out. Ctrl+wheel also zooms.",
            Tool::Pencil => "Draws single pixels. Right button uses the secondary color.",
            Tool::Brush => "Paints with the chosen brush.",
            Tool::Airbrush => "Hold the button down to spray.",
            Tool::Text => "Click where the text should go and type. Click elsewhere or pick another tool to place it; Esc cancels.",
            Tool::Line => "Drag to draw a line. Hold Shift for straight or 45 degree lines.",
            Tool::Curve => "Drag a line, then click or drag twice to bend it.",
            Tool::Rect | Tool::Ellipse | Tool::RoundRect => "Drag to draw. Hold Shift for a square or circle.",
            Tool::Polygon => "Drag the first side, then click each corner. Double-click or click the start to finish.",
        }
    }

    pub fn icon_uri(self) -> String {
        format!("bytes://tool-{:?}.svg", self)
    }

    pub fn icon_svg(self) -> &'static [u8] {
        let s: &'static str = match self {
            Tool::Select => SELECT,
            Tool::Eraser => ERASER,
            Tool::Fill => FILL,
            Tool::Picker => PICKER,
            Tool::Zoom => ZOOM,
            Tool::Pencil => PENCIL,
            Tool::Brush => BRUSH,
            Tool::Airbrush => AIRBRUSH,
            Tool::Text => TEXT,
            Tool::Line => LINE,
            Tool::Curve => CURVE,
            Tool::Rect => RECT,
            Tool::Polygon => POLYGON,
            Tool::Ellipse => ELLIPSE,
            Tool::RoundRect => ROUNDRECT,
        };
        s.as_bytes()
    }
}

macro_rules! svg {
    ($body:literal) => {
        concat!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" width="48" height="48" stroke-linejoin="round" stroke-linecap="round">"#,
            $body,
            "</svg>"
        )
    };
}

pub const SWAP: &str = svg!(
    r##"<path d="M5 9 H18 M14.5 5.5 L18 9 L14.5 12.5" fill="none" stroke="#333" stroke-width="2"/><path d="M19 15 H6 M9.5 11.5 L6 15 L9.5 18.5" fill="none" stroke="#333" stroke-width="2"/>"##
);

const SELECT: &str = svg!(r##"<rect x="3.5" y="4.5" width="17" height="15" fill="#e8f0fe" stroke="#333" stroke-width="1.8" stroke-dasharray="3 2.4"/>"##);
const ERASER: &str = svg!(
    r##"<path d="M3.5 14.5 L12 6 L19.5 13.5 L13 20 L9 20 Z" fill="#f8a5c2" stroke="#333" stroke-width="1.7"/><path d="M8 10 L15.5 17.5" stroke="#333" stroke-width="1.5"/><path d="M13 20 H21" stroke="#333" stroke-width="1.7"/>"##
);
const FILL: &str = svg!(
    r##"<path d="M4.5 11 L11 4.5 L18.5 12 L12 18.5 Z" fill="#9ecbf5" stroke="#333" stroke-width="1.7"/><path d="M4.5 11 H18" stroke="#333" stroke-width="1.5"/><path d="M8 3 L11 4.5" stroke="#333" stroke-width="1.5"/><path d="M19.5 13.5 C21.5 16 21.8 18.2 20.2 19.3 C18.6 20.3 17.2 18.6 18 16.6 Z" fill="#1976d2" stroke="#1976d2" stroke-width="0.8"/>"##
);
const PICKER: &str = svg!(
    r##"<path d="M16 3.8 a2.8 2.8 0 0 1 4.2 4.2 l-2.4 2.4 -4.2 -4.2 z" fill="#333" stroke="#333" stroke-width="1"/><path d="M13.2 7.6 L16.4 10.8 L8.5 18.7 L4.6 19.4 L5.3 15.5 Z" fill="#eef4fb" stroke="#333" stroke-width="1.6"/><path d="M5.6 17 L8.2 14.4" stroke="#e53935" stroke-width="2.2"/>"##
);
const ZOOM: &str = svg!(r##"<circle cx="10" cy="10" r="6" fill="#e3f2fd" stroke="#333" stroke-width="2"/><path d="M14.5 14.5 L20 20" stroke="#333" stroke-width="3"/><path d="M7.5 10 H12.5 M10 7.5 V12.5" stroke="#333" stroke-width="1.5"/>"##);
const PENCIL: &str = svg!(
    r##"<path d="M15.5 4 L20 8.5 L8.5 20 L3.5 20.5 L4 15.5 Z" fill="#ffd54f" stroke="#333" stroke-width="1.6"/><path d="M15.5 4 L20 8.5 L18 10.5 L13.5 6 Z" fill="#f48fb1" stroke="#333" stroke-width="1.6"/><path d="M4 15.5 L8.5 20" stroke="#333" stroke-width="1.4"/><path d="M3.5 20.5 L5 19" stroke="#333" stroke-width="2"/>"##
);
const BRUSH: &str = svg!(
    r##"<path d="M20.5 3.5 L12 12" stroke="#8d6e63" stroke-width="3"/><path d="M11.5 11 C8 11 6.8 13.4 6.6 15.6 C6.4 17.6 5 19 3.5 19.8 C7.5 21.2 12.8 20.2 13.6 15.4 C13.9 13.5 13.2 11.7 11.5 11 Z" fill="#e53935" stroke="#333" stroke-width="1.4"/>"##
);
const AIRBRUSH: &str = svg!(
    r##"<rect x="11" y="9" width="8" height="12" rx="1.6" fill="#66bb6a" stroke="#333" stroke-width="1.6"/><rect x="13" y="5.5" width="4" height="3.5" fill="#555" stroke="#333" stroke-width="1"/><g fill="#333"><circle cx="8" cy="5" r="1"/><circle cx="5" cy="7.5" r="1"/><circle cx="8.5" cy="9" r="1"/><circle cx="4" cy="11.5" r="1"/><circle cx="7" cy="12.5" r="1"/><circle cx="4.5" cy="4" r="1"/></g>"##
);
const TEXT: &str = svg!(r##"<path d="M5.5 19 L12 4 L18.5 19 M8 13.5 H16" fill="none" stroke="#333" stroke-width="2.4"/><path d="M4 22 H20" stroke="#1e88e5" stroke-width="2"/>"##);
const LINE: &str = svg!(r##"<path d="M4.5 19.5 L19.5 4.5" stroke="#333" stroke-width="2.4"/>"##);
const CURVE: &str = svg!(r##"<path d="M3.5 18 C8 0 14.5 25 20.5 6" fill="none" stroke="#333" stroke-width="2.4"/>"##);
const RECT: &str = svg!(r##"<rect x="3.5" y="5.5" width="17" height="13" fill="#fff176" stroke="#333" stroke-width="1.9"/>"##);
const POLYGON: &str = svg!(r##"<path d="M4 18.5 L6.5 5 L13.5 9.5 L20 4.5 L19.5 19 Z" fill="#ce93d8" stroke="#333" stroke-width="1.9"/>"##);
const ELLIPSE: &str = svg!(r##"<ellipse cx="12" cy="12" rx="8.7" ry="6.8" fill="#80deea" stroke="#333" stroke-width="1.9"/>"##);
const ROUNDRECT: &str = svg!(r##"<rect x="3.5" y="5.5" width="17" height="13" rx="4.2" fill="#ffab91" stroke="#333" stroke-width="1.9"/>"##);
