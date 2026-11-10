//! Prints the systemd user unit generated for the DSP service.
fn main() {
    print!("{}", mchose_core::dsp::pipewire::Service::dsp().render_unit());
}
