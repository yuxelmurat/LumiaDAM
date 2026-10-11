# PhotoCraft web editörü

LumiaDAM, PhotoCraft'ın sabitlenmiş web sürümünü yerel dosyalar olarak paketler.
Çalışırken bir web sitesine görsel yüklenmez. Libraries içinde desteklenen bir görseli seçtikten sonra
LumiaDAM logosunun yanındaki menüden **PhotoEditor** açılır. Aynı menüden **Libraries** görünümüne
dönmek editörü kapatmaz.
İlk sürümde tek editör oturumu ve en fazla 256 MB kaynak/çıktı desteklenir.
Başka kaynak açmak için oturumu kapatıp Libraries üzerinden seçim yapılır; editördeki File > Open
ve dosya sürükleme, kaynak ilişkisinin yanlış dosyaya bağlanmasını önlemek için yönlendirme gösterir.

## Kullanım

- Kaynak biçimleri: JPG/JPEG, PNG, WebP, BMP, TIFF ve `.pcraft`.
- **File > Export** ile düz görüntü, **File > Save As** ile katmanlı PhotoCraft projesi kaydedilir.
- Çıktı kaynak klasörüne `kaynak_edited_<export-uuid>.uzantı` adıyla yazılır. Orijinalin üzerine yazılmaz.
- Kaydedilen dosya kataloğa eklenir; görüntüler mevcut küçük resim kuyruğuna gönderilir. NAS yeniden taranmaz.
- Çıktının metadata alanında `source_asset_id`, `editor` ve `editor_export_id` tutulur.
- Başarısız kayıt editörün üstündeki durum satırında görünür; **Retry saving** aynı çıktıyı yeniden dener.
  Katalog güncellemesi başarısızsa diske yazılmış dosya silinmez. Başarılı onay gelene kadar yeni export engellenir.
- Editörü kapatırken kaydedilmemiş çalışma uyarısı gösterilir. PhotoCraft'ın kendi Save bildirimi
  aktarımın başlamasını gösterir; diskte kayıt için LumiaDAM üst durum satırındaki **Saved** sonucunu bekleyin.

HDR/EXR/PSD düzenlemesi, HEIF ve Max bağlantısı bu entegrasyonun doğrulanmış kapsamına dahil değildir.
PhotoCraft erken alpha aşamasındadır; kendi menülerindeki tüm işlevler LumiaDAM tarafından doğrulanmış sayılmaz.

## Geliştirme

Node/pnpm ve Rust yanında `wasm32-unknown-unknown` hedefi ve Trunk 0.21.14 gerekir:

```sh
rustup target add wasm32-unknown-unknown
cargo install trunk --version 0.21.14 --locked
pnpm --filter @lumiadam/desktop editor:prepare
```

`vite:dev` ve `vite:build` hazırlığı otomatik çağırır. İlk hazırlık internet ve zaman gerektirir;
sonrakiler kaynak/uyarlama değişmedikçe önbelleği kullanır. Kaynak `.cache/photocraft`, web çıktısı
`apps/desktop/public/photocraft` içindedir; bu üretilen dosyalar Git'e eklenmez.
`PHOTOCRAFT_SOURCE` mevcut bir geliştirme checkout'unu, `TRUNK` Trunk çalıştırılabilir dosyasını seçebilir.
Hazırlama, checkout'un `apps/photocraft-web` dosyalarına uyarlamayı uygular; bunun için ayrı checkout kullanın.
Kurulum paketine Vite'ın public diziniyle web çıktısı ve lisans bildirimleri birlikte girer.

## Upstream güncelleme

`integrations/photocraft/upstream.json` tam commit SHA'sını sabitler. `host.rs` ve `bridge.js`
dosya aktarımını sağlar; PhotoCraft motorunu veya UI'sini yeniden yazmayız. Hazırlama betiği
beklediği kaynak noktalarını bulamazsa hata verir; sessizce uyumsuz paket üretmez.

Güncelleme için pin'i yeni commit'e taşıyın, ayrı checkout ile `editor:prepare` çalıştırın;
bridge testlerini, Rust kayıt testlerini ve gerçek görüntü açma/düzenleme/export kontrolünü tekrarlayın.
Otomatik upstream güncelleme veya kurulu uygulama güncelleyicisi bu değişikliğe dahil değildir.

PhotoCraft: https://github.com/storytold/photocraft — MIT OR Apache-2.0.
Dağıtımda LICENSE-MIT, LICENSE-APACHE, ATTRIBUTION.md ve gömülü varlıkların lisans bildirimleri korunur.
LumiaDAM ve OpenDAM'in mevcut MIT lisansı değişmez.
