# LumiaDAM geliştirme rehberi

LumiaDAM, [OpenDAM](https://github.com/LawLaw09/OpenDAM) tabanlı bir geliştirme fork'udur. Orijinal MIT lisansını ve katkı bildirimlerini koruyun. Mevcut işlevleri, deneysel kodu ve planlanan özellikleri açıkça ayırın.

## Depo yapısı

| Yol | İçerik |
| --- | --- |
| `apps/desktop/src` | React, TypeScript, Zustand ve CSS Modules arayüzü |
| `apps/desktop/src-tauri` | Tauri 2 / Rust, SQLite, indeksleme ve Windows installer |
| `packages/preview-engine` | Python önizleme işçisi |
| `packages/dcc-bridge` | Deneysel DCC protokol tipleri |
| `plugins/blender` | Deneysel Blender köprüsü |
| `tools/cleanup` | Windows kurulum kalıntıları için yardımcı araç |
| `docs` | Mimari ve marka/uyumluluk notları |

## Windows ortamı

- Node.js 20+ ve pnpm 9+.
- Güncel Rust stable, `x86_64-pc-windows-msvc` toolchain. Eski Rust 1.80 minimumu güncel bağımlılıklarla doğrulanmış değildir.
- Visual Studio C++ Build Tools, Windows SDK ve WebView2 Runtime.
- Python önizlemeleri için Python 3.10+; bazı biçimler ayrıca yerel kütüphaneler gerektirir.

```powershell
git clone https://github.com/yuxelmurat/LumiaDAM.git
cd LumiaDAM
pnpm install --frozen-lockfile
pnpm dev
```

Python işçisi isteğe bağlı olarak şöyle hazırlanır:

```powershell
python -m venv packages/preview-engine/venv
.\packages\preview-engine\venv\Scripts\python.exe -m pip install -r packages/preview-engine/requirements.txt

# Eski ortam değişkeni adları uyumluluk için korunmuştur.
$env:OPENDAM_PYTHON = (Resolve-Path packages/preview-engine/venv/Scripts/python.exe).Path
$env:OPENDAM_WORKER = (Resolve-Path packages/preview-engine/worker.py).Path
pnpm dev
```

İşçi bu konumları otomatik de arar. OpenImageIO/Assimp gibi bağımlılıklar platforma göre ek kurulum gerektirebilir. Installer henüz bu ortamı kendi içinde dağıtmaz.

## Doğrulama

Komutları depo kökünde çalıştırın:

```powershell
pnpm --filter @lumiadam/desktop vite:build
cargo clippy --manifest-path apps/desktop/src-tauri/Cargo.toml --all-targets -- -D warnings
ruff check packages/preview-engine/
pnpm build
```

CI, ön yüz derlemesi, Rust Clippy ve Python Ruff kontrolü içerir. Kök `pnpm test` komutu mevcut test paketi bulunduğunun kanıtı değildir; yapılandırılmış kapsamlı bir test paketi yoktur. Başarısız kontrolü geçti diye raporlamayın; kullanılan sürümleri ve ilk ilgili hatayı PR'a yazın.

Tauri derlemesi JavaScript/Rust sürüm uyuşmazlığı bildirirse gerçek sürümleri `pnpm --filter @lumiadam/desktop exec tauri info` ile inceleyin. Sürüm kontrolünü atlamayın. Bu dönüşümde tekrar derlemeyi engelleyen dört paket hizalanmıştır: Rust Tauri 2.12.2 / JavaScript API 2.12.1 (uyumlu 2.12 minor), dialog 2.8.1, fs 2.6.0, shell 2.4.1. JavaScript ve Rust taraflarında sürümler sabitlenmiştir; ileride birlikte güncelleyin. Cargo kilit dosyaları upstream'de takip edilmediğinden Rust çözümlemesi zamanla değişebilir.

Mevcut başlangıç kodunda iki ek kontrol sorunu vardır: kök `pnpm typecheck`, deneysel `packages/dcc-bridge` paketinin `tsconfig.json` dosyası olmadığı için başarısız olur; Python Ruff kontrolü `worker.py` içindeki `UP038` uyarısını bildirir. Masaüstü için yukarıdaki hedefli TypeScript/Vite kontrolünü kullanın. Bu sorunlar marka dönüşümünde işlevsel düzeltme olarak ele alınmamıştır.

## Veri ve marka uyumluluğu

- Uygulama arayüzündeki metinleri ve uygulamanın ürettiği hata mesajlarını İngilizce yazın. Kod açıklamaları ve geliştirme notları Türkçe olabilir.
- Simge veya arayüz değişikliklerinde `build.rs` dosyasındaki `icons` ve `../dist` bağımlılıklarını koruyun; aksi halde eski kaynaklar EXE içinde kalabilir. Windows simgesini yalnız PNG dosyasında değil, derlenmiş EXE'den çıkararak da kontrol edin.

- Eski `.opendam` kataloglarının aktarım uyumluluğunu koruyun. Yeni katalogları kaynak arşive yazmayın; kaynak yollarını ve yerel önizleme yollarını ayırın. [Depolama düzeni](docs/storage.md).
- Kütüphane SQLite bağlantılarındaki rollback journal yaklaşımını marka dönüşümünde değiştirmeyin.
- `OPENDAM_PYTHON`, `OPENDAM_WORKER`, `application/opendam-asset-id` ve Blender operator kimlikleri geriye uyumluluk yüzeyleridir.
- Yeni uygulama kimliği OpenDAM'den ayrı ayarlar oluşturur. Otomatik veri taşıma yoktur.
- Silme ve temizlik işlemlerini gerçek arşivde test etmeyin. Temizlik aracı için yalnızca `--dry-run --silent` kullanın; OpenDAM klasörlerini hedeflemediğini doğrulayın.
- Logo dosyaları ve üretim komutları [marka notlarında](docs/branding.md) açıklanır.

## Katkı akışı

Ayrı feature branch açın, değişikliği küçük tutun, kontrolleri çalıştırın ve [LumiaDAM Pull Requests](https://github.com/yuxelmurat/LumiaDAM/pulls) üzerinden `main` dalına PR hazırlayın. Issues/Discussions şu anda kapalıdır. Yeni özellik ve performans iddialarını ölçüm veya doğrulama olmadan tamamlanmış gibi yazmayın.
