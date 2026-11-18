#!/usr/bin/env bash
# LiMedal'i kaldırır. Kliplere ve ayarlara dokunmaz.
set -u

printf '\nLiMedal kaldırılıyor\n--------------------\n'

systemctl --user stop limedal.service 2>/dev/null && printf '  [ok] servis durduruldu\n'
systemctl --user disable limedal.service 2>/dev/null && printf '  [ok] servis devre dışı\n'

rm -f "$HOME/.local/bin/limedal" && printf '  [ok] komut kaldırıldı\n'
rm -f "$HOME/.local/share/applications/limedal.desktop" && printf '  [ok] masaüstü girdisi kaldırıldı\n'
rm -f "$HOME/.config/systemd/user/limedal.service" && printf '  [ok] systemd servisi kaldırıldı\n'

systemctl --user daemon-reload 2>/dev/null

cat <<'EOF'

Kaldırıldı. Şunlar bilerek korundu:
  ~/Videos/LiMedal              klipleriniz
  ~/.config/limedal             ayarlarınız
  ~/.cache/limedal              günlük dosyası

Tamamen silmek isterseniz:
  rm -rf ~/.config/limedal ~/.cache/limedal
EOF
