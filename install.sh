#!/usr/bin/env bash
# MCHOSE Control — instalação local (sem pacote).
#   ./install.sh              compila e instala em ~/.local (+ regra udev com sudo)
#   ./install.sh --uninstall  remove tudo, inclusive os serviços de áudio do usuário
set -euo pipefail
cd "$(dirname "$0")"

PREFIX="${PREFIX:-$HOME/.local}"
RULE_SRC="packaging/udev/70-mchose.rules"
RULE_DST="/etc/udev/rules.d/70-mchose.rules"
APP_ID="io.github.JonathanRamosNascimento.MchoseControl"

refresh_caches() {
    gtk-update-icon-cache -q -f -t "$PREFIX/share/icons/hicolor" 2>/dev/null || true
    update-desktop-database -q "$PREFIX/share/applications" 2>/dev/null || true
}

if [[ "${1:-}" == "--uninstall" ]]; then
    for unit in mchose-dsp.service mchose-mic.service; do
        systemctl --user disable --now "$unit" 2>/dev/null || true
        rm -f "${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user/$unit"
    done
    systemctl --user daemon-reload || true
    rm -f "$PREFIX/bin/mchose" "$PREFIX/bin/mchose-control" \
          "$PREFIX/share/applications/$APP_ID.desktop" \
          "$PREFIX/share/icons/hicolor/scalable/apps/$APP_ID.svg"
    refresh_caches
    if [[ -f "$RULE_DST" ]]; then
        echo "Removendo regra udev (sudo)…"
        sudo rm -f "$RULE_DST" && sudo udevadm control --reload-rules
    fi
    echo "Removido. Suas configurações ficaram em ~/.config/mchose (apague se quiser)."
    exit 0
fi

echo "==> Compilando (release)…"
cargo build --release --locked 2>/dev/null || cargo build --release

echo "==> Instalando em $PREFIX"
install -Dm755 target/release/mchose          "$PREFIX/bin/mchose"
install -Dm755 target/release/mchose-control  "$PREFIX/bin/mchose-control"
install -Dm644 "packaging/$APP_ID.desktop"    "$PREFIX/share/applications/$APP_ID.desktop"
install -Dm644 "crates/mchose-gui/data/icons/scalable/apps/$APP_ID.svg" \
               "$PREFIX/share/icons/hicolor/scalable/apps/$APP_ID.svg"
refresh_caches

if ! cmp -s "$RULE_SRC" "$RULE_DST" 2>/dev/null; then
    echo "==> Instalando regra udev para acesso ao headset (sudo)…"
    sudo install -Dm644 "$RULE_SRC" "$RULE_DST"
    sudo udevadm control --reload-rules
    sudo udevadm trigger --subsystem-match=hidraw --action=change
fi

case ":$PATH:" in *":$PREFIX/bin:"*) ;; *) echo "Aviso: $PREFIX/bin não está no PATH." ;; esac
echo "==> Pronto! Abra \"MCHOSE Control\" no menu de aplicativos ou rode: mchose status"
