#!/usr/bin/env bash
# LiMedal kurulum betiği — hazır ikiliyi ya da kaynaktan derlenmiş sürümü
# ~/.local altına kurar.
set -u

SRC="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BIN_DIR="$HOME/.local/bin"
APP_DIR="$HOME/.local/share/applications"
UNIT_DIR="$HOME/.config/systemd/user"
TARGET="$BIN_DIR/limedal"
FAILED=0
CARGO=""
BIN_SOURCE=""

ok()   { printf '  [ok] %s\n' "$1"; }
warn() { printf '  [!!] %s\n' "$1"; }
err()  { printf '  [xx] %s\n' "$1"; FAILED=1; }

printf '\nLiMedal kurulumu (Rust)\n------------------------\n'

if command -v ffmpeg >/dev/null 2>&1; then
  ok "ffmpeg $(ffmpeg -version 2>/dev/null | head -n1 | awk '{print $3}')"
else
  err "ffmpeg yok — 'sudo apt install ffmpeg'"
fi
command -v ffprobe >/dev/null 2>&1 && ok "ffprobe" || warn "ffprobe yok (süreler tahmini olur)"
command -v pactl >/dev/null 2>&1 && ok "pactl (ses kaynakları)" \
  || warn "pactl yok — 'sudo apt install pulseaudio-utils'"

if id -nG "$USER" 2>/dev/null | tr ' ' '\n' | grep -qx input; then
  ok "'input' grubundasınız — global kısayollar çalışabilir"
else
  warn "Global kısayol için: sudo usermod -aG input $USER  (sonra oturumu kapat/aç)"
  warn "Alternatif: masaüstü kısayolunu 'limedal clip' komutuna bağlayın"
fi

# Release paketiyle geldiyse derlemeye gerek yok.
if [ -x "$SRC/limedal" ]; then
  ok "hazır ikili bulundu — derleme atlanıyor"
  BIN_SOURCE="$SRC/limedal"
else
  if command -v cargo >/dev/null 2>&1; then
    CARGO="$(command -v cargo)"
  elif [ -x "$HOME/.cargo/bin/cargo" ]; then
    CARGO="$HOME/.cargo/bin/cargo"
  fi

  if [ -n "$CARGO" ]; then
    ok "cargo $("$CARGO" --version 2>/dev/null | awk '{print $2}')"
  else
    err "cargo bulunamadı. Rust kurun:"
    printf '       curl --proto "=https" --tlsv1.2 -sSf https://sh.rustup.rs | sh\n'
    printf '       source "$HOME/.cargo/env"\n'
  fi
fi

if [ "$FAILED" -ne 0 ]; then
  printf '\nKurulum yapılamadı. Eksikleri giderip betiği tekrar çalıştırın.\n\n'
  exit 1
fi

if [ -z "$BIN_SOURCE" ]; then
  printf '\nDerleniyor (ilk seferde crate indirmesi için internet gerekir)…\n\n'
  if ! "$CARGO" build --release --manifest-path "$SRC/Cargo.toml"; then
    printf '\n[xx] derleme başarısız.\n\n'
    exit 1
  fi
  ok "derleme tamamlandı"
  BIN_SOURCE="$SRC/target/release/limedal"
fi

mkdir -p "$BIN_DIR" "$APP_DIR" "$UNIT_DIR" || err "klasörler oluşturulamadı"

if cp "$BIN_SOURCE" "$TARGET"; then
  chmod 755 "$TARGET"
  ok "komut: $TARGET"
else
  err "ikili kopyalanamadı"
fi

for entry in limedal limedal-clip; do
  sed "s|__EXEC__|$TARGET|g" "$SRC/desktop/$entry.desktop" > "$APP_DIR/$entry.desktop" \
    && ok "masaüstü girdisi: $APP_DIR/$entry.desktop" \
    || err "masaüstü girdisi yazılamadı: $entry"
done

cp "$SRC/systemd/limedal.service" "$UNIT_DIR/limedal.service" \
  && ok "systemd servisi: $UNIT_DIR/limedal.service" \
  || err "systemd servisi yazılamadı"

printf '\n'
if [ "$FAILED" -ne 0 ]; then
  printf 'Eksikler var. Yukarıdaki [xx] satırlarını giderip betiği tekrar çalıştırın.\n\n'
  exit 1
fi

cat <<EOF
Kurulum tamam.

Başlatmak için:
  limedal run

Sistem açılışında otomatik başlatmak için:
  systemctl --user daemon-reload
  systemctl --user enable --now limedal.service

Durum / tanı:
  limedal status
  limedal doctor

Klipler:  ~/Videos/LiMedal
Ayarlar:  ~/.config/limedal/config.json
Günlük:   ~/.cache/limedal/limedal.log

EOF
