//! MCHOSE Control — GTK4 / libadwaita front-end.

mod debug;
mod pages;
mod state;
mod widgets;

use std::path::PathBuf;
use std::rc::Rc;

use adw::prelude::*;
use gtk::{gio, glib};

use state::{Change, Conn, Controller};

const APP_ID: &str = "io.github.JonathanRamosNascimento.MchoseControl";

fn main() -> glib::ExitCode {
    gio::resources_register_include!("mchose.gresource").expect("embedded resources");
    let app = adw::Application::builder().application_id(APP_ID).build();
    app.connect_startup(|_| load_css());
    app.connect_activate(build_window);
    app.set_accels_for_action("app.quit", &["<Control>q"]);
    app.set_accels_for_action("app.refresh", &["F5", "<Control>r"]);
    app.set_accels_for_action("window.close", &["<Control>w"]);
    app.run()
}

/// Our stylesheet only targets app-specific classes. It is installed just above the
/// *user* priority so custom components keep working when a full GTK theme is
/// dropped into `~/.config/gtk-4.0/gtk.css` — colours still come from that theme.
fn load_css() {
    let provider = gtk::CssProvider::new();
    provider.load_from_resource("/io/github/JonathanRamosNascimento/MchoseControl/app.css");
    if let Some(display) = gtk::gdk::Display::default() {
        gtk::style_context_add_provider_for_display(&display, &provider, gtk::STYLE_PROVIDER_PRIORITY_USER + 1);
    }
}

fn build_window(app: &adw::Application) {
    if let Some(w) = app.active_window() {
        w.present();
        return;
    }
    let ctl = Controller::new();
    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("MCHOSE Control")
        .icon_name(APP_ID)
        .default_width(1080)
        .default_height(820)
        .width_request(360)
        .height_request(480)
        .build();

    // ---------------------------------------------------------------- pages
    let stack = adw::ViewStack::new();
    let overview = pages::overview::build(&ctl);
    stack.add_titled_with_icon(&overview.widget, Some("headset"), "Headset", "audio-headset-symbolic");
    stack.add_titled_with_icon(
        &pages::equalizer::build(&ctl, &window),
        Some("eq"),
        "Equalizador",
        "mchose-equalizer-symbolic",
    );
    stack.add_titled_with_icon(&pages::effects::build(&ctl), Some("effects"), "Efeitos", "mchose-sparkle-symbolic");
    stack.add_titled_with_icon(&pages::mic::build(&ctl), Some("mic"), "Microfone", "audio-input-microphone-symbolic");

    // --------------------------------------------------------------- header
    let switcher = adw::ViewSwitcher::builder().stack(&stack).policy(adw::ViewSwitcherPolicy::Wide).build();
    let header = adw::HeaderBar::builder().title_widget(&switcher).build();

    let dsp_btn = {
        let b = gtk::Box::builder().spacing(6).build();
        b.append(&gtk::Image::from_icon_name("mchose-power-symbolic"));
        b.append(&gtk::Label::new(Some("DSP")));
        gtk::ToggleButton::builder()
            .child(&b)
            .css_classes(["dsp-toggle"])
            .tooltip_text("Liga/desliga o equalizador e os efeitos no áudio do headset")
            .build()
    };
    dsp_btn.update_property(&[gtk::accessible::Property::Label("Processamento de áudio")]);
    header.pack_start(&dsp_btn);

    let menu = gio::Menu::new();
    menu.append(Some("Atualizar dispositivo"), Some("app.refresh"));
    let section = gio::Menu::new();
    section.append(Some("Sobre o MCHOSE Control"), Some("app.about"));
    menu.append_section(None, &section);
    let menu_btn = gtk::MenuButton::builder()
        .icon_name("open-menu-symbolic")
        .menu_model(&menu)
        .primary(true)
        .tooltip_text("Menu principal")
        .build();
    header.pack_end(&menu_btn);

    let perm_banner = adw::Banner::builder()
        .title(
            "O receptor foi encontrado, mas o sistema não permite acessá-lo. Permita uma vez para ver bateria e modos.",
        )
        .button_label("Permitir acesso")
        .revealed(false)
        .build();

    let switcher_bar = adw::ViewSwitcherBar::builder().stack(&stack).build();
    let toasts = adw::ToastOverlay::new();
    toasts.set_child(Some(&stack));
    ctl.set_toast_overlay(&toasts);

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.add_top_bar(&perm_banner);
    toolbar.set_content(Some(&toasts));
    toolbar.add_bottom_bar(&switcher_bar);
    window.set_content(Some(&toolbar));

    // ------------------------------------------------------------ breakpoints
    let narrow = adw::Breakpoint::new(adw::BreakpointCondition::parse("max-width: 760sp").expect("valid condition"));
    narrow.add_setter(&overview.columns, "orientation", Some(&gtk::Orientation::Vertical.to_value()));
    narrow.add_setter(&switcher_bar, "reveal", Some(&true.to_value()));
    narrow.add_setter(&header, "title-widget", Some(&None::<gtk::Widget>.to_value()));
    window.add_breakpoint(narrow);

    // ---------------------------------------------------------------- wiring
    {
        let c = ctl.clone();
        dsp_btn.connect_toggled(move |b| {
            if !c.syncing() {
                c.set_dsp_enabled(b.is_active());
            }
        });
        let c = ctl.clone();
        let b = dsp_btn.clone();
        let pb = perm_banner.clone();
        let refresh = move |ch: Change| match ch {
            Change::Dsp => {
                b.set_active(c.dsp_active.get());
                b.set_sensitive(!c.busy.get());
            }
            Change::Device => pb.set_revealed(c.device.borrow().conn == Conn::Denied),
            _ => {}
        };
        ctl.run_synced(|| {
            refresh(Change::Dsp);
        });
        ctl.subscribe(refresh);

        let c = ctl.clone();
        perm_banner.connect_button_clicked(move |_| grant_access(&c));
    }

    // Ctrl+1…4 jump to pages.
    let shortcuts = gtk::ShortcutController::new();
    for (i, name) in ["headset", "eq", "effects", "mic"].into_iter().enumerate() {
        let s = stack.clone();
        let trigger = gtk::ShortcutTrigger::parse_string(&format!("<Control>{}", i + 1)).expect("valid accel");
        let action = gtk::CallbackAction::new(move |_, _| {
            s.set_visible_child_name(name);
            glib::Propagation::Stop
        });
        shortcuts.add_shortcut(gtk::Shortcut::new(Some(trigger), Some(action)));
    }
    window.add_controller(shortcuts);

    install_app_actions(app, &window, &ctl);

    let c = ctl.clone();
    window.connect_close_request(move |_| {
        c.save_now();
        glib::Propagation::Proceed
    });
    debug::apply_window_size(&window);
    window.present();
    debug::maybe_take_screenshots(app, &window, &stack);
}

fn install_app_actions(app: &adw::Application, window: &adw::ApplicationWindow, ctl: &Rc<Controller>) {
    let about = gio::SimpleAction::new("about", None);
    let w = window.clone();
    about.connect_activate(move |_, _| show_about(&w));
    app.add_action(&about);

    let refresh = gio::SimpleAction::new("refresh", None);
    let c = ctl.clone();
    refresh.connect_activate(move |_, _| {
        c.refresh_device();
        c.toast("Atualizando informações do headset…");
    });
    app.add_action(&refresh);

    let quit = gio::SimpleAction::new("quit", None);
    let a = app.clone();
    let c = ctl.clone();
    quit.connect_activate(move |_, _| {
        c.save_now();
        a.quit();
    });
    app.add_action(&quit);
}

fn show_about(window: &adw::ApplicationWindow) {
    let about = adw::AboutDialog::builder()
        .application_name("MCHOSE Control")
        .application_icon(APP_ID)
        .developer_name("Jonathan Ramos")
        .developers(["Jonathan Ramos https://github.com/JonathanRamosNascimento"])
        .version(env!("CARGO_PKG_VERSION"))
        .license_type(gtk::License::Gpl30)
        .comments(
            "Controle no Linux para headsets MCHOSE com receptor C-Media 2.4 GHz (V9 PRO, V9, G9 PRO, S9 PRO).\n\n\
             Bateria, modos de som e firmware via protocolo HID obtido por engenharia reversa do MCHOSE HUB; \
             equalizador, efeitos e microfone limpo processados pelo PipeWire.",
        )
        .build();
    about.add_credit_section(
        Some("Tecnologias"),
        &["GTK 4", "libadwaita", "PipeWire", "WirePlumber", "WebRTC Audio Processing"],
    );
    about.add_legal_section(
        "Marcas",
        None,
        gtk::License::Custom,
        Some(
            "MCHOSE é marca de seus respectivos donos. Este é um projeto independente, sem afiliação com a fabricante.",
        ),
    );
    about.present(Some(window));
}

/// Installs the udev rule through polkit (the user authenticates in the system dialog).
fn grant_access(ctl: &Rc<Controller>) {
    const SCRIPT: &str = r#"install -m 0644 "$1" "$2" && udevadm control --reload-rules && udevadm trigger --subsystem-match=hidraw --action=change"#;
    let c = ctl.clone();
    glib::spawn_future_local(async move {
        let res = gio::spawn_blocking(|| -> Result<(), String> {
            let dir = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
            let tmp = dir.join("mchose-70-mchose.rules");
            std::fs::write(&tmp, mchose_core::UDEV_RULE).map_err(|e| e.to_string())?;
            let status = std::process::Command::new("pkexec")
                .args(["sh", "-c", SCRIPT, "sh"])
                .arg(&tmp)
                .arg(mchose_core::UDEV_RULE_PATH)
                .status();
            let _ = std::fs::remove_file(&tmp);
            match status {
                Ok(s) if s.success() => Ok(()),
                Ok(s) if matches!(s.code(), Some(126) | Some(127)) => Err("autenticação cancelada".into()),
                Ok(s) => Err(format!("pkexec saiu com código {}", s.code().unwrap_or(-1))),
                Err(e) => Err(format!("não foi possível executar o pkexec: {e}")),
            }
        })
        .await;
        match res {
            Ok(Ok(())) => {
                c.toast("Acesso concedido — conectando ao headset…");
                c.refresh_device();
            }
            Ok(Err(e)) => c.toast(&format!("Acesso não concedido: {e}")),
            Err(_) => c.toast("Falha ao conceder acesso"),
        }
    });
}
