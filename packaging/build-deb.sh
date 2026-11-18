#!/usr/bin/env bash
# LiMedal için .deb paketi üretir.
#   bash packaging/build-deb.sh [ikili-yolu]
# Varsayılan ikili: target/release/limedal
set -eu

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
VERSION="$(awk -F'"' '/^version = /{print $2; exit}' "$ROOT/Cargo.toml")"
ARCH="${ARCH:-amd64}"
PKG="limedal"
MAINTAINER="hivuv <hivuv@users.noreply.github.com>"
HOMEPAGE="https://github.com/hivuv/limedal"
BIN="${1:-$ROOT/target/release/$PKG}"
OUT_DIR="${OUT_DIR:-$ROOT/target/deb}"
STAGE="$OUT_DIR/${PKG}_${VERSION}_${ARCH}"
DEB="$OUT_DIR/${PKG}_${VERSION}_${ARCH}.deb"

if [ ! -x "$BIN" ]; then
  printf 'ikili bulunamadı: %s\nÖnce derleyin: cargo build --release\n' "$BIN" >&2
  exit 1
fi
if ! command -v dpkg-deb >/dev/null 2>&1; then
  printf 'dpkg-deb bulunamadı (dpkg paketini kurun).\n' >&2
  exit 1
fi

rm -rf "$STAGE" "$DEB"
mkdir -p "$STAGE/DEBIAN" \
         "$STAGE/usr/bin" \
         "$STAGE/usr/share/applications" \
         "$STAGE/usr/share/doc/$PKG" \
         "$STAGE/usr/lib/systemd/user"

install -m 755 "$BIN" "$STAGE/usr/bin/$PKG"

for entry in limedal limedal-clip; do
  sed "s|__EXEC__|/usr/bin/$PKG|g" "$ROOT/desktop/$entry.desktop" \
    > "$STAGE/usr/share/applications/$entry.desktop"
done

sed "s|__EXEC__|/usr/bin/$PKG|g" "$ROOT/systemd/$PKG.service" \
  > "$STAGE/usr/lib/systemd/user/$PKG.service"

install -m 644 "$ROOT/README.md" "$STAGE/usr/share/doc/$PKG/README.md"
install -m 644 "$ROOT/LICENSE" "$STAGE/usr/share/doc/$PKG/copyright"

{
  printf '%s (%s) unstable; urgency=medium\n\n' "$PKG" "$VERSION"
  printf '  * Sürüm %s.\n\n' "$VERSION"
  printf ' -- %s  %s\n' "$MAINTAINER" "$(date -R)"
} | gzip -9n > "$STAGE/usr/share/doc/$PKG/changelog.Debian.gz"

SIZE="$(du -sk "$STAGE" | awk '{print $1}')"

cat > "$STAGE/DEBIAN/control" <<EOF
Package: $PKG
Version: $VERSION
Architecture: $ARCH
Maintainer: $MAINTAINER
Installed-Size: $SIZE
Section: video
Priority: optional
Depends: ffmpeg
Recommends: pulseaudio-utils, libnotify-bin
Homepage: $HOMEPAGE
Description: Linux için Medal alternatifi - oyun klip motoru
 Arka planda sürekli oyun kaydı tutar ve bir tuşa basınca son N saniyeyi
 kalıcı bir klibe çevirir. Donanım kodlayıcı desteği (NVENC, VA-API,
 Quick Sync), klip kütüphanesi, global kısayollar ve yerel ağ paylaşımı
 içerir. Kayıt için ffmpeg kullanır; arayüzü egui ile çizilen yerel bir
 Rust uygulamasıdır.
EOF

dpkg-deb --root-owner-group --build "$STAGE" "$DEB"
printf 'paket: %s\n' "$DEB"
