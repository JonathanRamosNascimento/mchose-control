//! Development helpers driven by environment variables.
//!
//! * `MCHOSE_DEMO=1` — fake a connected headset (no HID access, settings never saved)
//! * `MCHOSE_SCREENSHOT_DIR=<dir>` — render every page to `<dir>/<page>.png` with the
//!   software (Cairo) GSK renderer, then quit. Works under any GDK backend.

use std::path::{Path, PathBuf};
use std::time::Duration;

use adw::prelude::*;
use gtk::{glib, gsk};

pub fn demo_enabled() -> bool {
    std::env::var_os("MCHOSE_DEMO").is_some_and(|v| v != "0")
}

fn render_png(widget: &impl IsA<gtk::Widget>, path: &Path) -> Result<(), String> {
    let widget = widget.as_ref();
    let (w, h) = (widget.width(), widget.height());
    let paintable = gtk::WidgetPaintable::new(Some(widget));
    let snapshot = gtk::Snapshot::new();
    paintable.snapshot(&snapshot, w as f64, h as f64);
    let node = snapshot.to_node().ok_or("empty snapshot")?;
    let renderer = gsk::CairoRenderer::new();
    renderer.realize_for_display(&widget.display()).map_err(|e| e.to_string())?;
    let texture = renderer.render_texture(&node, None);
    renderer.unrealize();
    texture.save_to_png(path).map_err(|e| e.to_string())
}

/// `MCHOSE_WINDOW_SIZE=WxH` overrides the initial window size.
pub fn apply_window_size(window: &adw::ApplicationWindow) {
    let Ok(v) = std::env::var("MCHOSE_WINDOW_SIZE") else { return };
    if let Some((w, h)) = v.split_once('x').and_then(|(w, h)| Some((w.parse().ok()?, h.parse().ok()?))) {
        window.set_default_size(w, h);
    }
}

pub fn maybe_take_screenshots(app: &adw::Application, window: &adw::ApplicationWindow, stack: &adw::ViewStack) {
    let Some(dir) = std::env::var_os("MCHOSE_SCREENSHOT_DIR").map(PathBuf::from) else { return };
    let _ = std::fs::create_dir_all(&dir);
    // Offscreen captures may not get frame-clock ticks: make CSS transitions instant.
    if let Some(settings) = gtk::Settings::default() {
        settings.set_gtk_enable_animations(false);
    }
    let pages: Vec<String> = std::env::var("MCHOSE_SCREENSHOT_PAGES")
        .unwrap_or_else(|_| "headset,eq,effects,mic".into())
        .split(',')
        .map(str::to_string)
        .collect();
    if std::env::var_os("MCHOSE_MEASURE").is_some() {
        let mut child = stack.first_child();
        while let Some(page) = child {
            report_min_width(&page, 0);
            child = page.next_sibling();
        }
    }
    let (app, window, stack) = (app.clone(), window.clone(), stack.clone());
    let step = std::rc::Rc::new(std::cell::Cell::new(0usize));
    glib::timeout_add_local(Duration::from_millis(1400), move || {
        let i = step.get();
        // Even steps switch page, odd steps capture it (gives layout + animations time).
        let page = i / 2;
        if page >= pages.len() {
            app.quit();
            return glib::ControlFlow::Break;
        }
        if i % 2 == 0 {
            stack.set_visible_child_name(&pages[page]);
        } else {
            let path = dir.join(format!("{}.png", pages[page]));
            match render_png(&window, &path) {
                Ok(()) => eprintln!("screenshot: {}", path.display()),
                Err(e) => eprintln!("screenshot {}: {e}", path.display()),
            }
        }
        step.set(i + 1);
        glib::ControlFlow::Continue
    });
}

/// Prints the minimum width of a widget tree (debugging narrow layouts).
fn report_min_width(w: &gtk::Widget, depth: usize) {
    let (min, _, _, _) = w.measure(gtk::Orientation::Horizontal, -1);
    let limit: i32 = std::env::var("MCHOSE_MEASURE").ok().and_then(|v| v.parse().ok()).unwrap_or(330);
    if min >= limit || depth < 2 {
        eprintln!("{:indent$}{} {:?} min={min}", "", w.type_().name(), w.css_classes(), indent = depth * 2);
    }
    if min >= limit {
        let mut c = w.first_child();
        while let Some(ch) = c {
            report_min_width(&ch, depth + 1);
            c = ch.next_sibling();
        }
    }
}
