# LumiaDAM marka ve uyumluluk notları

## Uygulama kimliği

| Alan | LumiaDAM değeri |
| --- | --- |
| Görünen ad / pencere başlığı | `LumiaDAM` |
| Tauri identifier | `io.github.yuxelmurat.lumiadam` |
| Windows executable | `lumiadam-desktop.exe` |
| Installer / kısayol | `LumiaDAM_0.1.0_x64-setup.exe` / `LumiaDAM.lnk` |
| Start Menu klasörü | `LumiaDAM` |
| Paketler | `lumiadam`, `@lumiadam/desktop`, `@lumiadam/dcc-bridge` |
| Rust crate / library | `lumiadam-desktop` / `lumiadam_desktop_lib` |

GitHub hesap adına dayanan identifier, sahipliği doğrulanmamış bir alan adı kullanmadan OpenDAM'in `com.opendam.desktop` kimliğinden ayrılır. Ürün adı, executable, kurulum kayıtları ve temizlik hedefleri de ayrılmıştır. Installer OpenDAM sürecini sonlandırmaz. Dağıtım başladıktan sonra identifier sabit tutulmalıdır.

Bu değişiklik OpenDAM üzerine yerinde güncelleme değildir. İşletim sisteminin uygulama veri/ayar klasörü değişir; eski kütüphane listesi ve WebView ayarları otomatik kopyalanmaz. Kullanıcı mevcut kütüphane klasörlerini tekrar ekler. Kütüphane verileri ortak okunabildiği için aynı arşivde eşzamanlı yazma güvenliği sağlandığı iddia edilmez.

## Bilinçli olarak korunan adlar

- `LICENSE` içindeki `OpenDAM Contributors` bildirimi ve upstream bağlantıları.
- `.opendam/library.sqlite`, `.opendam/thumbs/`, kilit dosyaları ve SQL migration dosyaları: veri uyumluluğu ve migration checksum'ları.
- Uygulama veri klasörü içindeki `opendam.sqlite` ve WebView içindeki `opendam-ui`: dış uygulama kimliği zaten ayrıdır; gereksiz veri geçişi eklenmemiştir.
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
