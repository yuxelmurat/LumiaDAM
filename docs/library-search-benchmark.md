# Katalog arama ölçümü

12 Ekim 2026; Windows üzerinde mevcut SQLx/SQLite sürümü ve Rust test (`debug`) derlemesi. Bu ölçüm yalnız geçici yerel katalog kullanır; gerçek NAS veya kullanıcı kataloğu ölçümü değildir.

Tekrar çalıştırma (`apps/desktop/src-tauri` içinde):

```text
cargo test --lib search_index::tests::synthetic_search_benchmark -- --ignored --nocapture
```

Test 50.000 kayıt üretir. `kitch` sorgusu 100 kayıt döndürür; dosya adları sayısaldır ve eşleşme klasör yolundadır. Eski `instr(lower(root || path), lower(query))` sorgusu ile yeni trigram aday sorgusu aynı katalogda 12 kez ardışık çalıştırılır. Her turda sonuç sayılarının eşitliği doğrulanır. Bunlar ısınmış yerel katalog süreleridir; disk önbelleği temizlenmez.

| İşlem | Süre |
| --- | ---: |
| Geçici veri üretimi (ölçüm dışında) | 1.244 ms |
| Eski arama COUNT sorgusu, medyan | 28,099 ms |
| Yeni trigram COUNT sorgusu, medyan | 0,469 ms |
| Yeni tam ilk sayfa, tek örnek (100 asset + ilişkili görseller) | 12,765 ms |
| Aynı sayfa önbellekten, tek örnek | 0,172 ms |

Bu seçici sorguda COUNT yaklaşık 59,9 kat hızlandı. Bu oran tüm uygulamaya, 1–2 karakter sorgularına, yoğun yazma sırasında önbellek davranışına veya NAS taramasına genellenemez. İlk sayfa ve önbellek süreleri tek örnektir, medyan değildir.

Sorgu planı yeni aramada `SEARCH a USING INTEGER PRIMARY KEY (rowid=?)` ve `assets_path_fts VIRTUAL TABLE INDEX ... MATCH` gösterir. Doğrudan klasör sorgusu `idx_assets_normalized_path (<expr>>? AND <expr><?)` aralığını kullanır.

Doğruluk kontrolleri aynı SQLite katalogdaki eski sorguyla 23 aramayı karşılaştırır: kısmi yol, kök, kök/yol sınırı, kısa sorgu, `%`, `_`, köşeli parantez, tırnak, ters eğik çizgi, Unicode ve birden fazla sözcük. Ayrıca kapsam ayrımı, filtreli klasörler, başka bağlantıdan tag/collection güncellemesinde önbellek geçersizleştirme, eski katalog geçişi, snapshot aktarımı ve önizleme durumunun metin FTS indeksine dokunmaması test edilir.

Önbellek en fazla 32 yanıt ve tahmini 8 MiB serileştirilmiş yanıt + anahtar taşır. Katalog sürümü her sorguda okunur; assets, folders, tags, asset_tags, collections ve collection_assets yazmaları SQL tetikleyicileriyle sürümü artırır. Sorgu sırasında sürüm değişirse sonuç önbelleğe alınmaz.

Son entegrasyon kontrolünde aynı benchmark tekrar geçti: eski COUNT medyan 31,662 ms, trigram 0,477 ms; ilk sayfa 13,097 ms, cached sayfa 0,164 ms. Tekrar çalıştırmaların farklı süreleri normaldir; ilk tablodaki yaklaşık 60 kat fark yalnız bu seçici yerel COUNT sorgusunu anlatır.
