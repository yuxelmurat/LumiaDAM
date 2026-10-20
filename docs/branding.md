# LumiaDAM marka ve uyumluluk notları

## Uygulama kimliği

| Alan | LumiaDAM değeri |
| --- | --- |
| Görünen ad / pencere başlığı | `LumiaDAM` |
| Tauri identifier | `io.github.yuxelmurat.lumiadam` |
| Windows executable | `lumiadam-desktop.exe` |
| Installer / kısayol | `LumiaDAM_0.1.1_x64-setup.exe` / `LumiaDAM.lnk` |
| Start Menu klasörü | `LumiaDAM` |
| Paketler | `lumiadam`, `@lumiadam/desktop`, `@lumiadam/dcc-bridge` |
| Rust crate / library | `lumiadam-desktop` / `lumiadam_desktop_lib` |

GitHub hesap adına dayanan identifier, sahipliği doğrulanmamış bir alan adı kullanmadan OpenDAM'in `com.opendam.desktop` kimliğinden ayrılır. Ürün adı, executable, kurulum kayıtları ve temizlik hedefleri de ayrılmıştır. Installer OpenDAM sürecini sonlandırmaz. Dağıtım başladıktan sonra identifier sabit tutulmalıdır.

Bu değişiklik OpenDAM üzerine yerinde güncelleme değildir. Eski OpenDAM kütüphane listesi ve WebView ayarları otomatik kopyalanmaz. Kullanıcı mevcut kütüphane klasörlerini tekrar ekler; eski katalog yerel alana kopyalanır. Güncel [depolama ve geçiş davranışı](storage.md) ayrı belgelenmiştir.

## Bilinçli olarak korunan adlar

- `LICENSE` içindeki `OpenDAM Contributors` bildirimi ve upstream bağlantıları.
- `.opendam/library.sqlite` ve `.opendam/thumbs/`: eski katalog aktarımının kaynak biçimi; yeni kataloglar yerel/seçili depolamada tutulur. SQL migration dosyaları checksum uyumluluğu için korunur.
- `opendam.sqlite` dosya adı ve WebView içindeki `opendam-ui`: dış uygulama kimliği ayrıdır. LumiaDAM ana veritabanı artık yerel `LumiaDAM/data` konumuna alınır.
- `OPENDAM_PYTHON` / `OPENDAM_WORKER`: mevcut önizleme ortamları.
- `application/opendam-asset-id`: sürükleme/bırakma üretici ve tüketicisinin ortak protokolü.
- Blender dosya adı ve `opendam.*` / `OPENDAM_*` kimlikleri: kayıtlı kısayollar ve scriptler. Görünen eklenti adı LumiaDAM'dir; orijinal yazar bildirimi korunur.
- `opendam_implementation_plan.md` ve `docs/images/opendam-ui.jpg`: tarihsel upstream belgeleri/görseli; güncel LumiaDAM ekranı gibi sunulmaz.

## Görsel kaynaklar

Başlangıç görseli, kullanıcının önceki sohbetinde üretilen katmanlı mavi/cyan LumiaDAM logosudur. Tarayıcıda görülen raster logo referans alınmıştır; kaynak indirme işlemi sonuç vermediğinden kare amblem Imagegen ile bu görsele sadık kalınarak uyarlanmıştır. Orijinal PNG veya özgün vektör dosyası olduğu iddia edilmez.

- `apps/desktop/public/brand/lumiadam-mark.png`: şeffaf kare amblem; ikon üretim kaynağı. Bu raster PNG düzenlenebilir vektör kaynak değildir.
- `apps/desktop/src-tauri/icons/`: Tauri ile üretilen PNG, Windows ICO ve macOS ICNS varyantları. ICNS bulunması macOS dağıtımının test edildiği anlamına gelmez.

İkonları depo kökünden yeniden üretmek için:

```powershell
pnpm --filter @lumiadam/desktop exec tauri icon public/brand/lumiadam-mark.png --output src-tauri/icons
```

README, ön yüz favicon'u ve araç çubuğu aynı amblemi kullanır. Windows için çok çözünürlüklü `icon.ico` kurulum ve executable kaynaklarına bağlanır.

Imagegen yönergesinin özeti: Önceki görseldeki katlanmış L biçimli mavi/cyan amblemi ve dört eğik karoyu koru; tarayıcı arayüzünü, yazıyı ve arka planı kaldır; ortalanmış, kare ve şeffaf uygulama amblemi üret. Sonuç raster uyarlamadır.

## Derleme kontrolleri

Tauri JavaScript/Rust API paketleri aynı 2.12 minor sürümünü kullanır. JavaScript API 2.12.1, GitHub CI minimum yayın yaşı kuralını korumak için seçilmiştir; Rust 2.12.2 ile uyumludur. Python işinin `src` girdisi önizleme motorunu hedefler; kaynak yolu `args` içine eklenerek depo kökü de yanlışlıkla taranmaz. Kaynaklar: [Tauri sürüm eşleştirmesi](https://v2.tauri.app/develop/updating-dependencies/) ve [Ruff Action kaynak kapsamı](https://github.com/astral-sh/ruff-action#specify-a-different-source-directory).
