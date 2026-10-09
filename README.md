# LumiaDAM

<p align="center"><img src="apps/desktop/public/brand/lumiadam-mark.png" width="160" alt="LumiaDAM mavi/cyan amblemi" /></p>

**3D, mimarlık ve tasarım arşivleri için yerel dijital varlık yöneticisi.**

LumiaDAM, [LawLaw09/OpenDAM](https://github.com/LawLaw09/OpenDAM) projesinden türetilen, geliştirme aşamasındaki bir masaüstü uygulamasıdır. Bu ilk dönüşüm marka, uygulama kimliği ve depo belgelerini düzenler; yeni işlevlerin tamamlandığı anlamına gelmez.

[Kaynak kod](https://github.com/yuxelmurat/LumiaDAM) · [Sürümler](https://github.com/yuxelmurat/LumiaDAM/releases) · [Geliştirme rehberi](CONTRIBUTING.md) · [MIT lisansı](LICENSE)

## Mevcut kapsam

Kaynak kodda bulunan işlevler aşağıdadır. Bunlar tüm biçimlerin veya stüdyo ortamlarının uçtan uca doğrulandığına ilişkin garanti değildir.

| Alan | Mevcut uygulama ve sınırlar |
| --- | --- |
| Kütüphaneler | Klasör ekleme, indeksleme, ızgara/liste görünümü; SQLite tabanlı veriler |
| Organizasyon | Etiketler, puanlar, renkler ve iç içe koleksiyonlar için arayüz ve komutlar |
| Arama | Metin araması ve filtreleme; SQLite FTS5 altyapısı |
| Önizleme | Rust ile raster küçük resimler; Python işçisiyle bazı 3D/CAD dosyalarında gömülü görsel çıkarma veya önizleme denemeleri |
| 3D görünüm | Three.js / GLTF yükleyicisi; her indekslenen 3D biçimi etkileşimli açılamaz |
| Hızlı bakış | Space tuşuyla önizleme ve dosya yolu kopyalama |
| DCC köprüsü | Protokol tanımları ve deneysel Blender eklentisi; üretime hazır 3ds Max/Revit/SketchUp entegrasyonu sunulmaz |

Bir uzantının indekslenmesi; tam geometri okuma, malzeme desteği veya DCC uygulamasına aktarım desteği anlamına gelmez. Python bağımlılıkları ve dosyanın gömülü önizlemesi sonucu etkiler.

## Durum ve bilinen sınırlamalar

- Bu dönüşüm hazırlanırken fork'ta yayımlanmış LumiaDAM sürümü yoktu. Hazır installer veya bağımsız portable paket vaat edilmiyor; güncel durum için [Sürümler](https://github.com/yuxelmurat/LumiaDAM/releases) sayfasına bakın.
- Windows NSIS kurulum hedefi yapılandırılmıştır. Python işçisi ve bütün önizleme bağımlılıkları installer içine henüz paketlenmemiştir.
- Kütüphanelerde `.opendam/library.sqlite` ve `.opendam/thumbs/` biçimi korunur. NAS üzerinde eşzamanlı yazma, kilitleme ve bağlantı kesilmesi ayrıca doğrulanmalıdır. Doğrulanmış performans ölçümü sunulmuyor.
- Mevcut dosya silme kodu, geri dönüşüm kutusu işlemi başarısız olursa kalıcı silmeye geçebilir. Güvenli silme düzeltmesi bu marka çalışmasının kapsamı dışındadır; değerlendirmeyi yedeklenmiş örnek arşivlerle yapın.
- 3D görünümün ortam haritası gibi yardımcı kaynaklar dış ağdan yüklenebilir. Koşulsuz çevrimdışı çalışma iddiasında bulunulmuyor.

## Kaynaktan çalıştırma

Windows için Node.js 20+, pnpm 9+, güncel Rust stable MSVC, Visual Studio C++ Build Tools/Windows SDK ve WebView2 Runtime gerekir. Python önizlemeleri için Python 3.10+ ve ek bağımlılıklar gerekir. Ayrıntılar: [CONTRIBUTING.md](CONTRIBUTING.md).

```powershell
# Depoyu indirip bağımlılıkları kilit dosyasından kurun.
git clone https://github.com/yuxelmurat/LumiaDAM.git
cd LumiaDAM
pnpm install --frozen-lockfile

# Geliştirme penceresini açın.
pnpm dev
```

```powershell
# TypeScript kontrolü ve ön yüz derlemesi.
pnpm --filter @lumiadam/desktop vite:build

# Windows uygulaması ve NSIS installer derlemesi.
pnpm build
```

Başarılı Windows x64 derlemesinin beklenen çıktıları:

- `apps/desktop/src-tauri/target/release/lumiadam-desktop.exe`
- `apps/desktop/src-tauri/target/release/bundle/nsis/LumiaDAM_0.1.0_x64-setup.exe`

Tek EXE'nin bulunması Python önizlemelerini içeren bağımsız portable dağıtım anlamına gelmez.

## OpenDAM'den geçiş

Kimlik `com.opendam.desktop` yerine `io.github.yuxelmurat.lumiadam` olarak ayrıldı. LumiaDAM ayrı kurulum ve uygulama ayarları kullanır; OpenDAM verileri otomatik taşınmaz. Mevcut kütüphane klasörlerini LumiaDAM'e yeniden ekleyebilirsiniz. Aynı kütüphaneyi iki uygulamada eşzamanlı düzenlemekten kaçının. [Kimlik ve uyumluluk notları](docs/branding.md).

## Sonraki geliştirmeler

Güvenli dosya işlemleri, benzersiz önizleme adları, NAS kilitleme testleri, önizleme işçisinin paketlenmesi ve gerçek DCC entegrasyonları sonraki çalışmalar içindir. AI arama, Corona/V-Ray malzeme yönetimi ve proje paketleme mevcut özellik olarak sunulmaz. Eski [OpenDAM uygulama planı](opendam_implementation_plan.md) tarihsel bir tasarım belgesidir.

## Katkı ve lisans

[Katkı rehberini](CONTRIBUTING.md) izleyerek `main` dalına Pull Request açabilirsiniz. Fork'ta Issues ve Discussions şu anda kapalıdır.

LumiaDAM, OpenDAM'in MIT lisanslı kodunu temel alır. **Copyright (c) 2026 OpenDAM Contributors** bildirimi ve [orijinal MIT lisansı](LICENSE) aynen korunmuştur. Marka değişikliği kaynak projenin katkısını ortadan kaldırmaz.
