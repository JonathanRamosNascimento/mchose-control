//! "Equalizador" page.

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use gtk::glib;
use mchose_core::dsp::pipewire::SAMPLE_RATE;
use mchose_core::dsp::settings::{EQ_FREQS, EQ_MAX_DB, EQ_MIN_DB, PREAMP_MAX_DB, PREAMP_MIN_DB};

use crate::state::{Change, Controller};
use crate::widgets::eq_graph::EqGraph;
use crate::widgets::{SliderRow, fmt_db};

fn freq_label(f: f32) -> String {
    if f >= 1000.0 { format!("{}k", f / 1000.0) } else { format!("{f}") }
}

pub fn dsp_off_banner(ctl: &Rc<Controller>) -> adw::Banner {
    let banner = adw::Banner::builder()
        .title("O processamento de áudio está desligado — as mudanças são salvas, mas não aplicadas.")
        .button_label("Ativar")
        .revealed(!ctl.dsp_active.get())
        .build();
    let c = ctl.clone();
    banner.connect_button_clicked(move |_| c.set_dsp_enabled(true));
    let b = banner.clone();
    let c = ctl.clone();
    ctl.subscribe(move |ch| {
        if ch == Change::Dsp {
            b.set_revealed(!c.dsp_active.get());
        }
    });
    banner
}

pub fn build(ctl: &Rc<Controller>, window: &adw::ApplicationWindow) -> gtk::Widget {
    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(18)
        .margin_top(18)
        .margin_bottom(24)
        .margin_start(18)
        .margin_end(18)
        .build();

    // -------------------------------------------------------------- header row
    let head = gtk::Box::builder().spacing(12).build();
    let titles = gtk::Box::builder().orientation(gtk::Orientation::Vertical).hexpand(true).build();
    titles.append(
        &gtk::Label::builder()
            .label("Equalizador de 10 bandas")
            .css_classes(["title-2"])
            .wrap(true)
            .xalign(0.0)
            .build(),
    );
    titles.append(
        &gtk::Label::builder()
            .label("Arraste os pontos, use a roda do mouse ou as setas do teclado. Shift = ajuste fino, duplo clique zera.")
            .css_classes(["caption", "dim-label"])
            .wrap(true)
            .xalign(0.0)
            .build(),
    );
    let eq_switch = gtk::Switch::builder().valign(gtk::Align::Center).tooltip_text("Ativar equalizador").build();
    eq_switch.update_property(&[gtk::accessible::Property::Label("Ativar equalizador")]);
    head.append(&titles);
    content.append(&head);

    // ------------------------------------------------------------ preset chips
    let presets_row = gtk::Box::builder().spacing(8).build();
    let chips = adw::WrapBox::builder().child_spacing(6).line_spacing(6).hexpand(true).build();
    let save_btn = gtk::Button::builder()
        .icon_name("document-save-symbolic")
        .tooltip_text("Salvar como preset")
        .valign(gtk::Align::Center)
        .css_classes(["flat", "circular"])
        .build();
    let delete_btn = gtk::Button::builder()
        .icon_name("user-trash-symbolic")
        .tooltip_text("Excluir preset")
        .valign(gtk::Align::Center)
        .css_classes(["flat", "circular"])
        .build();
    let reset_btn = gtk::Button::builder()
        .icon_name("edit-undo-symbolic")
        .tooltip_text("Zerar equalizador")
        .valign(gtk::Align::Center)
        .css_classes(["flat", "circular"])
        .build();
    presets_row.append(&chips);
    content.append(&presets_row);
    let actions = gtk::Box::builder().spacing(4).valign(gtk::Align::Center).build();
    actions.append(&reset_btn);
    actions.append(&save_btn);
    actions.append(&delete_btn);
    actions.append(
        &gtk::Separator::builder().orientation(gtk::Orientation::Vertical).margin_start(6).margin_end(6).build(),
    );
    actions.append(&eq_switch);
    head.append(&actions);

    // ------------------------------------------------------------------- graph
    let card = gtk::Box::builder().orientation(gtk::Orientation::Vertical).css_classes(["eq-card"]).build();
    let graph = EqGraph::new();
    graph.widget.set_content_height(280);
    card.append(&graph.widget);
    let legend = gtk::Label::builder().css_classes(["graph-legend"]).xalign(0.0).wrap(true).visible(false).build();
    card.append(&legend);
    content.append(&card);

    // -------------------------------------------------------------- band sliders
    let bands_box = gtk::Box::builder().homogeneous(true).spacing(0).margin_top(2).build();
    let mut scales = Vec::new();
    let mut values = Vec::new();
    for (i, &f) in EQ_FREQS.iter().enumerate() {
        let col =
            gtk::Box::builder().orientation(gtk::Orientation::Vertical).spacing(4).css_classes(["eq-band"]).build();
        let val = gtk::Label::builder().label("0").css_classes(["band-value"]).build();
        let scale = gtk::Scale::with_range(gtk::Orientation::Vertical, EQ_MIN_DB as f64, EQ_MAX_DB as f64, 0.5);
        scale.set_inverted(true);
        scale.set_has_origin(false);
        scale.set_draw_value(false);
        scale.set_vexpand(true);
        scale.set_height_request(150);
        scale.set_halign(gtk::Align::Center);
        scale.update_property(&[gtk::accessible::Property::Label(&format!("{} Hz", freq_label(f)))]);
        let lab = gtk::Label::builder().label(freq_label(f)).css_classes(["band-freq"]).build();
        col.append(&val);
        col.append(&scale);
        col.append(&lab);
        bands_box.append(&col);
        let c = ctl.clone();
        let v = val.clone();
        scale.connect_value_changed(move |s| {
            let g = ((s.value() * 2.0).round() / 2.0) as f32;
            v.set_label(&if g.abs() < 0.05 {
                "0".into()
            } else if g.fract() == 0.0 {
                format!("{g:+}")
            } else {
                format!("{g:+.1}")
            });
            if !c.syncing() {
                c.update_dsp(Change::Eq, |d| {
                    d.eq_gains[i] = g;
                    d.refresh_preset_match();
                });
            }
        });
        scales.push(scale);
        values.push(val);
    }
    content.append(&bands_box);

    // ------------------------------------------------------------------ pre-amp
    let pre_group = adw::PreferencesGroup::builder().title("Pré-amplificação").build();
    let auto_row = adw::SwitchRow::builder()
        .title("Automática")
        .subtitle("Reduz o volume de entrada o necessário para que ganhos positivos não distorçam")
        .build();
    let pre = SliderRow::new("Ganho manual", None, PREAMP_MIN_DB as f64, PREAMP_MAX_DB as f64, 0.5, fmt_db);
    pre.scale.add_mark(0.0, gtk::PositionType::Bottom, None);
    pre_group.add(&auto_row);
    pre_group.add(&pre.row);
    content.append(&pre_group);

    // ------------------------------------------------------------------ wiring
    {
        let c = ctl.clone();
        graph.connect_change(move |band, gain| {
            c.update_dsp(Change::Eq, |d| {
                d.eq_gains[band] = gain;
                d.refresh_preset_match();
            });
        });
        let c = ctl.clone();
        eq_switch.connect_active_notify(move |s| {
            if !c.syncing() {
                c.update_dsp(Change::Eq, |d| d.eq_enabled = s.is_active());
            }
        });
        let c = ctl.clone();
        auto_row.connect_active_notify(move |r| {
            if !c.syncing() {
                c.update_dsp(Change::Eq, |d| d.auto_preamp = r.is_active());
            }
        });
        let c = ctl.clone();
        pre.scale.connect_value_changed(move |s| {
            if !c.syncing() {
                c.update_dsp(Change::Eq, |d| d.preamp_db = s.value() as f32);
            }
        });
        let c = ctl.clone();
        reset_btn.connect_clicked(move |_| {
            c.update_dsp(Change::Eq, |d| {
                d.apply_preset("flat");
            });
        });
        let c = ctl.clone();
        delete_btn.connect_clicked(move |_| {
            let id = c.dsp().preset;
            if id.starts_with("user-") {
                c.update_dsp(Change::Presets, |d| d.delete_user_preset(&id));
                c.notify(Change::Eq);
                c.toast("Preset excluído");
            }
        });
        let c = ctl.clone();
        let win = window.clone();
        save_btn.connect_clicked(move |_| ask_preset_name(&c, &win));
    }

    // Chips are rebuilt whenever the preset list changes.
    let chip_buttons: Rc<RefCell<Vec<(String, gtk::ToggleButton)>>> = Rc::default();
    let rebuild_chips = {
        let ctl = ctl.clone();
        let chips = chips.clone();
        let chip_buttons = chip_buttons.clone();
        move || {
            while let Some(child) = chips.first_child() {
                chips.remove(&child);
            }
            let mut list = chip_buttons.borrow_mut();
            list.clear();
            let dsp = ctl.dsp();
            let mut presets: Vec<(String, String)> = dsp.all_presets().into_iter().map(|p| (p.id, p.name)).collect();
            presets.push(("custom".into(), "Personalizado".into()));
            let mut group: Option<gtk::ToggleButton> = None;
            for (id, name) in presets {
                let b = gtk::ToggleButton::builder().label(&name).css_classes(["preset-chip", "flat"]).build();
                if id.starts_with("user-") {
                    b.set_tooltip_text(Some("Preset salvo por você"));
                }
                if let Some(g) = &group {
                    b.set_group(Some(g));
                } else {
                    group = Some(b.clone());
                }
                let c = ctl.clone();
                let pid = id.clone();
                b.connect_toggled(move |b| {
                    if b.is_active() && !c.syncing() && pid != "custom" {
                        c.update_dsp(Change::Eq, |d| {
                            d.apply_preset(&pid);
                        });
                    }
                });
                chips.append(&b);
                list.push((id, b));
            }
        }
    };
    rebuild_chips();

    let refresh = {
        let ctl = ctl.clone();
        let chip_buttons = chip_buttons.clone();
        move |change: Change| {
            if change == Change::Presets {
                rebuild_chips();
            }
            if !matches!(change, Change::Eq | Change::Presets | Change::Dsp | Change::Effects) {
                return;
            }
            let d = ctl.dsp();
            let active = ctl.dsp_active.get() && d.eq_enabled;
            graph.set_settings(&d);
            graph.set_active(d.eq_enabled);
            card.set_opacity(if active { 1.0 } else { 0.85 });
            eq_switch.set_active(d.eq_enabled);
            for (i, s) in scales.iter().enumerate() {
                if (s.value() - d.eq_gains[i] as f64).abs() > 1e-3 {
                    s.set_value(d.eq_gains[i] as f64);
                }
                s.set_sensitive(d.eq_enabled);
            }
            for (id, b) in chip_buttons.borrow().iter() {
                let on = *id == d.preset;
                if b.is_active() != on {
                    b.set_active(on);
                }
                if id == "custom" {
                    b.set_visible(d.preset == "custom");
                }
            }
            let mut extra = Vec::new();
            if d.bass.enabled {
                extra.push("Graves potentes");
            }
            if d.clarity.enabled {
                extra.push("Clareza");
            }
            if d.crossfeed.enabled {
                extra.push("Crossfeed");
            }
            legend.set_visible(!extra.is_empty());
            legend.set_label(&format!("A curva mostra a resposta final, incluindo: {}", extra.join(" · ")));
            delete_btn.set_visible(d.preset.starts_with("user-"));
            save_btn.set_sensitive(d.eq_enabled);
            auto_row.set_active(d.auto_preamp);
            pre.set(d.preamp_db as f64);
            pre.row.set_sensitive(!d.auto_preamp);
            let eff = d.effective_preamp_db(SAMPLE_RATE);
            auto_row.set_subtitle(&if d.auto_preamp {
                format!("Aplicando {} para evitar distorção", fmt_db(eff as f64))
            } else {
                "Reduz o volume de entrada o necessário para que ganhos positivos não distorçam".into()
            });
        }
    };
    ctl.run_synced(|| {
        refresh(Change::Eq);
    });
    ctl.subscribe(refresh);

    let clamp = adw::Clamp::builder().maximum_size(1120).tightening_threshold(900).child(&content).build();
    let scroll =
        gtk::ScrolledWindow::builder().hscrollbar_policy(gtk::PolicyType::Never).vexpand(true).child(&clamp).build();
    let page = gtk::Box::builder().orientation(gtk::Orientation::Vertical).build();
    page.append(&dsp_off_banner(ctl));
    page.append(&scroll);
    page.upcast()
}

fn ask_preset_name(ctl: &Rc<Controller>, window: &adw::ApplicationWindow) {
    let entry = gtk::Entry::builder().placeholder_text("Nome do preset").activates_default(true).build();
    let dialog = adw::AlertDialog::builder()
        .heading("Salvar preset")
        .body("Guarde o ajuste atual do equalizador para usar depois.")
        .extra_child(&entry)
        .default_response("save")
        .close_response("cancel")
        .build();
    dialog.add_responses(&[("cancel", "Cancelar"), ("save", "Salvar")]);
    dialog.set_response_appearance("save", adw::ResponseAppearance::Suggested);
    dialog.set_response_enabled("save", false);
    let d = dialog.clone();
    entry.connect_changed(move |e| d.set_response_enabled("save", !e.text().trim().is_empty()));
    let c = ctl.clone();
    let e = entry.clone();
    dialog.connect_response(None, move |_, resp| {
        if resp != "save" {
            return;
        }
        let name = e.text().trim().to_string();
        if name.is_empty() {
            return;
        }
        c.update_dsp(Change::Presets, |d| {
            d.save_user_preset(&name);
        });
        c.notify(Change::Eq);
        c.toast(&format!("Preset “{name}” salvo"));
    });
    dialog.present(Some(window));
    glib::idle_add_local_once(move || {
        entry.grab_focus();
    });
}
