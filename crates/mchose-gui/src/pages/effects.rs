//! "Efeitos" page: bass boost, clarity, crossfeed and balance.

use std::rc::Rc;

use adw::prelude::*;
use mchose_core::dsp::settings::{BASS_FREQ_RANGE, BASS_MAX_DB, CLARITY_FREQ_RANGE, CLARITY_MAX_DB};

use crate::pages::equalizer::dsp_off_banner;
use crate::state::{Change, Controller};
use crate::widgets::{SliderRow, fmt_db, fmt_hz, fmt_pct};

fn expander(title: &str, subtitle: &str, icon: &str) -> adw::ExpanderRow {
    let row = adw::ExpanderRow::builder().title(title).subtitle(subtitle).show_enable_switch(true).build();
    row.add_prefix(&gtk::Image::builder().icon_name(icon).pixel_size(20).build());
    row
}

fn fmt_balance(v: f64) -> String {
    if v.abs() < 0.01 {
        "Centro".into()
    } else if v < 0.0 {
        format!("E {:.0}%", -v * 100.0)
    } else {
        format!("D {:.0}%", v * 100.0)
    }
}

pub fn build(ctl: &Rc<Controller>) -> gtk::Widget {
    let page = adw::PreferencesPage::new();

    // ------------------------------------------------------------------- bass
    let tone = adw::PreferencesGroup::builder()
        .title("Tonalidade")
        .description("Realces aplicados depois do equalizador")
        .build();
    let bass = expander("Graves potentes", "Reforço de baixas frequências (Virtual Bass)", "audio-speakers-symbolic");
    let bass_gain = SliderRow::new("Intensidade", None, 0.0, BASS_MAX_DB as f64, 0.5, fmt_db);
    let bass_freq =
        SliderRow::new("Frequência de corte", None, BASS_FREQ_RANGE.0 as f64, BASS_FREQ_RANGE.1 as f64, 5.0, fmt_hz);
    bass.add_row(&bass_gain.row);
    bass.add_row(&bass_freq.row);
    tone.add(&bass);

    // --------------------------------------------------------------- clarity
    let clarity = expander("Clareza", "Brilho e definição nos agudos (Audio Brilliant)", "mchose-sparkle-symbolic");
    let clar_gain = SliderRow::new("Intensidade", None, 0.0, CLARITY_MAX_DB as f64, 0.5, fmt_db);
    let clar_freq =
        SliderRow::new("A partir de", None, CLARITY_FREQ_RANGE.0 as f64, CLARITY_FREQ_RANGE.1 as f64, 100.0, fmt_hz);
    clarity.add_row(&clar_gain.row);
    clarity.add_row(&clar_freq.row);
    tone.add(&clarity);
    page.add(&tone);

    // ---------------------------------------------------------------- spatial
    let space = adw::PreferencesGroup::builder()
        .title("Espacialização")
        .description(
            "Fones isolam totalmente os canais; o crossfeed mistura um pouco de cada lado no outro, \
                      como acontece com caixas de som — palco mais natural e menos fadiga em sessões longas.",
        )
        .build();
    let xfeed = expander("Crossfeed", "Palco sonoro à frente, em vez de dentro da cabeça", "audio-headphones-symbolic");
    let xfeed_amt = SliderRow::new("Intensidade", None, 0.0, 1.0, 0.01, fmt_pct);
    xfeed.add_row(&xfeed_amt.row);
    space.add(&xfeed);
    let balance = SliderRow::new("Balanço", Some("Ajuste fino entre esquerdo e direito"), -1.0, 1.0, 0.01, fmt_balance);
    balance.scale.add_mark(0.0, gtk::PositionType::Bottom, None);
    space.add(&balance.row);
    page.add(&space);

    // --------------------------------------------------------------- wiring
    macro_rules! on_scale {
        ($w:expr, |$d:ident, $v:ident| $body:expr) => {{
            let c = ctl.clone();
            $w.scale.connect_value_changed(move |s| {
                if !c.syncing() {
                    let $v = s.value() as f32;
                    c.update_dsp(Change::Effects, |$d| $body);
                }
            });
        }};
    }
    macro_rules! on_enable {
        ($w:expr, |$d:ident, $v:ident| $body:expr) => {{
            let c = ctl.clone();
            $w.connect_enable_expansion_notify(move |r| {
                if !c.syncing() {
                    let $v = r.enables_expansion();
                    c.update_dsp(Change::Effects, |$d| $body);
                }
            });
        }};
    }
    on_enable!(bass, |d, v| d.bass.enabled = v);
    on_scale!(bass_gain, |d, v| d.bass.gain_db = v);
    on_scale!(bass_freq, |d, v| d.bass.freq = v);
    on_enable!(clarity, |d, v| d.clarity.enabled = v);
    on_scale!(clar_gain, |d, v| d.clarity.gain_db = v);
    on_scale!(clar_freq, |d, v| d.clarity.freq = v);
    on_enable!(xfeed, |d, v| d.crossfeed.enabled = v);
    on_scale!(xfeed_amt, |d, v| d.crossfeed.amount = v);
    on_scale!(balance, |d, v| d.balance = v);

    let refresh = {
        let ctl = ctl.clone();
        move |change: Change| {
            if !matches!(change, Change::Effects | Change::Eq | Change::Dsp) {
                return;
            }
            let d = ctl.dsp();
            bass.set_enable_expansion(d.bass.enabled);
            bass_gain.set(d.bass.gain_db as f64);
            bass_freq.set(d.bass.freq as f64);
            clarity.set_enable_expansion(d.clarity.enabled);
            clar_gain.set(d.clarity.gain_db as f64);
            clar_freq.set(d.clarity.freq as f64);
            xfeed.set_enable_expansion(d.crossfeed.enabled);
            xfeed_amt.set(d.crossfeed.amount as f64);
            balance.set(d.balance as f64);
        }
    };
    ctl.run_synced(|| {
        refresh(Change::Effects);
    });
    ctl.subscribe(refresh);

    let wrap = gtk::Box::builder().orientation(gtk::Orientation::Vertical).build();
    wrap.append(&dsp_off_banner(ctl));
    page.set_vexpand(true);
    wrap.append(&page);
    wrap.upcast()
}
