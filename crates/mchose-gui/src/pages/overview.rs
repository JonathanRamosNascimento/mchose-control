//! "Headset" page: live device status, battery, sound mode, volume and info.

use std::rc::Rc;

use adw::prelude::*;
use mchose_core::{ChargeState, SoundMode, protocol};

use crate::state::{Change, Conn, Controller};
use crate::widgets::battery_ring::{BatteryRing, RingState};
use crate::widgets::headset_art::HeadsetArt;
use crate::widgets::{PillKind, SliderRow, StatusPill, fmt_pct};

pub struct Overview {
    pub widget: gtk::Widget,
    /// Switched to vertical on narrow windows.
    pub columns: gtk::Box,
}

fn mode_tile(icon: &str, title: &str, sub: &str) -> gtk::ToggleButton {
    let b = gtk::Box::builder().orientation(gtk::Orientation::Vertical).spacing(6).build();
    b.append(&gtk::Image::builder().icon_name(icon).pixel_size(28).margin_bottom(4).build());
    b.append(&gtk::Label::builder().label(title).css_classes(["mode-title"]).build());
    b.append(
        &gtk::Label::builder()
            .label(sub)
            .css_classes(["mode-sub"])
            .wrap(true)
            .justify(gtk::Justification::Center)
            .build(),
    );
    let t = gtk::ToggleButton::builder().child(&b).hexpand(true).css_classes(["mode-tile", "flat"]).build();
    t.update_property(&[gtk::accessible::Property::Label(&format!("Modo de som {title}"))]);
    t
}

fn info_row(title: &str) -> (adw::ActionRow, gtk::Label) {
    let row = adw::ActionRow::builder().title(title).build();
    let v = gtk::Label::builder()
        .css_classes(["dim-label", "numeric"])
        .selectable(true)
        .ellipsize(gtk::pango::EllipsizeMode::Middle)
        .max_width_chars(28)
        .build();
    row.add_suffix(&v);
    (row, v)
}

pub fn build(ctl: &Rc<Controller>) -> Overview {
    // ------------------------------------------------------------- hero (left)
    let hero = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(6)
        .css_classes(["hero"])
        .width_request(300)
        .build();
    let brand = gtk::Label::builder().label("MCHOSE").css_classes(["brand"]).xalign(0.0).build();
    let name = gtk::Label::builder().label("Headset").css_classes(["product-name"]).xalign(0.0).build();
    let pill = StatusPill::new();
    let art = HeadsetArt::new(260, 220);
    art.widget.set_margin_top(10);
    let ring = BatteryRing::new(150);
    ring.widget.set_margin_top(4);
    let charge_hint = gtk::Label::builder()
        .css_classes(["caption", "dim-label"])
        .wrap(true)
        .justify(gtk::Justification::Center)
        .margin_top(6)
        .build();
    hero.append(&brand);
    hero.append(&name);
    hero.append(&pill.widget);
    hero.append(&art.widget);
    hero.append(&ring.widget);
    hero.append(&charge_hint);

    // ------------------------------------------------------- sound mode panel
    let mode_panel =
        gtk::Box::builder().orientation(gtk::Orientation::Vertical).spacing(12).css_classes(["panel"]).build();
    let mode_head = gtk::Box::builder().orientation(gtk::Orientation::Vertical).spacing(2).build();
    mode_head.append(&gtk::Label::builder().label("Modo de som").css_classes(["panel-title"]).xalign(0.0).build());
    mode_head.append(
        &gtk::Label::builder()
            .label("Perfis gravados no headset — também alternam pelo botão de modo do fone.")
            .css_classes(["caption", "dim-label"])
            .wrap(true)
            .xalign(0.0)
            .build(),
    );
    mode_panel.append(&mode_head);
    let tiles_box = gtk::Box::builder().spacing(10).homogeneous(true).build();
    let t_g1 = mode_tile("mchose-crosshair-symbolic", "Game 1", "Perfil de jogo");
    let t_g2 = mode_tile("input-gaming-symbolic", "Game 2", "Perfil de jogo alternativo");
    let t_mu = mode_tile("mchose-music-symbolic", "Music", "Perfil de música");
    t_g2.set_group(Some(&t_g1));
    t_mu.set_group(Some(&t_g1));
    let tiles = [(SoundMode::Game1, t_g1), (SoundMode::Game2, t_g2), (SoundMode::Music, t_mu)];
    for (m, t) in &tiles {
        tiles_box.append(t);
        let ctl = ctl.clone();
        let m = *m;
        t.connect_toggled(move |t| {
            if t.is_active() && !ctl.syncing() {
                ctl.set_mode(m);
            }
        });
    }
    mode_panel.append(&tiles_box);
    let mode_hint =
        gtk::Label::builder().css_classes(["caption", "hint-card"]).wrap(true).xalign(0.0).visible(false).build();
    mode_panel.append(&mode_hint);

    // ------------------------------------------------------------ volume group
    let vol_group = adw::PreferencesGroup::builder().title("Volume").build();
    let vol = SliderRow::new("Headset", None, 0.0, 1.0, 0.01, fmt_pct);
    let mic = SliderRow::new("Microfone", None, 0.0, 1.0, 0.01, fmt_pct);
    let mute_btn = |icon: &str, tip: &str| {
        gtk::ToggleButton::builder()
            .icon_name(icon)
            .valign(gtk::Align::Center)
            .tooltip_text(tip)
            .css_classes(["flat", "circular"])
            .build()
    };
    let vol_mute = mute_btn("audio-volume-high-symbolic", "Silenciar headset");
    let mic_mute = mute_btn("audio-input-microphone-symbolic", "Silenciar microfone");
    vol.row.add_prefix(&vol_mute);
    mic.row.add_prefix(&mic_mute);
    vol_group.add(&vol.row);
    vol_group.add(&mic.row);
    {
        let c = ctl.clone();
        vol.scale.connect_value_changed(move |s| {
            if !c.syncing() {
                c.set_volume(false, s.value() as f32);
            }
        });
        let c = ctl.clone();
        mic.scale.connect_value_changed(move |s| {
            if !c.syncing() {
                c.set_volume(true, s.value() as f32);
            }
        });
        let c = ctl.clone();
        vol_mute.connect_toggled(move |b| {
            b.set_icon_name(if b.is_active() { "audio-volume-muted-symbolic" } else { "audio-volume-high-symbolic" });
            if !c.syncing() {
                c.set_muted(false, b.is_active());
            }
        });
        let c = ctl.clone();
        mic_mute.connect_toggled(move |b| {
            b.set_icon_name(if b.is_active() {
                "microphone-sensitivity-muted-symbolic"
            } else {
                "audio-input-microphone-symbolic"
            });
            if !c.syncing() {
                c.set_muted(true, b.is_active());
            }
        });
    }

    // --------------------------------------------------------------- info group
    let info_group = adw::PreferencesGroup::builder().title("Informações").build();
    let (r_rx, v_rx) = info_row("Firmware do receptor");
    let (r_hs, v_hs) = info_row("Firmware do headset");
    let (r_sn, v_sn) = info_row("Número de série");
    let (r_port, v_port) = info_row("Conexão");
    for r in [&r_rx, &r_hs, &r_sn, &r_port] {
        info_group.add(r);
    }

    let right = gtk::Box::builder().orientation(gtk::Orientation::Vertical).spacing(22).hexpand(true).build();
    right.append(&mode_panel);
    right.append(&vol_group);
    right.append(&info_group);

    let columns =
        gtk::Box::builder().spacing(22).margin_top(18).margin_bottom(24).margin_start(18).margin_end(18).build();
    columns.append(&hero);
    columns.append(&right);
    let clamp = adw::Clamp::builder().maximum_size(1120).tightening_threshold(900).child(&columns).build();
    let scroll = gtk::ScrolledWindow::builder().hscrollbar_policy(gtk::PolicyType::Never).child(&clamp).build();

    // ------------------------------------------------------------------ refresh
    let refresh = {
        let ctl = ctl.clone();
        move |change: Change| {
            match change {
                Change::Device => {}
                Change::Volume => {
                    if let Some(v) = ctl.volume.get() {
                        vol.set(v.level as f64);
                        vol_mute.set_active(v.muted);
                    }
                    if let Some(v) = ctl.mic_volume.get() {
                        mic.set(v.level as f64);
                        mic_mute.set_active(v.muted);
                    }
                    let (has_sink, has_source) = {
                        let hw = ctl.hw.borrow();
                        (hw.sink.is_some(), hw.source.is_some())
                    };
                    vol.row.set_sensitive(has_sink);
                    mic.row.set_sensitive(has_source);
                    return;
                }
                _ => return,
            }
            let d = ctl.device.borrow().clone();
            let product = d.info.as_ref().map(|i| i.product.trim_start_matches("MCHOSE").trim().to_string());
            name.set_label(product.as_deref().filter(|p| !p.is_empty()).unwrap_or("Headset"));
            let awake = d.headset_awake();
            art.set_lit(awake);
            match d.conn {
                Conn::Searching => pill.set(PillKind::Neutral, "Receptor não encontrado"),
                Conn::Denied => pill.set(PillKind::Error, "Sem permissão de acesso"),
                Conn::Connected => match d.battery {
                    None => pill.set(PillKind::Ok, "Receptor conectado"),
                    Some(b) if b.asleep => pill.set(PillKind::Warn, "Headset desligado ou em repouso"),
                    Some(_) => pill.set(PillKind::Ok, "Conectado · 2.4 GHz"),
                },
            }
            ring.set_state(match d.battery {
                Some(b) if d.conn == Conn::Connected && b.asleep => RingState::Asleep,
                Some(b) if d.conn == Conn::Connected => RingState::Level {
                    pct: b.level,
                    charging: b.charge == ChargeState::Charging,
                    full: b.charge == ChargeState::Full,
                },
                _ => RingState::Unknown,
            });
            charge_hint.set_label(match (d.conn, d.battery) {
                (Conn::Connected, Some(b)) if b.asleep => "Ligue o headset para ver a bateria e trocar o modo.",
                (Conn::Connected, Some(b)) if b.charge == ChargeState::Charging => "Carregando pelo USB-C",
                (Conn::Connected, Some(b)) if b.level <= 15 => "Bateria baixa — conecte o carregador",
                (Conn::Searching, _) => "Conecte o receptor 2.4 GHz em uma porta USB.",
                _ => "",
            });
            charge_hint.set_visible(!charge_hint.label().is_empty());

            for (m, t) in &tiles {
                t.set_active(d.mode == Some(*m));
                t.set_sensitive(awake);
            }
            let old_fw = d.fw_receiver.filter(|v| v.value() < protocol::MIN_RECEIVER_FW_FOR_FEATURES);
            if let Some(v) = old_fw {
                mode_hint.set_label(&format!(
                    "O receptor está com firmware {v}. Bateria e modos de som exigem firmware 0008 ou superior \
                     (atualizável pelo MCHOSE HUB oficial)."
                ));
                mode_hint.set_visible(true);
            } else {
                mode_hint.set_visible(false);
            }

            let fmt = |v: Option<mchose_core::FirmwareVersion>| v.map_or("—".to_string(), |v| v.to_string());
            v_rx.set_label(&fmt(d.fw_receiver));
            v_hs.set_label(&fmt(d.fw_headset));
            v_sn.set_label(d.info.as_ref().map_or("—", |i| i.serial.as_str()));
            v_port.set_label(&match &d.info {
                Some(i) => format!("{} · {}", i.devnode.display(), i.phys.split('/').next().unwrap_or("")),
                None => "—".into(),
            });
        }
    };
    ctl.run_synced(|| {
        refresh(Change::Device);
        refresh(Change::Volume);
    });
    ctl.subscribe(refresh);

    Overview { widget: scroll.upcast(), columns }
}
