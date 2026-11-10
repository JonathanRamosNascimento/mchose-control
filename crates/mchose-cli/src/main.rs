use std::process::ExitCode;
use std::time::{Duration, Instant};

use clap::{Parser, Subcommand};
use mchose_core::config::AppConfig;
use mchose_core::device::{Error, Headset};
use mchose_core::dsp::pipewire::{self, Service};
use mchose_core::dsp::settings::{EQ_FREQS, EQ_MAX_DB, EQ_MIN_DB};
use mchose_core::{ChargeState, FirmwareTarget, SoundMode, hidraw, protocol};

const TIMEOUT: Duration = Duration::from_millis(1500);

#[derive(Parser)]
#[command(name = "mchose", version, about = "Controle de headsets MCHOSE (receptor C-Media 291d:385d) no Linux")]
struct Cli {
    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Mostra dispositivo, firmware, bateria, modo e estado do DSP
    Status {
        #[arg(long)]
        json: bool,
    },
    /// Lê a bateria
    Battery,
    /// Lê ou define o modo de som (game1 | game2 | music)
    Mode { mode: Option<String> },
    /// Versões de firmware do headset e do receptor
    Firmware,
    /// Lista os receptores encontrados
    List,
    /// Acompanha notificações do headset em tempo real
    Listen {
        /// Mostra os frames HID brutos
        #[arg(long)]
        raw: bool,
    },
    /// Equalizador
    Eq {
        #[command(subcommand)]
        cmd: EqCmd,
    },
    /// Liga/desliga o processamento de áudio (EQ e efeitos)
    Dsp { state: Option<OnOff> },
    /// Liga/desliga o microfone com redução de ruído
    Mic { state: Option<OnOff> },
    /// Mostra a regra udev necessária e como instalá-la
    Udev,
}

#[derive(Subcommand)]
enum EqCmd {
    /// Lista presets
    List,
    /// Mostra o EQ atual
    Show,
    /// Aplica um preset
    Preset { id: String },
    /// Ajusta uma banda: índice (0-9) ou frequência (ex.: 1k) e ganho em dB
    Set { band: String, db: f32 },
    /// Zera o EQ
    Reset,
}

#[derive(Clone, Copy, clap::ValueEnum)]
enum OnOff {
    On,
    Off,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.cmd.unwrap_or(Cmd::Status { json: false }) {
        Cmd::Status { json } => status(json),
        Cmd::Battery => with_device(|d| {
            print_battery(&d.battery(TIMEOUT)?);
            Ok(())
        }),
        Cmd::Mode { mode: None } => with_device(|d| {
            let (m, raw) = d.mode(TIMEOUT)?;
            match m {
                Some(m) => println!("{m} ({})", m.id()),
                None => println!("desconhecido (0x{raw:02x})"),
            }
            Ok(())
        }),
        Cmd::Mode { mode: Some(s) } => match SoundMode::from_id(&s) {
            None => Err(format!("modo inválido: {s} (use game1, game2 ou music)")),
            Some(m) => with_device(|d| {
                match d.set_mode(m, TIMEOUT)? {
                    Some(now) if now == m => println!("✔ modo definido: {now}"),
                    Some(now) => println!("⚠ headset reporta {now} (pedido: {m})"),
                    None => println!("⚠ modo enviado; headset respondeu valor desconhecido"),
                }
                Ok(())
            }),
        },
        Cmd::Firmware => with_device(|d| {
            for t in [FirmwareTarget::Receiver, FirmwareTarget::Headset] {
                match d.read_firmware(t) {
                    Ok(v) => println!("{:<9} {v}", label_target(t)),
                    Err(e) => println!("{:<9} erro: {e}", label_target(t)),
                }
            }
            Ok(())
        }),
        Cmd::List => {
            let list = hidraw::enumerate();
            if list.is_empty() {
                println!("nenhum receptor encontrado");
            }
            for d in list {
                println!("{}  {} {}  serial={}  {}", d.devnode.display(), d.manufacturer, d.product, d.serial, d.phys);
            }
            Ok(())
        }
        Cmd::Listen { raw } => listen(raw),
        Cmd::Eq { cmd } => eq(cmd),
        Cmd::Dsp { state } => dsp(state),
        Cmd::Mic { state } => mic(state),
        Cmd::Udev => {
            println!("# {}\n{}", mchose_core::UDEV_RULE_PATH, mchose_core::UDEV_RULE.trim_end());
            println!(
                "\nInstalação:\n  mchose udev | sed 1d | sudo tee {} >/dev/null && sudo udevadm control --reload-rules && sudo udevadm trigger --subsystem-match=hidraw --action=change",
                mchose_core::UDEV_RULE_PATH
            );
            Ok(())
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("erro: {e}");
            ExitCode::FAILURE
        }
    }
}

fn label_target(t: FirmwareTarget) -> &'static str {
    match t {
        FirmwareTarget::Headset => "headset",
        FirmwareTarget::Receiver => "receptor",
    }
}

fn with_device(f: impl FnOnce(&Headset) -> Result<(), Error>) -> Result<(), String> {
    let dev = Headset::open_first().map_err(|e| match e {
        Error::PermissionDenied(p) => {
            format!("sem permissão em {} — rode `mchose udev` para instalar a regra", p.display())
        }
        e => e.to_string(),
    })?;
    f(&dev).map_err(|e| e.to_string())
}

fn print_battery(b: &protocol::BatteryStatus) {
    if b.asleep {
        println!("headset desligado / em repouso");
        return;
    }
    let state = match b.charge {
        ChargeState::Charging => "carregando",
        ChargeState::Full => "carga completa",
        ChargeState::Discharging => "em uso",
        ChargeState::Unknown => "status desconhecido",
    };
    let bars = (b.level as usize + 5) / 10;
    println!("{:>3}% [{}{}] {state}", b.level, "█".repeat(bars), "░".repeat(10 - bars));
}

fn status(json: bool) -> Result<(), String> {
    let cfg = AppConfig::load();
    let dsp_active = Service::dsp().is_active();
    let mic_active = Service::mic().is_active();
    let mut out = serde_json::json!({
        "dsp": { "active": dsp_active, "preset": cfg.dsp.preset, "eq": cfg.dsp.eq_gains },
        "mic_processing": mic_active,
    });

    match Headset::open_first() {
        Ok(d) => {
            let info = d.info().clone();
            let rx = d.read_firmware(FirmwareTarget::Receiver).ok();
            let bat = d.battery(TIMEOUT).ok();
            let awake = bat.is_some_and(|b| !b.asleep);
            let hs = if awake { d.read_firmware(FirmwareTarget::Headset).ok() } else { None };
            let mode = if awake { d.mode(TIMEOUT).ok().and_then(|m| m.0) } else { None };
            if json {
                out["device"] = serde_json::to_value(&info).unwrap();
                out["firmware"] = serde_json::json!({
                    "receiver": rx.map(|v| v.to_string()),
                    "headset": hs.map(|v| v.to_string()),
                });
                out["battery"] = serde_json::to_value(bat).unwrap();
                out["mode"] = serde_json::to_value(mode.map(|m| m.id())).unwrap();
            } else {
                println!("● {} {}  ({})", info.manufacturer, info.product, info.devnode.display());
                println!(
                    "  firmware   receptor {}  ·  headset {}",
                    rx.map_or("?".into(), |v| v.to_string()),
                    hs.map_or("?".into(), |v| v.to_string())
                );
                if let Some(v) = rx.filter(|v| v.value() < protocol::MIN_RECEIVER_FW_FOR_FEATURES) {
                    println!("  ⚠ receptor {v}: bateria/modo exigem firmware ≥ 8 (atualize pelo MCHOSE HUB)");
                }
                print!("  bateria    ");
                match bat {
                    Some(b) => print_battery(&b),
                    None => println!("sem resposta"),
                }
                println!("  modo       {}", mode.map_or("—".to_string(), |m| m.to_string()));
            }
        }
        Err(e) => {
            if json {
                out["device"] = serde_json::Value::Null;
                out["error"] = e.to_string().into();
            } else {
                println!("○ {e}");
            }
        }
    }
    if json {
        println!("{}", serde_json::to_string_pretty(&out).unwrap());
    } else {
        let preset = cfg.dsp.find_preset(&cfg.dsp.preset).map_or("personalizado".into(), |p| p.name);
        println!("  DSP        {}  ·  preset: {preset}", if dsp_active { "ativo" } else { "desligado" });
        println!("  microfone  redução de ruído {}", if mic_active { "ativa" } else { "desligada" });
    }
    Ok(())
}

fn listen(raw: bool) -> Result<(), String> {
    with_device(|d| {
        println!("ouvindo {} — Ctrl+C para sair", d.info().devnode.display());
        d.send_battery_query()?;
        d.send_mode_query()?;
        let start = Instant::now();
        loop {
            let t = start.elapsed().as_secs_f32();
            if raw {
                if let Some(f) = d.next_raw(Duration::from_secs(1))? {
                    println!("[{t:8.3}] {:02x?}", &f[..f.len().min(24)]);
                }
            } else if let Some(r) = d.next_report(Duration::from_secs(1))? {
                match r {
                    protocol::InputReport::Battery(b) => {
                        print!("[{t:8.3}] bateria  ");
                        print_battery(&b);
                    }
                    protocol::InputReport::Mode { mode, raw } => {
                        println!("[{t:8.3}] modo     {}", mode.map_or(format!("0x{raw:02x}"), |m| m.to_string()));
                    }
                    protocol::InputReport::Other { report_id, payload } => {
                        println!("[{t:8.3}] report 0x{report_id:02x} {:02x?}", &payload[..payload.len().min(16)]);
                    }
                }
            }
        }
    })
}

fn parse_band(s: &str) -> Option<usize> {
    if let Ok(i) = s.parse::<usize>() {
        if i < EQ_FREQS.len() {
            return Some(i);
        }
    }
    let lower = s.to_ascii_lowercase().replace("hz", "");
    let hz =
        if let Some(k) = lower.strip_suffix('k') { k.parse::<f32>().ok()? * 1000.0 } else { lower.parse().ok()? };
    EQ_FREQS.iter().position(|&f| (f - hz).abs() / f < 0.1)
}

fn apply_dsp(cfg: &AppConfig) -> Result<(), String> {
    cfg.save().map_err(|e| e.to_string())?;
    let svc = Service::dsp();
    let nodes = pipewire::find_hw_nodes();
    let card = nodes.sink.as_ref().map(|n| n.card_name.as_str());
    svc.write_config(&pipewire::render_dsp_config(&cfg.dsp, card)).map_err(|e| e.to_string())?;
    if svc.is_active() {
        pipewire::LiveControl::new(pipewire::DSP_NODE).push(cfg.dsp.controls(pipewire::SAMPLE_RATE));
        std::thread::sleep(Duration::from_millis(200));
    }
    Ok(())
}

fn print_eq(cfg: &AppConfig) {
    let freqs = EQ_FREQS.map(|f| if f >= 1000.0 { format!("{}k", f / 1000.0) } else { format!("{f}") });
    for (f, g) in freqs.iter().zip(cfg.dsp.eq_gains) {
        let n = ((g - EQ_MIN_DB) / (EQ_MAX_DB - EQ_MIN_DB) * 24.0).round() as usize;
        println!("{f:>5} Hz {g:+5.1} dB  {}", "▮".repeat(n.max(1)));
    }
    println!(
        "pré-amp {:+.1} dB{}",
        cfg.dsp.effective_preamp_db(pipewire::SAMPLE_RATE),
        if cfg.dsp.auto_preamp { " (auto)" } else { "" }
    );
}

fn eq(cmd: EqCmd) -> Result<(), String> {
    let mut cfg = AppConfig::load();
    match cmd {
        EqCmd::List => {
            for p in cfg.dsp.all_presets() {
                let mark = if p.id == cfg.dsp.preset { "●" } else { " " };
                println!("{mark} {:<12} {}", p.id, p.name);
            }
            Ok(())
        }
        EqCmd::Show => {
            print_eq(&cfg);
            Ok(())
        }
        EqCmd::Preset { id } => {
            if !cfg.dsp.apply_preset(&id) {
                return Err(format!("preset desconhecido: {id} (veja `mchose eq list`)"));
            }
            apply_dsp(&cfg)?;
            print_eq(&cfg);
            Ok(())
        }
        EqCmd::Set { band, db } => {
            let i = parse_band(&band).ok_or_else(|| format!("banda inválida: {band}"))?;
            cfg.dsp.eq_gains[i] = db.clamp(EQ_MIN_DB, EQ_MAX_DB);
            cfg.dsp.refresh_preset_match();
            apply_dsp(&cfg)?;
            print_eq(&cfg);
            Ok(())
        }
        EqCmd::Reset => {
            cfg.dsp.apply_preset("flat");
            apply_dsp(&cfg)
        }
    }
}

fn dsp(state: Option<OnOff>) -> Result<(), String> {
    let mut cfg = AppConfig::load();
    let svc = Service::dsp();
    match state {
        None => {
            println!("DSP {}", if svc.is_active() { "ativo" } else { "desligado" });
            Ok(())
        }
        Some(OnOff::On) => {
            cfg.dsp.enabled = true;
            apply_dsp(&cfg)?;
            svc.enable_now().map_err(|e| e.to_string())?;
            println!("✔ DSP ativo — o áudio do headset passa pelo equalizador");
            Ok(())
        }
        Some(OnOff::Off) => {
            cfg.dsp.enabled = false;
            cfg.save().map_err(|e| e.to_string())?;
            svc.disable_now().map_err(|e| e.to_string())?;
            println!("✔ DSP desligado");
            Ok(())
        }
    }
}

fn mic(state: Option<OnOff>) -> Result<(), String> {
    let mut cfg = AppConfig::load();
    let svc = Service::mic();
    match state {
        None => {
            println!("microfone limpo {}", if svc.is_active() { "ativo" } else { "desligado" });
            Ok(())
        }
        Some(OnOff::On) => {
            cfg.mic.enabled = true;
            cfg.save().map_err(|e| e.to_string())?;
            let nodes = pipewire::find_hw_nodes();
            let src = nodes.source.as_ref();
            let conf =
                pipewire::render_mic_config(&cfg.mic, src.map(|n| n.name.as_str()), src.map(|n| n.card_name.as_str()));
            let changed = svc.write_config(&conf).map_err(|e| e.to_string())?;
            if changed && svc.is_active() {
                svc.restart().map_err(|e| e.to_string())?;
            } else {
                svc.enable_now().map_err(|e| e.to_string())?;
            }
            println!("✔ microfone com redução de ruído disponível: \"{}\"", pipewire::MIC_NODE);
            Ok(())
        }
        Some(OnOff::Off) => {
            cfg.mic.enabled = false;
            cfg.save().map_err(|e| e.to_string())?;
            svc.disable_now().map_err(|e| e.to_string())?;
            println!("✔ microfone limpo desligado");
            Ok(())
        }
    }
}
