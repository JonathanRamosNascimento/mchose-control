//! Vector illustration of the headset, tinted with the system accent colour.

use std::cell::Cell;
use std::f64::consts::PI;
use std::rc::Rc;

use gtk::prelude::*;
use gtk::{cairo, gdk};

#[derive(Clone)]
pub struct HeadsetArt {
    pub widget: gtk::DrawingArea,
    lit: Rc<Cell<bool>>,
}

impl HeadsetArt {
    pub fn new(width: i32, height: i32) -> Self {
        let widget = gtk::DrawingArea::builder()
            .content_width(width)
            .content_height(height)
            .halign(gtk::Align::Center)
            .accessible_role(gtk::AccessibleRole::Presentation)
            .css_classes(["accent"])
            .build();
        let lit = Rc::new(Cell::new(false));
        let l = lit.clone();
        widget.set_draw_func(move |a, cr, w, h| {
            let _ = draw(cr, w as f64, h as f64, l.get(), a.color());
        });
        Self { widget, lit }
    }

    /// `true` while the headset is connected and awake.
    pub fn set_lit(&self, lit: bool) {
        if self.lit.replace(lit) != lit {
            self.widget.queue_draw();
        }
    }
}

fn rrect(cr: &cairo::Context, x: f64, y: f64, w: f64, h: f64, r: f64) {
    let r = r.min(w / 2.0).min(h / 2.0);
    cr.new_sub_path();
    cr.arc(x + w - r, y + r, r, -PI / 2.0, 0.0);
    cr.arc(x + w - r, y + h - r, r, 0.0, PI / 2.0);
    cr.arc(x + r, y + h - r, r, PI / 2.0, PI);
    cr.arc(x + r, y + r, r, PI, 1.5 * PI);
    cr.close_path();
}

fn draw(cr: &cairo::Context, w: f64, h: f64, lit: bool, accent: gdk::RGBA) -> Result<(), cairo::Error> {
    let (ar, ag, ab) = (accent.red() as f64, accent.green() as f64, accent.blue() as f64);
    let dark = adw::StyleManager::default().is_dark();

    // Design space is 220 × 200, centred.
    let s = (w / 220.0).min(h / 200.0);
    cr.translate((w - 220.0 * s) / 2.0, (h - 200.0 * s) / 2.0);
    cr.scale(s, s);
    let alpha = if lit { 1.0 } else { 0.55 };
    cr.push_group();

    // Floor shadow
    let sh = cairo::RadialGradient::new(110.0, 186.0, 4.0, 110.0, 186.0, 92.0);
    sh.add_color_stop_rgba(0.0, 0.0, 0.0, 0.0, if dark { 0.45 } else { 0.22 });
    sh.add_color_stop_rgba(1.0, 0.0, 0.0, 0.0, 0.0);
    cr.save()?;
    cr.translate(110.0, 186.0);
    cr.scale(1.0, 0.16);
    cr.translate(-110.0, -186.0);
    cr.set_source(&sh)?;
    cr.arc(110.0, 186.0, 92.0, 0.0, 2.0 * PI);
    cr.fill()?;
    cr.restore()?;

    // Accent halo behind the headset
    if lit {
        let halo = cairo::RadialGradient::new(110.0, 95.0, 10.0, 110.0, 95.0, 100.0);
        halo.add_color_stop_rgba(0.0, ar, ag, ab, 0.28);
        halo.add_color_stop_rgba(1.0, ar, ag, ab, 0.0);
        cr.set_source(&halo)?;
        cr.arc(110.0, 95.0, 100.0, 0.0, 2.0 * PI);
        cr.fill()?;
    }

    let (cx, cy, r) = (110.0, 98.0, 72.0);
    cr.set_line_cap(cairo::LineCap::Round);

    // Headband – outer shell
    let band = cairo::LinearGradient::new(0.0, cy - r - 10.0, 0.0, cy);
    band.add_color_stop_rgb(0.0, 0.33, 0.35, 0.40);
    band.add_color_stop_rgb(1.0, 0.13, 0.14, 0.17);
    cr.set_source(&band)?;
    cr.set_line_width(15.0);
    cr.arc(cx, cy, r, PI * 1.02, PI * 1.98);
    cr.stroke()?;
    // highlight
    cr.set_source_rgba(1.0, 1.0, 1.0, 0.16);
    cr.set_line_width(2.0);
    cr.arc(cx, cy, r + 5.5, PI * 1.08, PI * 1.92);
    cr.stroke()?;
    // Inner cushion
    cr.set_source_rgba(0.07, 0.07, 0.09, 1.0);
    cr.set_line_width(7.0);
    cr.arc(cx, cy, r - 11.0, PI * 1.12, PI * 1.88);
    cr.stroke()?;
    // Accent stitch on the cushion
    cr.set_source_rgba(ar, ag, ab, 0.85);
    cr.set_line_width(1.6);
    cr.set_dash(&[3.0, 4.0], 0.0);
    cr.arc(cx, cy, r - 11.0, PI * 1.25, PI * 1.75);
    cr.stroke()?;
    cr.set_dash(&[], 0.0);

    for side in [-1.0f64, 1.0] {
        let x = cx + side * r;
        // Slider / yoke
        cr.set_source_rgba(0.55, 0.57, 0.62, 1.0);
        rrect(cr, x - 3.5, cy - 6.0, 7.0, 24.0, 3.0);
        cr.fill()?;
        cr.set_source_rgba(0.20, 0.21, 0.25, 1.0);
        cr.set_line_width(5.0);
        cr.arc(x, cy + 34.0, 24.0, PI * 1.2, PI * 1.8);
        cr.stroke()?;

        // Ear cup shell
        let (cw, ch) = (44.0, 70.0);
        let (ox, oy) = (x - cw / 2.0, cy + 18.0);
        let shell = cairo::LinearGradient::new(ox, oy, ox + cw, oy + ch);
        shell.add_color_stop_rgb(0.0, 0.36, 0.38, 0.44);
        shell.add_color_stop_rgb(1.0, 0.10, 0.11, 0.13);
        rrect(cr, ox, oy, cw, ch, 20.0);
        cr.set_source(&shell)?;
        cr.fill_preserve()?;
        cr.set_source_rgba(0.0, 0.0, 0.0, 0.5);
        cr.set_line_width(1.2);
        cr.stroke()?;

        // Accent ring
        let (iw, ih) = (28.0, 50.0);
        let (ix, iy) = (x - iw / 2.0, oy + (ch - ih) / 2.0);
        if lit {
            cr.set_source_rgba(ar, ag, ab, 0.35);
            cr.set_line_width(7.0);
            rrect(cr, ix, iy, iw, ih, 13.0);
            cr.stroke()?;
        }
        cr.set_source_rgba(ar, ag, ab, if lit { 1.0 } else { 0.35 });
        cr.set_line_width(2.4);
        rrect(cr, ix, iy, iw, ih, 13.0);
        cr.stroke()?;
        // Plate
        let plate = cairo::LinearGradient::new(ix, iy, ix, iy + ih);
        plate.add_color_stop_rgb(0.0, 0.24, 0.25, 0.30);
        plate.add_color_stop_rgb(1.0, 0.08, 0.08, 0.10);
        rrect(cr, ix + 4.0, iy + 4.0, iw - 8.0, ih - 8.0, 9.0);
        cr.set_source(&plate)?;
        cr.fill()?;
        // Shine
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.10);
        rrect(cr, ox + 6.0, oy + 5.0, 10.0, ch - 24.0, 5.0);
        cr.fill()?;
    }

    // Microphone boom (left cup)
    let (mx, my) = (cx - r + 6.0, cy + 76.0);
    cr.set_source_rgba(0.16, 0.17, 0.20, 1.0);
    cr.set_line_width(5.0);
    cr.move_to(mx, my);
    cr.curve_to(mx + 4.0, my + 22.0, mx + 28.0, my + 26.0, mx + 48.0, my + 20.0);
    cr.stroke()?;
    rrect(cr, mx + 44.0, my + 13.0, 16.0, 12.0, 6.0);
    cr.set_source_rgba(0.12, 0.12, 0.15, 1.0);
    cr.fill()?;
    cr.set_source_rgba(ar, ag, ab, if lit { 1.0 } else { 0.4 });
    cr.arc(mx + 56.0, my + 19.0, 2.2, 0.0, 2.0 * PI);
    cr.fill()?;

    cr.pop_group_to_source()?;
    cr.paint_with_alpha(alpha)?;
    Ok(())
}
