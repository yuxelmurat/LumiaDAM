# LumiaDAM kütüphane performansı uygulama planı

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Aynı arama ve dosya güvenliği davranışıyla katalog sorgularını, önizleme kuyruğunu ve NAS güncellemelerini hızlandırmak.
**Architecture:** Mevcut SQLite, Tokio, notify, React ve Zustand kullanılır. Dosya/klasör sorguları ayrı istekler olur. SQL indeksleri ve tetikleyiciler bütün yazma yollarını kapsar. Görsel ve ağır model önizlemeleri ayrı sınırlı kuyruklarda çalışır; dosya izleyicisi değişen alt ağaçları günceller.
**Tech Stack:** Tauri 2, Rust, SQLx/SQLite FTS5, Tokio, notify 6, React/TypeScript.

## Global Constraints
- Yeni bağımlılık eklenmez. Uygulama metinleri İngilizce; açıklamalar ve kod yorumları Türkçe.
- Kaynak dosya silinmez; tamamlanmamış/başarısız tarama eski kayıtları silmez.
- Aramada kısmi eşleşme, 1–2 karakter, özel SQL karakterleri, tam kök yolu, filtreler ve birden fazla kütüphane korunur.
- Yerel disk kataloglarında WAL; UNC, eşlenmiş ağ sürücüleri ve bilinmeyen konumlarda DELETE journal.
- Mevcut menu dalından codex/library-performance oluşturuldu. PR #6 tabanı korunur.
- Release EXE/installer derlenmez; doğrulama için Rust testleri/check ve TypeScript testleri çalıştırılır.

## 1. SQLite ve arama (search_catalogs, search_index, folders, db, models)
- [x] Gerçek geçici SQLite kataloglarıyla kısmi yol araması, filtreli klasör sonuçları, kısa/özel karakter, indeks geçişi, preview update FTS dokunmaması ve journal politikası testleri ekle; başarısız davranışı doğrula.
- [x] SearchPayload.result_scope: Option<String>, serde resultScope; None eski birleşik davranış, files yalnız dosyalar, folders yalnız klasör sonuçları. Yanıt SearchResult şeklinde kalır.
- [x] FTS5 trigram indeksini ayrı tabloda tut; >=3 karakter aday seçiminde kullan, instr ile son doğruluk kontrolünü koru; kısa sorgularda mevcut karşılaştırmayı kullan. Kök yoldan eşleşen sözcük için tabloyu tarama.
- [x] Doğrudan klasördeki dosyalarda ön ek aralığı indeksini kullan; klasör-altı EXISTS koşullarını aralık sorgusuyla daralt.
- [x] assets_au tetikleyicisini yalnız indekslenen alan değişikliklerinde çalıştır. Yeni trigram tetikleyicileri insert/update/delete ile tutarlı olsun.
- [x] Klasör/sorgu önbelleği sınırlı kapasiteye sahip olsun; SQL revision tetikleyicileri assets, folders, asset_tags, collection_assets ve collections yazmalarında geçersizleştirsin.
- [x] connect_library_sqlite içinde yerel disk politikası ve tüm bağlantılara PRAGMA ayarları. preview::initialize(pool) ve search_index::initialize(pool) çağrıları.
- [x] Geçici, temsilî büyük katalogda önce/sonra arama ve sorgu planı ölçümü; sonuçlar gerçek NAS benchmarkı olarak sunulmaz.
- [x] cargo test --lib ile doğrula; yeni testler ilgili modüllerin içinde olsun.

## 2. Önizleme kuyruğu (preview.rs, commands/preview.rs)
- [x] Önce SQL sayaçlarının insert/status-change/delete doğruluğu; tekrar istek sırasında processing işinin korunması; öncelik sırası ve paralellik sınırı testleri.
- [x] pub async fn initialize(pool: &SqlitePool) -> anyhow::Result<()>: priority sütunu, kuyruk indeksi, trigger ile güncellenen tek satırlı preview_summary(total, completed, pending, failed). Mevcut katalog bir kez sayılır.
- [x] pub fn is_previewable(ext: &str) -> bool ortak destek listesi.
- [x] #[tauri::command] pub async fn prioritize_previews(asset_ids: Vec<String>, state: State<Arc<AppState>>) -> Result<()>: görünür adayları toplu önceliklendir; tamamlanan/işlenmekte olan işleri yeniden oluşturma. asset_ids en fazla 500.
- [x] Native görseller spawn_blocking ve en fazla iki eşzamanlı görev. Ağır Python işi ayrı sırada çalışır; native başarısızlık için Python fallback korunur. İşler atomik sahiplenilir.
- [x] Önizleme tamamlanması jobs ve assets yazılarını tek transaction ile kaydeder. İnvalid eski dosya önizlemesi tekrar kullanılmaz.
- [x] Öncelikli Related image ve kart küçük resimleri yerel cache'ten gelir; kaynak JPG kartta fallback olarak yüklenmez.
- [x] Mevcut HDR/EXR testleri ve ek kuyruk testleri geçer.

## 3. Değişen klasör taraması (watcher.rs)
- [x] Önce geçici dosya sisteminde yeni dosya, değiştirme, rename/delete, başarısız/offline alt ağaç, değişmeyen kayıt korunması ve dış/katalog yolunun reddi testleri.
- [x] WalkDir/metadata işini async runtime üzerinde bloklamadan, sınırlı kanal üzerinden yürüt.
- [x] Tam index_paths taraması korunur. Alt ağaç güncelleme mevcut kayıtların yalnız kapsanan kısmını karşılaştırır; kaynak kökü ulaşılamazsa silme yapmaz.
- [x] pub fn start_library_watchers(state: Arc<AppState>): mevcut notify bağımlılığıyla recursive watch, değişiklikleri debounce/topla, yeni/silinen kütüphanelere uyum sağla. Watch hatası/overflow tam uzlaşım gerektirsin; kaynak erişimi yokken eski katalog korunur.
- [x] Değişen dosyanın eski thumbnail'ini geçersizleştir ve yeniden kuyruğa al; sağlam mevcut önizlemeleri koru.
- [x] library:updated ve library:activity olaylarıyla UI ve durum sorguları güncellensin. Ağ paylaşımlarının kaçırılan olayları için manuel tam Rescan erişilebilir kalır.
- [x] Tam ve hedefli tarama testleri geçer; source dosyaları korunur.

## 4. Arayüz, durum ve entegrasyon (root)
- [x] Gerçek Zustand store testlerinde dosyalar klasör cevabını beklemeden görünür; eski klasör cevapları yeni filtreleri ezmez; klasör hatası dosyaları gizlemez.
- [x] searchAssets({ ...payload, resultScope: 'files' }) sonra bağımsız folders isteği. folderLoading/folderError; adet hazırlanırken sıfır yerine loading göstergesi.
- [x] Görünür kartlar IntersectionObserver ile aday ID'leri toplar ve toplu prioritize_previews çağrısı yapar. Related image kaynak görsel yerine local thumbnail bekler; istekler tekrar tekrar gönderilmez.
- [x] Status preview_summary tablosundan O(1) sayaç okur. Önceki scan bilgilerini tek master sorgusunda alır. UI yenilemeleri çakışmaz; olaylar birleşir; iş sırasında/panel açıkken hızlı, boşta seyrek fallback kullanılır.
- [x] lib.rs yeni initialize/command/watcher akışlarını kaydeder. İşlem süreleri tracing ile ölçülür.
- [x] pnpm --filter @lumiadam/desktop typecheck ve test, cargo test --lib, cargo check, git diff --check; tarayıcıda Files/Folders ve bilgi panelleri kontrolü.
- [x] Bağımsız son kod incelemesi, performans belgesi ve ölçüm çıktıları; taslak PR oluştur ve iliştir. Eski menü PR'si bağımlılık olarak belirtilir.

## Tamamlama kanıtı

51 Rust testi ve açıkça çalıştırılan sentetik benchmark geçti; cargo check ve tüm hedeflerde warnings-as-errors Clippy başarılı. 25 frontend testi ve TypeScript kontrolü ve Vite web paketi başarılı. Pillow 12.3.0 ve FFmpeg bulunan Codex Python ortamında gerçek AVIF/video cache testleri 2/2 geçti; Python Ruff kontrolü geçti. Tarayıcıda geçici fixture ile Files/Folders, kardeş küçük resim ve Activity paneli kontrol edildi; fixture temizlendi.

Bağımsız kod incelemesinde bulunan legacy cache, takılı processing iş, format cache, idle polling ve kısmi tarama tarihi sorunları düzeltildi. Pillow minimum 11.3.0'a yükseltildi; video FFmpeg şartı belgede açıklandı. Uygulama metinleri İngilizce kaldı. Kullanıcının NAS dosyaları üzerinden benchmark veya release EXE/installer derlemesi yapılmadı.

Sonuç codex/library-performance dalından codex/complete-app-menu tabanına taslak PR olarak sunulur. Menü PR #6 bağımlılığı PR açıklamasında belirtilir.
