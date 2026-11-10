//! "Microfone" page: WebRTC noise suppression and input level.

use std::rc::Rc;

use adw::prelude::*;
use mchose_core::dsp::MicSettings;

use crate::state::{Change, Controller};
use crate::widgets::{SliderRow, fmt_pct};

pub fn build(ctl: &Rc<Controller>) -> gtk::Widget {
    let page = adw::PreferencesPage::new();

    let clean = adw::PreferencesGroup::builder()
        .title("Microfone limpo")
        .description(
            "Cria a entrada “Microfone limpo” com o processamento de voz do WebRTC (o mesmo usado em \
             navegadores e apps de chamada). Escolha essa entrada no Discord, OBS, etc. — ou defina-a como padrão.",
        )
        .build();
    let master = adw::SwitchRow::builder().title("Ativar microfone limpo").subtitle("Desligado").build();
    master.add_prefix(&gtk::Image::builder().icon_name("mchose-waveform-symbolic").pixel_size(20).build());
    clean.add(&master);

    let opts = adw::PreferencesGroup::builder().title("Processamento").build();
    let ns = adw::SwitchRow::builder()
        .title("Supressão de ruído")
        .subtitle("Ventiladores, ar-condicionado, ruído de fundo")
        .build();
    let agc =
        adw::SwitchRow::builder().title("Ganho automático").subtitle("Mantém sua voz num volume constante").build();
    let hp =
        adw::SwitchRow::builder().title("Filtro passa-alta").subtitle("Remove ronco e batidas graves na mesa").build();
    let ts = adw::SwitchRow::builder()
        .title("Supressão de transientes")
        .subtitle("Atenua cliques de teclado e mouse")
        .build();
    let def = adw::SwitchRow::builder()
        .title("Usar como microfone padrão")
        .subtitle("Ao desligar, o microfone padrão anterior é restaurado")
        .build();
    for r in [&ns, &agc, &hp, &ts, &def] {
        opts.add(r);
    }

    let level = adw::PreferencesGroup::builder().title("Nível de entrada").build();
    let vol = SliderRow::new("Volume do microfone do headset", None, 0.0, 1.0, 0.01, fmt_pct);
    level.add(&vol.row);

    page.add(&clean);
    page.add(&opts);
    page.add(&level);

    // ------------------------------------------------------------------ wiring
    let bind = |row: &adw::SwitchRow, set: fn(&mut MicSettings, bool)| {
        let c = ctl.clone();
        row.connect_active_notify(move |r| {
            if !c.syncing() {
                let v = r.is_active();
                c.update_mic(|m| set(m, v));
            }
        });
    };
    bind(&master, |m, v| m.enabled = v);
    bind(&ns, |m, v| m.noise_suppression = v);
    bind(&agc, |m, v| m.gain_control = v);
    bind(&hp, |m, v| m.high_pass = v);
    bind(&ts, |m, v| m.transient_suppression = v);
    bind(&def, |m, v| m.set_default = v);
    {
        let c = ctl.clone();
        vol.scale.connect_value_changed(move |s| {
            if !c.syncing() {
                c.set_volume(true, s.value() as f32);
            }
        });
    }

    let refresh = {
        let ctl = ctl.clone();
        move |change: Change| match change {
            Change::Mic | Change::Dsp | Change::Device => {
                let m = ctl.mic();
                let active = ctl.mic_active.get();
                master.set_active(m.enabled);
                master.set_subtitle(match (m.enabled, active, ctl.busy.get()) {
                    (_, _, true) => "Aplicando…",
                    (true, true, _) => "Ativo — entrada “Microfone limpo” disponível",
                    (true, false, _) => "Ativado, mas o serviço não está rodando",
                    (false, _, _) => "Desligado",
                });
                ns.set_active(m.noise_suppression);
                agc.set_active(m.gain_control);
                hp.set_active(m.high_pass);
                ts.set_active(m.transient_suppression);
                def.set_active(m.set_default);
                opts.set_sensitive(m.enabled);
                let has_source = ctl.hw.borrow().source.is_some();
                master.set_sensitive(has_source || m.enabled);
            }
            Change::Volume => {
                if let Some(v) = ctl.mic_volume.get() {
                    vol.set(v.level as f64);
                }
                let has_source = ctl.hw.borrow().source.is_some();
                vol.row.set_sensitive(has_source);
                master.set_sensitive(has_source || ctl.mic().enabled);
            }
            _ => {}
        }
    };
    ctl.run_synced(|| {
        refresh(Change::Mic);
        refresh(Change::Volume);
    });
    ctl.subscribe(refresh);

    page.upcast()
}
