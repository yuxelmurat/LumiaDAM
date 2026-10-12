# Kütüphane performansı

Bu değişiklikler `codex/library-performance` dalındadır. İlk açılışta mevcut kataloglara indeks ve sayaç geçişi uygulanır; kaynak arşiv dosyaları değişmez. Büyük katalogda ilk indeks oluşturma normal aramadan uzun sürebilir.

| Alan | Yeni davranış |
| --- | --- |
| Yol araması | Üç veya daha uzun karakterli uygun sözcüklerde FTS5 trigram aday indeksi; kısa/özel/kök sınırındaki aramalarda önceki doğruluk davranışı korunur. |
| Tekrar arama | En fazla 32 yanıt ve yaklaşık 8 MiB önbellek; SQL revision tetikleyicileri katalog ve kullanıcı metadatası değiştiğinde sonuçları geçersizleştirir. |
| Files / Folders | Ayrı sorgular ve hata durumları; dosyalar klasör sayımını beklemez. Klasör sayfası değişince dosyalar yeniden sorgulanmaz. |
| Durum | Önizleme sayaçları tek özet satırdan okunur. Panel kapalı ve işlem yokken 30 saniye, işlem sırasında/panel açıkken 2 saniye. Olaylar birleştirilir, sorgular çakışmaz. |
| Önizleme | İki native görüntü işi ve ayrı tek Python işi; görünür kartlar ve kardeş görseller topluca öne alınır. Kuyruk boşken bildirim beklenir, 5 saniye fallback; boş özet için sahiplenme yazısı yapılmaz. |
| Kartlar | Kaynak JPG/video yerine katalog küçük resmi kullanılır. Related image de yerel küçük resmi bekler. Asıl dosya Quick Look ve editör işlemlerinde açılır. |
| Tarama | İşletim sistemi bildirimleri 750 ms içinde birleştirilir. Değişen alt ağaç karşılaştırılır; değişmeyen dosya/klasör yeniden yazılmaz. Manuel tam Rescan korunur. |
| SQLite | Windows üzerinde bilinen yerel disklerde WAL; UNC, ağ sürücüleri ve bilinmeyen konumlarda DELETE. Beş bağlantının her birinde 4 MiB cache ve MEMORY geçici tablolar. |

Katalog konumu ayarlardan ağ üzerinde seçilirse küçük resimler de o konumdan okunur; varsayılan uygulama veri klasörü yereldir.

## Veri ve durum doğruluğu

Başarısız/offline tarama veya eksik dolaşım eski varlıkları ve klasörleri silmez. Kapsam dışında kalan kardeş klasörler ve diğer kaynak kökleri korunur. Değişen kaynak yeni önizleme işi alır; eski çalışan işin çıktısı yeni kaydı ezmez. Çıktılar varlık/sürüm/iş kimliğine göre ayrı klasörlere yazılır. Eski ortak küçük resimler görünür olduğunda yeniden üretilir.

`Last full scan` yalnız kayıtlı bütün kaynak kökleri tarandığında güncellenir. Alt klasör taramasında tüm arşivin dosya sayısından sahte kalan öğe hesabı yapılmaz. Tam taramanın kalan sayısı da önceki dosya sayısına göre tahmindir; ilk taramada bilinmez.

NAS/SMB cihazının dosya değişikliği bildirimini desteklemesi gerekir. Kaçırılan olaylar için manuel `Rescan all` kullanılır; her NAS'ta otomatik bildirim garantisi yoktur. Kaynak erişimi bulunmasa da mevcut yerel katalog aranabilir.

## Formatlar ve önkoşullar

JPG, PNG, TIFF, GIF, BMP, TGA, WebP, HDR ve EXR mevcut Rust görüntü motorunda işlenir. SVG yerel vektör kopyası olarak gösterilir. AVIF mevcut Python/Pillow görüntü işlemini kullanır; minimum Pillow 11.3.0 gereklidir. Resmî Windows x64 wheel desteği için [Pillow sürüm notları](https://pillow.readthedocs.io/en/stable/releasenotes/11.3.0.html#avif-support-in-wheels) esas alınmıştır. Windows ARM64 wheel desteği bu minimum sürümle garanti edilmez.

Video posterleri mevcut `video_thumb` işlemini kullanır ve FFmpeg'in PATH içinde bulunmasını gerektirir. Ağır modellerin mevcut Python bağımlılıkları korunur. Bu değişiklik Python/FFmpeg'i release içine paketlemez; eksik bağımlılıkta hata durumu görünür, kart kaynak videoyu otomatik açmaz. Yeni bağımlılık eklenmedi; mevcut Pillow minimum sürümü güncellendi.

## Doğrulama

```powershell
# apps/desktop içinde
npm run typecheck
npm test
# apps/desktop/src-tauri içinde
cargo test --lib
cargo check
cargo clippy --all-targets -- -D warnings
cargo test --lib search_index::tests::synthetic_search_benchmark -- --ignored --nocapture
# Repo kökünde; Pillow ve FFmpeg bulunan Python ile
python -m unittest discover -s packages/preview-engine/tests -p test_cached_previews.py -v
```

Yerel sentetik ölçüm ve sınırlar [arama ölçümü](library-search-benchmark.md) belgesindedir. UI, geçici temsili dosyalarla tarayıcıda kontrol edildi: Files varsayılan sekme, klasör sayımı sürerken dosyaların görünmesi, Folders sekmesinde tam yol, ilişkili küçük resmin tamamlanması ve Activity paneli. Gerçek kullanıcı NAS'ında veya kurulu EXE üzerinde performans ölçümü yapılmadı. Release EXE/installer üretilmedi.
