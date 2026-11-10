pub mod battery_ring;
pub mod eq_graph;
pub mod headset_art;

use adw::prelude::*;

/// A preferences row with a horizontal slider and a live value label.
pub struct SliderRow {
    pub row: adw::ActionRow,
    pub scale: gtk::Scale,
    pub value: gtk::Label,
    fmt: fn(f64) -> String,
}

impl SliderRow {
    pub fn new(title: &str, subtitle: Option<&str>, min: f64, max: f64, step: f64, fmt: fn(f64) -> String) -> Self {
        let scale = gtk::Scale::with_range(gtk::Orientation::Horizontal, min, max, step);
        scale.set_draw_value(false);
        scale.set_hexpand(true);
        scale.set_valign(gtk::Align::Center);
        scale.set_width_request(110);
        scale.update_property(&[gtk::accessible::Property::Label(title)]);
        let value = gtk::Label::builder().css_classes(["value-label", "dim-label"]).xalign(1.0).build();
        let row = adw::ActionRow::builder().title(title).title_lines(2).css_classes(["slider-row"]).build();
        if let Some(s) = subtitle {
            row.set_subtitle(s);
        }
        row.add_suffix(&scale);
        row.add_suffix(&value);
        let s = Self { row, scale, value, fmt };
        s.refresh_label();
        let v = s.value.clone();
        scale_label_sync(&s.scale, v, fmt);
        s
    }

    pub fn set(&self, v: f64) {
        if (self.scale.value() - v).abs() > 1e-6 {
            self.scale.set_value(v);
        }
        self.refresh_label();
    }

    fn refresh_label(&self) {
        self.value.set_label(&(self.fmt)(self.scale.value()));
    }
}

fn scale_label_sync(scale: &gtk::Scale, label: gtk::Label, fmt: fn(f64) -> String) {
    scale.connect_value_changed(move |s| label.set_label(&fmt(s.value())));
}

/// Small coloured "● Conectado" pill.
pub struct StatusPill {
    pub widget: gtk::Box,
    label: gtk::Label,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PillKind {
    Ok,
    Warn,
    Error,
    Neutral,
}

impl StatusPill {
    pub fn new() -> Self {
        let widget = gtk::Box::builder().spacing(8).halign(gtk::Align::Start).css_classes(["status-pill"]).build();
        let dot = gtk::Box::builder().valign(gtk::Align::Center).css_classes(["dot"]).build();
        let label = gtk::Label::new(None);
        widget.append(&dot);
        widget.append(&label);
        Self { widget, label }
    }

    pub fn set(&self, kind: PillKind, text: &str) {
        for c in ["ok", "warn", "error"] {
            self.widget.remove_css_class(c);
        }
        match kind {
            PillKind::Ok => self.widget.add_css_class("ok"),
            PillKind::Warn => self.widget.add_css_class("warn"),
            PillKind::Error => self.widget.add_css_class("error"),
            PillKind::Neutral => {}
        }
        self.label.set_label(text);
    }
}

pub fn fmt_db(v: f64) -> String {
    if v.abs() < 0.05 { "0 dB".into() } else { format!("{v:+.1} dB") }
}

pub fn fmt_hz(v: f64) -> String {
    if v >= 1000.0 { format!("{:.1} kHz", v / 1000.0) } else { format!("{v:.0} Hz") }
}

pub fn fmt_pct(v: f64) -> String {
    format!("{:.0}%", v * 100.0)
}
