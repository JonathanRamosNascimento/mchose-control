//! Circular battery gauge with animated level changes and a charging shimmer.

use std::cell::Cell;
use std::f64::consts::PI;
use std::rc::Rc;

use adw::prelude::*;
use gtk::{cairo, gdk, glib};

#[derive(Clone, Copy, PartialEq)]
pub enum RingState {
    Unknown,
    Asleep,
    Level { pct: u8, charging: bool, full: bool },
}

struct Inner {
    shown: Cell<f64>,
    state: Cell<RingState>,
    anim: std::cell::RefCell<Option<adw::TimedAnimation>>,
    tick: std::cell::RefCell<Option<gtk::TickCallbackId>>,
}

#[derive(Clone)]
pub struct BatteryRing {
    pub widget: gtk::Overlay,
    area: gtk::DrawingArea,
    pct: gtk::Label,
    sub: gtk::Label,
    inner: Rc<Inner>,
}

fn palette(level: f64) -> gdk::RGBA {
    let dark = adw::StyleManager::default().is_dark();
    let (r, g, b) = if level < 0.2 {
        if dark { (0xff, 0x7b, 0x63) } else { (0xc0, 0x1c, 0x28) }
    } else if level < 0.5 {
        if dark { (0xf8, 0xe4, 0x5c) } else { (0x9c, 0x6e, 0x03) }
    } else if dark {
        (0x8f, 0xf0, 0xa4)
    } else {
        (0x1b, 0x85, 0x53)
    };
    gdk::RGBA::new(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 1.0)
}

impl BatteryRing {
    pub fn new(size: i32) -> Self {
        let area = gtk::DrawingArea::builder().content_width(size).content_height(size).build();
        let pct = gtk::Label::builder().label("—").css_classes(["big-number"]).build();
        let sub = gtk::Label::builder().label("bateria").css_classes(["caption", "dim-label"]).build();
        let center = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .valign(gtk::Align::Center)
            .halign(gtk::Align::Center)
            .build();
        center.append(&pct);
        center.append(&sub);
        let widget = gtk::Overlay::builder().child(&area).halign(gtk::Align::Center).build();
        widget.add_overlay(&center);
        let inner = Rc::new(Inner {
            shown: Cell::new(0.0),
            state: Cell::new(RingState::Unknown),
            anim: Default::default(),
            tick: Default::default(),
        });
        let ring = Self { widget, area, pct, sub, inner };
        let inner = ring.inner.clone();
        ring.area.set_draw_func(move |a, cr, w, h| {
            let _ = draw(a, cr, w as f64, h as f64, &inner);
        });
        ring
    }

    pub fn set_state(&self, state: RingState) {
        if self.inner.state.get() == state {
            return;
        }
        self.inner.state.set(state);
        let (target, pct_text, sub_text) = match state {
            RingState::Unknown => (0.0, "—".to_string(), "sem dados"),
            RingState::Asleep => (0.0, "zZ".to_string(), "em repouso"),
            RingState::Level { pct, charging, full } => (
                pct as f64 / 100.0,
                format!("{pct}%"),
                if full {
                    "carga completa"
                } else if charging {
                    "carregando"
                } else {
                    "bateria"
                },
            ),
        };
        self.pct.set_label(&pct_text);
        self.sub.set_label(sub_text);
        self.widget.update_property(&[gtk::accessible::Property::Label(&format!("Bateria: {pct_text}, {sub_text}"))]);

        let inner = self.inner.clone();
        let area = self.area.clone();
        let target_cb = adw::CallbackAnimationTarget::new(move |v| {
            inner.shown.set(v);
            area.queue_draw();
        });
        let anim = adw::TimedAnimation::new(&self.area, self.inner.shown.get(), target, 900, target_cb);
        anim.set_easing(adw::Easing::EaseOutCubic);
        anim.play();
        *self.inner.anim.borrow_mut() = Some(anim);

        let charging = matches!(state, RingState::Level { charging: true, full: false, .. });
        let mut tick = self.inner.tick.borrow_mut();
        match (charging, tick.is_some()) {
            (true, false) => {
                *tick = Some(self.area.add_tick_callback(|a, _| {
                    a.queue_draw();
                    glib::ControlFlow::Continue
                }));
            }
            (false, true) => {
                if let Some(id) = tick.take() {
                    id.remove();
                }
            }
            _ => {}
        }
    }
}

fn draw(area: &gtk::DrawingArea, cr: &cairo::Context, w: f64, h: f64, inner: &Inner) -> Result<(), cairo::Error> {
    let size = w.min(h);
    let (cx, cy) = (w / 2.0, h / 2.0);
    let lw = (size * 0.085).max(8.0);
    let r = size / 2.0 - lw;
    let fg = area.color();
    let start = 0.75 * PI;
    let sweep = 1.5 * PI;

    cr.set_line_cap(cairo::LineCap::Round);
    cr.set_line_width(lw);
    cr.set_source_rgba(fg.red() as f64, fg.green() as f64, fg.blue() as f64, 0.10);
    cr.arc(cx, cy, r, start, start + sweep);
    cr.stroke()?;

    let v = inner.shown.get().clamp(0.0, 1.0);
    if v > 0.001 {
        let c = palette(v);
        let (cr_, cg, cb) = (c.red() as f64, c.green() as f64, c.blue() as f64);
        let end = start + sweep * v;
        // Soft glow
        cr.set_line_width(lw * 1.9);
        cr.set_source_rgba(cr_, cg, cb, 0.12);
        cr.arc(cx, cy, r, start, end);
        cr.stroke()?;
        cr.set_line_width(lw);
        cr.set_source_rgba(cr_, cg, cb, 1.0);
        cr.arc(cx, cy, r, start, end);
        cr.stroke()?;

        if matches!(inner.state.get(), RingState::Level { charging: true, full: false, .. }) {
            // Shimmer travelling along the filled arc.
            let t = area.frame_clock().map_or(0.0, |fc| fc.frame_time() as f64 / 1e6);
            let phase = (t * 0.6).fract();
            let pos = start + (end - start) * phase;
            let span = 0.35;
            cr.set_source_rgba(1.0, 1.0, 1.0, 0.55 * (1.0 - (phase * 2.0 - 1.0).abs()));
            cr.set_line_width(lw * 0.45);
            cr.arc(cx, cy, r, (pos - span).max(start), pos.min(end));
            cr.stroke()?;
        }
    }
    Ok(())
}
