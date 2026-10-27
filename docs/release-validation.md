# PR #1 dağıtım doğrulaması

Son güncelleme: 27 Ekim 2026. Bu kayıt `codex/lumiadam-rebrand` PR'ının güncel `main` ile birleştirilmiş kaynakları içindir. v0.1.1 farklı bir daldan yayımlanmıştır; onun testleri bu PR'a aktarılmaz. Bu PR'ın installer'ı inceleme adayıdır; yeni bir herkese açık sürüm yayımlanmaz.

## Tamamlanan kaynak kontrolleri

| Kontrol | Sonuç |
| --- | --- |
| `pnpm install --frozen-lockfile` | Geçti; mevcut pnpm kilit dosyası değişmedi |
| `pnpm typecheck` | Masaüstü ve DCC protokol paketi geçti |
| `pnpm --filter @lumiadam/dcc-bridge build` | JavaScript ve declaration çıktıları üretildi |
| `pnpm --filter @lumiadam/desktop test` | 16 test geçti; Node ortamında mevcut localStorage uyarıları var |
| `pnpm --filter @lumiadam/desktop vite:build` | PhotoCraft WASM dahil geçti; mevcut büyük chunk/PhotoCraft dead-code uyarıları var |
| `cargo clippy --manifest-path apps/desktop/src-tauri/Cargo.toml --all-targets -- -D warnings` | Geçti |
| `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --lib -- --include-ignored --test-threads=1` | Açıkça seçilmiş NAS paylaşımı ile 27 test geçti |
| `cargo test --manifest-path tools/cleanup/Cargo.toml` | 1 koruma testi geçti |
| Ruff 0.17.0 ile `ruff check packages/preview-engine/` | Geçti; proje aynı sürümü zorunlu tutar |
| Python 3.11.9 / Pillow 10.4.0 ile `python packages/preview-engine/test_worker.py` | Gerçek alt süreçte önizleme üretimi, boyutlandırma, eksik dosya ve bilinmeyen metot yanıtları geçti |
| Blender 4.5.4 LTS ile `--background --factory-startup --python-exit-code 1 --python plugins/blender/test_bridge.py` | Eklenti kaydı, örnek `.blend` içe aktarımı ve kaynak hash koruması geçti |

## Veri güvenliği ve NAS kapsamı

- Geri dönüşüm kutusu hatası kalıcı silmeye dönüştürülmez. Kayıtlar, dosya taşınamadan kaldırılmaz. Kütüphane dışına çıkan yollar, link dosyaları ve link içeren ara klasörler reddedilir. Katalog ilişkileri transaction içinde silinir ve SQL hataları kullanıcıya ulaşır. Dosya zaten geri dönüşüme taşındıktan sonra SQL yazımı başarısız olursa dosya geri dönüşümde, kayıt katalogda kalabilir; dosya sistemi ile SQLite arasında atomik işlem garantisi yoktur.
- Windows'ta UNC/ağ kaynaklarının fiziksel silinmesi reddedilir. İşletim sistemi bu dosyalarda geri dönüşüm garantisi vermediği için dosya ve katalog korunur. `deleteFile=false` ile yalnız katalog kaydı kaldırılabilir.
- Gerçek SMB paylaşımında yalnız testin oluşturduğu benzersiz klasör kullanıldı. Kaynak oluşturma/okuma, yerel katalog, ağda fiziksel silmeyi reddetme, kaynak klasörünün geçici olarak erişilemez olması, yeniden erişim ve katalogdan kaldırma doğrulandı. Mevcut arşiv dosyaları değiştirilmedi.
- Bu test sunucunun ağ bağlantısını kesmez; kablo/SMB oturumu kopması, sunucu yeniden başlatılması, paylaşım izinleri ve birden fazla istemcinin aynı kataloğa yazması için sertifikasyon değildir. Katalog ve önizlemeleri yerel diskte tutun. Ağ kataloğu/eşzamanlı yazma desteği bu kayıtla doğrulanmış sayılmaz.

NAS testini normal CI varsayılan olarak atlar. Yalnız açıkça seçilmiş yazılabilir test paylaşımında çalıştırın:

```powershell
$env:LUMIADAM_TEST_NAS_ROOT = '\\test-nas\test-share'
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --lib nas_source_offline_reconnect_and_delete_protection -- --ignored --test-threads=1
```

## Python ve DCC desteğinin sınırı

Installer `preview-engine/worker.py` ve `requirements.txt` içerir. Python 3.10+ ve biçime özel bağımlılıklar ayrıca sağlanır. İşçi yolu uygulamanın resource dizininden bulunur; özel Python ortamı `OPENDAM_PYTHON` ile seçilebilir. Python yoksa raster/HDR/EXR için mevcut yerel Rust yolları çalışabilir; Python'a ihtiyaç duyan işler başarısız olur ve Activity panelinde nedeni gösterilir.

Blender testi gerçek Blender içinde dosya içe aktarım testidir. Masaüstü uygulaması ile Blender arasında tam komut/yanıt entegrasyonunun kanıtı değildir. Bu dalda DCC protokol tanımları ve deneysel Blender eklentisi vardır; çalışan bir masaüstü bridge sunucusu veya üretim 3ds Max/Revit/SketchUp desteği bu PR kapsamında vaat edilmez. Bunları üretim özelliği olarak sunmadan önce gerçek uygulama-DCC uçtan uca testi gerekir.

## Windows ve yayın kapıları

CI iki ayrı Windows runner kullanır: biri aynı workflow'nun ön yüz artifact'ından NSIS installer üretir; diğeri sadece installer'ı alıp farklı çalışma dizini ve geliştirici araçları çıkarılmış uygulama PATH'iyle kurulum, ilk açılış, yeniden kurulum, kaldırma ve veri korumasını sınar. `windows-installer` ve SHA-256/sonuçları içeren `windows-install-evidence` artifact'ları saklanır. Bu bölümün test sonucu ilgili PR commit'inin CI çalışmasıyla değerlendirilmelidir; script'in varlığı başarılı test sayılmaz.

Yerel `pnpm build` Windows x64 NSIS installer üretti. Bu dosya yayımlanmış release'in yerine konulmadı ve mevcut kullanıcı kurulumunda çalıştırılmadı. CI, Trunk 0.21.14'ün resmi hazır paketini sabit SHA-256 ile doğrulayarak kullanır; her kontrolde aynı derleyiciyi kaynaktan kurmaz.

Bu smoke testi temiz kullanıcı profilli GitHub Windows runner kapsamındadır. WebView2 bulunmayan tamamen temiz Windows 10/11, çevrimdışı önkoşul kurulumu ve farklı yetki düzeylerinin tümü için manuel VM matrisi yerine geçmez. Bu bilgisayarda çalışır temiz Windows VM/Sandbox bulunamadı. Mevcut kullanıcı kurulumuna test amaçlı dokunulmadı.

Taslaktan incelemeye geçiş için güncel birleşimde tüm otomatik kontroller ve installer smoke başarılı olmalı; kalan ortam/özellik sınırları PR açıklamasında açık kalmalıdır. Üretim dağıtımı için vaat edilen özelliklerin gerçek ortam matrisi, imza/SmartScreen politikası ve yayımlanacak installer'ın commit/SHA-256 eşleşmesi ayrıca kabul edilmelidir. Yeşil CI, üretime hazır dağıtım iddiası değildir.
