# LiMedal

Linux (Debian/Ubuntu ve türevleri) için **Medal.tv alternatifi**: arka planda sürekli
oyun kaydı tutar, bir tuşa bastığında son N saniyeyi kalıcı bir klibe çevirir.

Tamamen **Rust** ile yazılmış tek bir yerel ikili (ELF). Python yok, tarayıcı yok,
Electron yok — arayüz egui ile çizilen gerçek bir pencere. Kodlama tarafında
endüstri standardı `ffmpeg` kullanılır (NVENC / VA-API / Quick Sync desteği için).

---

## Özellikler

**Yakalama**
- Sürekli dönen tampon: son 5–1800 saniye her an hazır, disk şişmez
- Donanım kodlayıcı desteği: NVIDIA NVENC, VA-API (AMD/Intel), Intel Quick Sync
- Kendi kendini test eden kodlayıcı seçimi; donanım çalışmazsa sessizce `libx264`'e düşer
- 30 / 60 / 120 / 144 fps, otomatik veya elle çözünürlük
- Oyun sesi ve mikrofon **ayrı ses kanalı** olarak, ayrı düzeylerle
- Tam kayıt (tamponun yanında bağımsız başlat/durdur)

**Klipler**
- `F9` ile anında klip — oyun içinden, pencere kapalıyken bile (global kısayol)
- Küçük resim ızgarası, favoriler, arama, sıralama, oyun bazlı filtre
- Seçili klipte **film şeridi önizleme** (klipten 5 kare, ffmpeg ile üretilir)
- Yeniden adlandırma, yeniden kodlamadan kırpma, silme
- **Düzenleyici**: film şeridi zaman çizgisi, kare kare gezinme, giriş/çıkış
  işaretleri, döngülü oynatma, sesli önizleme, GIF ve 720p MP4 dışa aktarma
- Animasyonlu arayüz: açılış animasyonu, yumuşak görünüm geçişleri, hover
  efektleri, nabız atan kayıt göstergesi
- Sistem oynatıcısında açma (`xdg-open`), klasörü açma
- Yerel ağda token'lı bağlantıyla paylaşım (minik gömülü HTTP sunucusu)
- Depolama sınırı: eski klipleri temizler, favorilere dokunmaz

**Sistem**
- Oyun algılama: `/proc` üzerinden Steam / Proton / Wine oyunlarını yoldan tanır
- **Hyprland uyumlu**: Wayland'de yerel pencere, `hyprctl` ile monitör çözünürlüğü,
  hazır pencere kuralı ve kısayol önerisi (`limedal hyprland`)
- Global kısayollar: X11 **ve** Wayland'de çalışan ham `/dev/input` dinleyicisi
- Masaüstü bildirimleri, systemd kullanıcı servisi
- CLI ↔ pencere iletişimi unix soket üzerinden (tarayıcı/port yok)

---

## Gereksinimler

| Paket | Ne için | Kurulum |
|---|---|---|
| `cargo` + `rustc` (≥ 1.76) | derleme | `curl https://sh.rustup.rs -sSf \| sh` |
| `ffmpeg` | kayıt ve kodlama | `sudo apt install ffmpeg` |
| `pulseaudio-utils` | ses kaynakları (`pactl`) | `sudo apt install pulseaudio-utils` |
| `libnotify-bin` | bildirimler (isteğe bağlı) | `sudo apt install libnotify-bin` |
| `libgl1-mesa-dri` | pencere çizimi (masaüstünde zaten var) | `sudo apt install libgl1-mesa-dri` |

---

## Kurulum

### Debian / Ubuntu / Mint (`.deb`)

```bash
sudo apt install ./limedal-<sürüm>_amd64.deb
limedal doctor       # her şey yerinde mi?
limedal run          # arayüzü aç
```

`apt` gerekli bağımlılığı (`ffmpeg`) kendisi kurar. Paket şunları yerleştirir:

| Yol | İçerik |
|---|---|
| `/usr/bin/limedal` | ikili |
| `/usr/share/applications/limedal.desktop` | uygulama menüsü girdisi |
| `/usr/share/applications/limedal-clip.desktop` | "Klip kaydet" kısayolu |
| `/usr/lib/systemd/user/limedal.service` | oturum açılışında otomatik başlatma |

Servisi açmak istersen (isteğe bağlı):

```bash
systemctl --user daemon-reload
systemctl --user enable --now limedal.service
```

### Arşiv (kullanıcı dizinine, `sudo` gerekmez)

```bash
mkdir -p limedal && tar -xzf limedal-<sürüm>-linux-x86_64.tar.gz -C limedal
cd limedal
bash install.sh      # hazır ikiliyi ~/.local/bin/limedal olarak kurar
```

### Kaynaktan derleme

```bash
cd /path/to/LiMedal
bash install.sh      # cargo ile derler ve ~/.local/bin altına kurar
```

`install.sh` yalnızca kullanıcı dizinine yazar, `sudo` istemez. Klasörde
`limedal` ikili dosyası varsa derleme atlanır.

Elle derleyip `.deb` üretmek istersen:

```bash
cargo build --release
bash packaging/build-deb.sh      # target/deb/limedal_<sürüm>_amd64.deb
```

---

## Kullanım

| Komut | Açıklama |
|---|---|
| `limedal run` | Arayüz + kayıt motoru (zaten açıksa pencereyi öne getirir) |
| `limedal clip` | Son 30 saniyeyi klip yapar |
| `limedal clip --seconds 60 --title "Güzel an"` | İstediğin uzunluk ve başlıkla |
| `limedal record start` / `stop` / `toggle` | Tam kayıt |
| `limedal status` (`--json`) | Durum özeti |
| `limedal open` | Pencereyi öne getir |
| `limedal stop` | Kapat |
| `limedal doctor` | ffmpeg, kodlayıcı, ses, izin kontrolü |
| `limedal hyprland` | Hyprland pencere kuralı ve kısayol önerisi |
| `limedal paths` | Klasör yolları |
| `limedal config dump` / `set capture.fps=120` | Ayarlar |

### Kısayollar

Varsayılan: **F9** = klip kaydet, **F10** = tam kaydı aç/kapa.
Ayarlar sayfasından `ctrl+shift+k` gibi kombinasyonlar atanabilir. Aynı tuş hem
pencereden hem de global dinleyiciden gelebildiği için 1,2 saniyelik bir tekrar
koruması vardır: çift kayıt oluşmaz.

Global kısayollar için kullanıcının `input` grubunda olması gerekir:

```bash
sudo usermod -aG input $USER
# oturumu kapatıp aç
```

Bu izni vermek istemiyorsan iki yol var:

- Arayüzdeki **Klip kaydet** düğmesi (pencere açıkken).
- Kurulumun eklediği **LiMedal — Klip kaydet** masaüstü girdisi ya da masaüstü
  ortamının klavye kısayollarına bağlayacağın `limedal clip` komutu. İkisi de
  motorun çalışıyor olmasına dayanır: systemd servisi açıksa pencere kapalıyken
  de klip alınır.

---

## Düzenleyici

Kütüphanede bir klibe **çift tıkla** (ya da sağ panelde **Düzenle**) — düzenleyici
açılır. Sol tarafta gerçek video önizlemesi, altında klibin karelerinden üretilen
film şeridi zaman çizgisi bulunur.

| Kısayol | İşlev |
|---|---|
| `Boşluk` | oynat / duraklat |
| `←` `→` | bir kare geri / ileri |
| `Shift` + `←` `→` | bir saniye geri / ileri |
| `I` / `O` | giriş ve çıkış işareti |
| `X` | işaretleri temizle |
| `L` | döngü (giriş–çıkış arasında tekrar) |
| `M` | sesi aç / kapa |
| `Ctrl+S` | seçimi kırp ve kütüphaneye ekle |
| `Esc` | kütüphaneye dön |

Yaptıkları:

- **Kırp**: işaretli aralığı *yeniden kodlamadan* yeni klip olarak kaydeder
- **GIF olarak aktar**: palet tabanlı, titreşimsiz GIF (en fazla 20 saniye)
- **MP4 (720p) aktar**: `~/Videos/LiMedal/exports/` altına h264 + aac
- **Paylaş / Klasörü aç / Favori / Yeniden adlandır / Sil**

Önizleme, ffmpeg'den ham RGBA kare akışı olarak gelir: sürüklerken tek kare
("scrub"), oynatırken `-re` ile gerçek zamanlı akış. `ffplay` kuruluysa ses de
senkron çalar. Kare adımı ≈ 1/30 saniyedir.

---

## Nasıl çalışıyor?

1. `ffmpeg` sürekli çalışır ve videoyu **N saniyelik mp4 segmentlerine** yazar
   (`-f segment -reset_timestamps 1`), her segment sınırında zorunlu keyframe vardır.
2. Bir arka plan işçisi tampon süresinden eski segmentleri siler; disk alanı
   kritik seviyeye inerse kaydı durdurur. Segmentler `~/.cache/limedal/buffer`
   altında **diskte** tutulur (tmpfs değil), böylece uzun tamponlar RAM'i şişirmez.
3. `F9` geldiğinde son N saniyeyi kapsayan segmentler `concat` demuxer'ı ile
   **yeniden kodlanmadan** birleştirilir, sonra tam N saniyeye kırpılır.
4. Klip `~/Videos/LiMedal/<id>/` altına `clip.mp4` + `thumb.jpg` + `meta.json`
   olarak taşınır.
5. Arayüz egui ile çizilir; küçük resimler ve film şeritleri `image` crate'i ile
   çözülüp GPU dokusu olarak gösterilir — arayüz için tarayıcı ya da web sunucusu
   kullanılmaz. Yalnızca isteğe bağlı LAN paylaşımı küçük bir HTTP sunucusu açar.
6. CLI komutları `$XDG_RUNTIME_DIR/limedal/control.sock` üzerinden tek satır JSON
   ile konuşur; bu yüzden `limedal clip` her yerden çalışır.

Kodlayıcı seçilirken 1,2 saniyelik sentetik bir test kodlaması yapılır; donanım
kodlayıcı hata verirse otomatik olarak CPU'ya düşülür ve bu durum arayüzde
"şu an:" satırında ve günlükte görünür.

---

## Ayarlar

`~/.config/limedal/config.json` — arayüzdeki **Ayarlar** sayfasıyla aynı şey.

- `capture.fps` — 30 / 60 / 120 / 144
- `capture.width`, `capture.height` — `null` = ekran çözünürlüğü, elle 720p/1080p/1440p
- `capture.buffer_seconds` — tampon uzunluğu (5–1800)
- `capture.segment_seconds` — segment boyu; küçük = daha isabetli kırpma
- `capture.quality` — `low` / `medium` / `high` / `ultra`
- `capture.encoder` — `auto` / `h264_nvenc` / `h264_vaapi` / `h264_qsv` / `libx264`
- `capture.audio.game_source` — `auto`, `none` veya tam kaynak adı
- `capture.audio.mic_source` — boş = kapalı, `auto` = ilk mikrofon
- `hotkeys.save_clip`, `hotkeys.toggle_record`
- `app.clip_seconds_default` — kısayolun kaydettiği süre
- `app.max_storage_gb` — 0 = sınırsız
- `app.share_enabled`, `app.share_port`

---

## Hyprland ve Wayland

LiMedal penceresi Wayland'de **yerel** olarak açılır (wgpu/winit) ve `app_id`
`limedal` olarak ayarlanır, böylece Hyprland pencere kuralları eşleşir. Monitör
çözünürlüğü `hyprctl monitors` üzerinden okunur — Wayland'de `xrandr` genelde
yanıt vermez, bu yüzden XWayland yakalama boyutu doğru hesaplanır.

`limedal hyprland` komutu şu öneriyi yazdırır (aynısı `contrib/hyprland.conf`):

```ini
windowrulev2 = float, class:^(limedal)$
windowrulev2 = size 1180 780, class:^(limedal)$
windowrulev2 = noblur, class:^(limedal)$
bind = , F9, exec, limedal clip
bind = , F10, exec, limedal record toggle
```

Hyprland kısayolları `limedal clip` çağırdığı için `input` grubuna gerek kalmaz —
motorun arka planda çalışıyor olması yeterlidir (systemd servisi ya da
`exec-once = limedal run`).

### Yakalama sınırı

Ekran yakalama `x11grab` kullanır: X11 oturumunda ve **XWayland** pencerelerinde
çalışır — Proton/Steam oyunlarının neredeyse tamamı XWayland kullanır, dolayısıyla
Hyprland'de oyun kaydı çalışır. Saf Wayland pencereleri (org.freedesktop.portal
PipeWire akışı gerektiren) bu sürümde yakalanamaz. Oyunu Gamescope ile X11
modunda çalıştırmak da bir çözümdür.

---

## Wayland notu

Yakalama `x11grab` kullanır: **X11 oturumunda** ve **Wayland altında XWayland
pencerelerinde** (Proton/Steam oyunlarının çoğu) sorunsuz çalışır. Saf Wayland
pencereleri yakalanamaz — bunun için `xdg-desktop-portal` PipeWire akışı gerekir
ve bu sürümde yok.

Pratik çözümler: giriş ekranında **"Xorg"** oturumunu seç, ya da oyunu Gamescope
ile X11 modunda çalıştır.
---

## Sorun giderme

**"Kayıt durdu" / pencere açılmıyor**
`limedal doctor` ve `~/.cache/limedal/limedal.log`. Arayüzde **Canlı** sayfası
ffmpeg'in son hata satırlarını canlı gösterir.

**Siyah video / boş klip**
Ekran yakalama başarısız oluyor olabilir; `echo $DISPLAY` boşsa servis X oturumunu
görmüyor demektir. `DISPLAY=:0 limedal run` deneyin.

Servisle çalıştırıyorsan oturum ortamını bir kez aktarman gerekir:

```bash
systemctl --user import-environment DISPLAY XAUTHORITY WAYLAND_DISPLAY
systemctl --user restart limedal.service
```

**"Tampon henüz dolmadı"**
Motor yeni başladıysa (uygulama açılışı, ayar kaydı ya da **Yeniden başlat**) ilk
segment yazılanana kadar (`segment_seconds`, varsayılan 2 sn) klip alınamaz.
Birkaç saniye bekleyip tekrar dene. Birleştirme sırasında son segment yarım
kalırsa LiMedal otomatik olarak bir önceki segmentle klibi oluşturur.

**"moov atom not found" / birleştirme hatası**
Bu sürümde giderildi: klip yalnızca sonlandırılmış segmentlerden üretilir.
Hâlâ görürsen `~/.cache/limedal/limedal.log` dosyasını ve `limedal doctor`
çıktısını paylaş.

**Ses yok**
`pactl list short sources` çıktısında `.monitor` ile biten bir kaynak olmalı.
Yoksa PipeWire'da monitör kapalıdır: `wpctl status`.

**Kısayol çalışmıyor**
`limedal doctor` → "kısayol erişimi". Genelde `input` grubu eksiktir.

**Derleme hatası**
`rustc --version` en az 1.76 olmalı. `rustup update` sonrası tekrar deneyin.

---

## Proje düzeni

```
Cargo.toml
packaging/build-deb.sh   .deb paketi üretir
src/
  main.rs         CLI + pencere açılışı
  core.rs         bileşenleri birleştiren çekirdek, arka plan işçileri
  capture.rs      ffmpeg süreç yönetimi, segment tamponu, klip çıkarma
  library.rs      klip kütüphanesi (meta, küçük resim, film şeridi, kırpma)
  ffmpeg.rs       kodlayıcı seçimi, kendini test, komut üretimi
  audio.rs        PulseAudio/PipeWire kaynak çözümleme
  gamedetect.rs   /proc üzerinden oyun algılama
  hotkeys.rs      /dev/input ham klavye dinleyicisi
  ipc.rs          unix soket kontrol kanalı (CLI ↔ pencere)
  share.rs        yerel ağ paylaşım sunucusu
  ui.rs           egui arayüzü, animasyonlar, açılış ekranı
  editor.rs       düzenleyici: zaman çizgisi, işaretler, dışa aktarma
  player.rs       video önizlemesi (ffmpeg kare akışı + scrub)
  anim.rs         yumuşatma eğrileri ve renk geçişleri
  hyprland.rs     Hyprland algılama, monitör çözünürlüğü, pencere kuralları
  config.rs       ayarlar + doğrulama
  util.rs         yollar, loglama, biçimlendirme
systemd/, desktop/  servis ve masaüstü girdisi
```

---

## Kaldırma

```bash
# .deb ile kurduysan:
sudo apt remove limedal

# install.sh ile kurduysan:
bash uninstall.sh
```

Klipler (`~/Videos/LiMedal`) ve ayarlar bilerek korunur.

---

## Lisans

MIT.
