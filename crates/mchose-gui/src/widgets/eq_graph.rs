//! Interactive frequency-response graph with one draggable handle per EQ band.
//!
//! * drag a handle vertically to change the band gain (Shift = fine steps)
//! * scroll over the graph to nudge the band under the pointer
//! * double-click a handle to reset it to 0 dB
//! * keyboard: Left/Right selects a band, Up/Down changes it, 0 resets

use std::cell::{Cell, RefCell};
use std::f64::consts::PI;
use std::rc::Rc;

use gtk::prelude::*;
use gtk::{cairo, gdk, glib, pango};
use mchose_core::dsp::DspSettings;
use mchose_core::dsp::pipewire::SAMPLE_RATE;
use mchose_core::dsp::response::log_space;
use mchose_core::dsp::settings::{EQ_FREQS, EQ_MAX_DB, EQ_MIN_DB};

const DB_RANGE: f64 = 15.0;
const F_MIN: f64 = 20.0;
const F_MAX: f64 = 20000.0;
const PAD_L: f64 = 40.0;
const PAD_R: f64 = 16.0;
const PAD_T: f64 = 16.0;
const PAD_B: f64 = 28.0;
const HANDLE_R: f64 = 7.0;

type ChangeFn = Box<dyn Fn(usize, f32)>;

struct State {
    settings: RefCell<DspSettings>,
    active: Cell<bool>,
    hover: Cell<Option<usize>>,
    focus_band: Cell<usize>,
    drag: Cell<Option<(usize, f64)>>,
    drag_value: Cell<f32>,
    on_change: RefCell<Option<ChangeFn>>,
}

#[derive(Clone)]
pub struct EqGraph {
    pub widget: gtk::DrawingArea,
    st: Rc<State>,
}

struct Geo {
    x0: f64,
    x1: f64,
    y0: f64,
    y1: f64,
}

impl Geo {
    fn new(w: f64, h: f64) -> Self {
        Self { x0: PAD_L, x1: (w - PAD_R).max(PAD_L + 10.0), y0: PAD_T, y1: (h - PAD_B).max(PAD_T + 10.0) }
    }
    fn x(&self, f: f64) -> f64 {
        self.x0 + (f.log10() - F_MIN.log10()) / (F_MAX.log10() - F_MIN.log10()) * (self.x1 - self.x0)
    }
    fn y(&self, db: f64) -> f64 {
        self.y0 + (DB_RANGE - db) / (2.0 * DB_RANGE) * (self.y1 - self.y0)
    }
    fn db(&self, y: f64) -> f64 {
        DB_RANGE - (y - self.y0) / (self.y1 - self.y0) * 2.0 * DB_RANGE
    }
}

fn fmt_freq(f: f32) -> String {
    if f >= 1000.0 { format!("{}k", f / 1000.0) } else { format!("{f}") }
}

fn quantize(db: f64, fine: bool) -> f32 {
    let step = if fine { 0.1 } else { 0.5 };
    ((db / step).round() * step).clamp(EQ_MIN_DB as f64, EQ_MAX_DB as f64) as f32
}

impl EqGraph {
    pub fn new() -> Self {
        let widget = gtk::DrawingArea::builder()
            .hexpand(true)
            .content_height(260)
            .focusable(true)
            .can_focus(true)
            .accessible_role(gtk::AccessibleRole::Img)
            .css_classes(["accent"])
            .build();
        widget.update_property(&[gtk::accessible::Property::Label("Curva do equalizador")]);
        let st = Rc::new(State {
            settings: RefCell::new(DspSettings::default()),
            active: Cell::new(true),
            hover: Cell::new(None),
            focus_band: Cell::new(5),
            drag: Cell::new(None),
            drag_value: Cell::new(0.0),
            on_change: RefCell::new(None),
        });
        let g = Self { widget, st };
        g.setup_drawing();
        g.setup_input();
        g
    }

    pub fn connect_change(&self, f: impl Fn(usize, f32) + 'static) {
        *self.st.on_change.borrow_mut() = Some(Box::new(f));
    }

    pub fn set_settings(&self, s: &DspSettings) {
        *self.st.settings.borrow_mut() = s.clone();
        self.widget.queue_draw();
    }

    pub fn set_active(&self, active: bool) {
        self.st.active.set(active);
        self.widget.queue_draw();
    }

    fn emit(st: &State, band: usize, gain: f32) {
        st.settings.borrow_mut().eq_gains[band] = gain;
        if let Some(f) = st.on_change.borrow().as_ref() {
            f(band, gain);
        }
    }

    fn band_at(st: &State, geo: &Geo, x: f64, y: f64, strict: bool) -> Option<usize> {
        let s = st.settings.borrow();
        let mut best = None;
        let mut best_d = f64::MAX;
        for (i, &f) in EQ_FREQS.iter().enumerate() {
            let hx = geo.x(f as f64);
            let hy = geo.y(s.eq_gains[i] as f64);
            let d = if strict { ((hx - x).powi(2) + (hy - y).powi(2)).sqrt() } else { (hx - x).abs() };
            if d < best_d {
                best_d = d;
                best = Some(i);
            }
        }
        let limit = if strict { HANDLE_R * 2.4 } else { 40.0 };
        best.filter(|_| best_d <= limit)
    }

    fn setup_input(&self) {
        let area = &self.widget;

        let motion = gtk::EventControllerMotion::new();
        {
            let st = self.st.clone();
            let a = area.clone();
            motion.connect_motion(move |_, x, y| {
                if st.drag.get().is_some() {
                    return;
                }
                let geo = Geo::new(a.width() as f64, a.height() as f64);
                let h = Self::band_at(&st, &geo, x, y, false);
                if h != st.hover.get() {
                    st.hover.set(h);
                    a.set_cursor_from_name(if h.is_some() { Some("grab") } else { None });
                    a.queue_draw();
                }
            });
            let st = self.st.clone();
            let a = area.clone();
            motion.connect_leave(move |_| {
                if st.drag.get().is_none() {
                    st.hover.set(None);
                    a.queue_draw();
                }
            });
        }
        area.add_controller(motion);

        let drag = gtk::GestureDrag::new();
        {
            let st = self.st.clone();
            let a = area.clone();
            drag.connect_drag_begin(move |g, x, y| {
                let geo = Geo::new(a.width() as f64, a.height() as f64);
                match Self::band_at(&st, &geo, x, y, false) {
                    Some(b) => {
                        a.grab_focus();
                        st.drag.set(Some((b, y)));
                        st.focus_band.set(b);
                        st.hover.set(Some(b));
                        st.drag_value.set(st.settings.borrow().eq_gains[b]);
                        a.set_cursor_from_name(Some("grabbing"));
                        // Clicking anywhere in the column jumps the handle there.
                        let fine = g.current_event_state().contains(gdk::ModifierType::SHIFT_MASK);
                        let v = quantize(geo.db(y), fine);
                        if st.active.get() {
                            st.drag_value.set(v);
                            Self::emit(&st, b, v);
                        }
                        a.queue_draw();
                    }
                    None => {
                        g.set_state(gtk::EventSequenceState::Denied);
                    }
                }
            });
            let st = self.st.clone();
            let a = area.clone();
            drag.connect_drag_update(move |g, _dx, dy| {
                let Some((b, y_start)) = st.drag.get() else { return };
                if !st.active.get() {
                    return;
                }
                let geo = Geo::new(a.width() as f64, a.height() as f64);
                let fine = g.current_event_state().contains(gdk::ModifierType::SHIFT_MASK);
                let v = quantize(geo.db(y_start + dy), fine);
                if (v - st.drag_value.get()).abs() > f32::EPSILON {
                    st.drag_value.set(v);
                    Self::emit(&st, b, v);
                    a.queue_draw();
                }
            });
            let st = self.st.clone();
            let a = area.clone();
            drag.connect_drag_end(move |_, _, _| {
                st.drag.set(None);
                a.set_cursor_from_name(if st.hover.get().is_some() { Some("grab") } else { None });
                a.queue_draw();
            });
        }
        area.add_controller(drag);

        let click = gtk::GestureClick::new();
        {
            let st = self.st.clone();
            let a = area.clone();
            click.connect_pressed(move |_, n, x, y| {
                if n != 2 || !st.active.get() {
                    return;
                }
                let geo = Geo::new(a.width() as f64, a.height() as f64);
                if let Some(b) = Self::band_at(&st, &geo, x, y, false) {
                    Self::emit(&st, b, 0.0);
                    a.queue_draw();
                }
            });
        }
        area.add_controller(click);

        let scroll = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::VERTICAL);
        {
            let st = self.st.clone();
            let a = area.clone();
            scroll.connect_scroll(move |c, _, dy| {
                let Some(b) = st.hover.get() else { return glib::Propagation::Proceed };
                if !st.active.get() {
                    return glib::Propagation::Proceed;
                }
                let fine = c.current_event_state().contains(gdk::ModifierType::SHIFT_MASK);
                let step = if fine { 0.1 } else { 0.5 };
                let cur = st.settings.borrow().eq_gains[b] as f64;
                let v = quantize(cur - dy.signum() * step, fine);
                Self::emit(&st, b, v);
                a.queue_draw();
                glib::Propagation::Stop
            });
        }
        area.add_controller(scroll);

        let keys = gtk::EventControllerKey::new();
        {
            let st = self.st.clone();
            let a = area.clone();
            keys.connect_key_pressed(move |_, key, _, state| {
                let b = st.focus_band.get();
                let fine = state.contains(gdk::ModifierType::SHIFT_MASK);
                let step = if fine { 0.1 } else { 0.5 };
                let cur = st.settings.borrow().eq_gains[b] as f64;
                match key {
                    gdk::Key::Left => st.focus_band.set(b.saturating_sub(1)),
                    gdk::Key::Right => st.focus_band.set((b + 1).min(EQ_FREQS.len() - 1)),
                    gdk::Key::Up | gdk::Key::KP_Up if st.active.get() => Self::emit(&st, b, quantize(cur + step, fine)),
                    gdk::Key::Down | gdk::Key::KP_Down if st.active.get() => {
                        Self::emit(&st, b, quantize(cur - step, fine))
                    }
                    gdk::Key::_0 | gdk::Key::KP_0 if st.active.get() => Self::emit(&st, b, 0.0),
                    _ => return glib::Propagation::Proceed,
                }
                a.queue_draw();
                glib::Propagation::Stop
            });
        }
        area.add_controller(keys);
        let a = area.clone();
        let focus = gtk::EventControllerFocus::new();
        focus.connect_enter(move |_| a.queue_draw());
        let a = area.clone();
        focus.connect_leave(move |_| a.queue_draw());
        area.add_controller(focus);
    }

    fn setup_drawing(&self) {
        let st = self.st.clone();
        self.widget.set_draw_func(move |area, cr, w, h| {
            let _ = draw(area, cr, w as f64, h as f64, &st);
        });
    }
}

fn set_rgba(cr: &cairo::Context, c: &gdk::RGBA, alpha: f64) {
    cr.set_source_rgba(c.red() as f64, c.green() as f64, c.blue() as f64, c.alpha() as f64 * alpha);
}

fn text(
    area: &gtk::DrawingArea,
    cr: &cairo::Context,
    s: &str,
    (x, y): (f64, f64),
    align: f64,
    size_pt: i32,
    bold: bool,
) {
    let layout = area.create_pango_layout(Some(s));
    let mut fd = pango::FontDescription::new();
    fd.set_size(size_pt * pango::SCALE);
    if bold {
        fd.set_weight(pango::Weight::Bold);
    }
    layout.set_font_description(Some(&fd));
    let (lw, lh) = layout.pixel_size();
    cr.move_to(x - lw as f64 * align, y - lh as f64 / 2.0);
    pangocairo::functions::show_layout(cr, &layout);
}

fn draw(area: &gtk::DrawingArea, cr: &cairo::Context, w: f64, h: f64, st: &State) -> Result<(), cairo::Error> {
    let geo = Geo::new(w, h);
    // The widget carries libadwaita's `.accent` class, so its CSS colour *is* the
    // accent; neutral ink comes from the parent.
    let accent_full = area.color();
    let fg = area.parent().map_or(accent_full, |p| p.color());
    let active = st.active.get();
    let accent = if active { accent_full } else { fg };
    let s = st.settings.borrow();

    // Grid --------------------------------------------------------------------
    cr.set_line_width(1.0);
    for db in [-12.0, -6.0, 0.0, 6.0, 12.0] {
        let y = geo.y(db).round() + 0.5;
        set_rgba(cr, &fg, if db == 0.0 { 0.22 } else { 0.08 });
        if db != 0.0 {
            cr.set_dash(&[3.0, 4.0], 0.0);
        }
        cr.move_to(geo.x0, y);
        cr.line_to(geo.x1, y);
        cr.stroke()?;
        cr.set_dash(&[], 0.0);
        set_rgba(cr, &fg, 0.45);
        let label = if db > 0.0 { format!("+{db}") } else { format!("{db}") };
        text(area, cr, &label, (geo.x0 - 8.0, y), 1.0, 8, false);
    }
    for (i, &f) in EQ_FREQS.iter().enumerate() {
        let x = geo.x(f as f64).round() + 0.5;
        let hl = st.hover.get() == Some(i) || st.drag.get().is_some_and(|d| d.0 == i);
        set_rgba(cr, if hl { &accent } else { &fg }, if hl { 0.35 } else { 0.06 });
        cr.move_to(x, geo.y0);
        cr.line_to(x, geo.y1);
        cr.stroke()?;
        set_rgba(cr, if hl { &accent } else { &fg }, if hl { 0.95 } else { 0.5 });
        text(area, cr, &fmt_freq(f), (x, geo.y1 + 15.0), 0.5, 8, hl);
    }

    // Response curve ----------------------------------------------------------
    let freqs = log_space(F_MIN as f32, F_MAX as f32, ((geo.x1 - geo.x0) / 2.0).max(64.0) as usize);
    let resp = s.response_db(&freqs, SAMPLE_RATE);
    let pts: Vec<(f64, f64)> = freqs
        .iter()
        .zip(&resp)
        .map(|(&f, &d)| (geo.x(f as f64), geo.y((d as f64).clamp(-DB_RANGE, DB_RANGE))))
        .collect();
    let zero = geo.y(0.0);

    // Fill between curve and 0 dB.
    cr.move_to(pts[0].0, zero);
    for &(x, y) in &pts {
        cr.line_to(x, y);
    }
    cr.line_to(pts[pts.len() - 1].0, zero);
    cr.close_path();
    let grad = cairo::LinearGradient::new(0.0, geo.y0, 0.0, geo.y1);
    let a = if active { 1.0 } else { 0.4 };
    grad.add_color_stop_rgba(0.0, accent.red() as f64, accent.green() as f64, accent.blue() as f64, 0.38 * a);
    grad.add_color_stop_rgba(0.5, accent.red() as f64, accent.green() as f64, accent.blue() as f64, 0.06 * a);
    grad.add_color_stop_rgba(1.0, accent.red() as f64, accent.green() as f64, accent.blue() as f64, 0.30 * a);
    cr.set_source(&grad)?;
    cr.fill()?;

    // Glow + line.
    cr.set_line_join(cairo::LineJoin::Round);
    cr.set_line_cap(cairo::LineCap::Round);
    for (width, alpha) in [(8.0, 0.10), (4.0, 0.18), (2.4, 1.0)] {
        cr.move_to(pts[0].0, pts[0].1);
        for &(x, y) in &pts[1..] {
            cr.line_to(x, y);
        }
        set_rgba(cr, &accent, alpha * if active { 1.0 } else { 0.5 });
        cr.set_line_width(width);
        cr.stroke()?;
    }

    // Handles -----------------------------------------------------------------
    let has_focus = area.has_visible_focus();
    for (i, &f) in EQ_FREQS.iter().enumerate() {
        let g = s.eq_gains[i] as f64;
        let (x, y) = (geo.x(f as f64), geo.y(g));
        let dragging = st.drag.get().is_some_and(|d| d.0 == i);
        let hovered = st.hover.get() == Some(i);
        let focused = has_focus && st.focus_band.get() == i;
        let r = if dragging {
            HANDLE_R + 2.5
        } else if hovered || focused {
            HANDLE_R + 1.5
        } else {
            HANDLE_R
        };
        if dragging || hovered {
            set_rgba(cr, &accent, 0.25);
            cr.arc(x, y, r + 6.0, 0.0, 2.0 * PI);
            cr.fill()?;
        }
        // Stem from 0 dB to the handle.
        set_rgba(cr, &accent, 0.35);
        cr.set_line_width(2.0);
        cr.move_to(x, zero);
        cr.line_to(x, y);
        cr.stroke()?;
        cr.arc(x, y, r, 0.0, 2.0 * PI);
        set_rgba(cr, &accent, if active { 1.0 } else { 0.6 });
        cr.fill_preserve()?;
        cr.set_source_rgba(1.0, 1.0, 1.0, if active { 0.95 } else { 0.5 });
        cr.set_line_width(2.0);
        cr.stroke()?;
        if focused {
            set_rgba(cr, &fg, 0.7);
            cr.set_line_width(1.5);
            cr.set_dash(&[2.0, 2.0], 0.0);
            cr.arc(x, y, r + 4.0, 0.0, 2.0 * PI);
            cr.stroke()?;
            cr.set_dash(&[], 0.0);
        }
    }

    // Value bubble ------------------------------------------------------------
    let bubble =
        st.drag.get().map(|d| d.0).or(st.hover.get()).or(if has_focus { Some(st.focus_band.get()) } else { None });
    if let Some(i) = bubble {
        let g = s.eq_gains[i];
        let label = format!("{} Hz  {:+.1} dB", fmt_freq(EQ_FREQS[i]), g);
        let layout = area.create_pango_layout(Some(&label));
        let mut fd = pango::FontDescription::new();
        fd.set_size(9 * pango::SCALE);
        fd.set_weight(pango::Weight::Bold);
        layout.set_font_description(Some(&fd));
        let (lw, lh) = layout.pixel_size();
        let (bw, bh) = (lw as f64 + 16.0, lh as f64 + 8.0);
        let hx = geo.x(EQ_FREQS[i] as f64);
        let hy = geo.y(g as f64);
        let bx = (hx - bw / 2.0).clamp(geo.x0, geo.x1 - bw);
        let by = if hy - bh - 16.0 < geo.y0 { hy + 16.0 } else { hy - bh - 16.0 };
        rounded_rect(cr, bx, by, bw, bh, bh / 2.0);
        set_rgba(cr, &accent_full, 0.95);
        cr.fill()?;
        cr.set_source_rgba(1.0, 1.0, 1.0, 1.0);
        cr.move_to(bx + 8.0, by + 4.0);
        pangocairo::functions::show_layout(cr, &layout);
    }
    Ok(())
}

fn rounded_rect(cr: &cairo::Context, x: f64, y: f64, w: f64, h: f64, r: f64) {
    cr.new_sub_path();
    cr.arc(x + w - r, y + r, r, -PI / 2.0, 0.0);
    cr.arc(x + w - r, y + h - r, r, 0.0, PI / 2.0);
    cr.arc(x + r, y + h - r, r, PI / 2.0, PI);
    cr.arc(x + r, y + r, r, PI, 1.5 * PI);
    cr.close_path();
}
