//! Prints the PipeWire config generated for the current settings.
//!
//! `cargo run -p mchose-core --example render_config [mic]`

use mchose_core::config::AppConfig;
use mchose_core::dsp::pipewire;

fn main() {
    let cfg = AppConfig::load();
    let nodes = pipewire::find_hw_nodes();
    if std::env::args().nth(1).as_deref() == Some("mic") {
        let src = nodes.source.as_ref();
        print!(
            "{}",
            pipewire::render_mic_config(&cfg.mic, src.map(|n| n.name.as_str()), src.map(|n| n.card_name.as_str()))
        );
    } else {
        print!("{}", pipewire::render_dsp_config(&cfg.dsp, nodes.sink.as_ref().map(|n| n.card_name.as_str())));
    }
}
