# Klasör araması ve arşiv dosyaları

Arama çubuğuna `kitchen` yazıldığında dosya adı/etiket eşleşmeleri ve tam yol eşleşmeleri birlikte değerlendirilir. Örneğin JPG filtresiyle hem `living/kitchenxxxxx.jpg` hem `kitchen/xxxxx.jpg` bulunur. `kitchen corona` gibi birden çok kelime, dosya adı ve yolun farklı bölümlerinde eşleşebilir. Her kelime eşleşmeli ve dosya seçili filtreleri sağlamalıdır.

Tam yolunda arama metni bulunan klasörler **Folders** bölümünde gösterilir. Filtre varsa klasörde veya alt klasörlerinde bu filtreye uyan en az bir dosya bulunmalıdır. Böylece JPG filtresinde yalnız ZIP içeren klasör gösterilmez; JPG içeren `kitchen/corona` klasörüne ulaşılabilir. Aynı adlı klasörler farklı kütüphanelerde ayrı sonuçlardır. Tür, uzantı, etiket, puan ve diğer filtrelerin koşulları dosya ve klasör aramasında ortaktır.

Klasöre tıklamak yalnız o klasördeki dosyaları ve doğrudan alt klasörleri açar. Seçili filtreler korunur; klasörü bulmak için yazılmış metin temizlenir. Filtre yoksa bütün dosya türleri görünür. **Up one level** üst klasöre çıkar; **Back** önceki klasöre veya arama sonuçlarına döner. **All Libraries** seçimi klasör kapsamını kaldırır. Dosya ve klasör sonuçlarının ayrı sayfa düğmeleri vardır.

Koleksiyon seçiliyken klasöre girildiğinde koleksiyon filtresi ve sol menüdeki seçimi korunur. Boş klasör veya filtre nedeniyle boş kalan görünüm için ayrı mesaj gösterilir.

Örnek gezinme:

```text
kitchen araması
  A:/Max arşivi/2023/3dsky/furniture/kitchen
    corona
      2542154.454545.zip
    vray
```

Tam yol klasör sonuçlarında, gezinme çubuğunda ve dosya kartı/liste görünümünde gösterilir. `3dsky`, `corona` veya `vray` bilgileri klasör yolundan okunabilir; dosyanın gerçek renderer uyumluluğu otomatik doğrulanmaz.

Klasör bilgileri kaynak diskte değil, mevcut yerel kütüphane veritabanındaki `folders` tablosunda tutulur. Arama ve gezinme kaynak diski taramaz. Eski kataloglarda kayıtlı dosya yollarından klasör ağacı ilk açılışta çıkarılır; NAS erişimi gerekmez. Önceki taramada atlanan dosya türleri, uzantısız dosyalar ve boş klasörler **Refresh / F5** ile sonraki başarılı taramada eklenir. Başarısız tarama eski klasör listesini silmez.

Bütün normal dosyalar, bilinmeyen uzantılar ve uzantısız dosyalar dahil kataloglanır; önizleme desteği görünürlüğü belirlemez. LumiaDAM'in kendi veritabanları ve katalog/önizleme klasörleri tarama dışında kalır. Arşiv dosyaları da görünür; içerikleri otomatik çıkartılmaz veya indekslenmez.

Bu geliştirmede yeni kurulum paketi üretilmedi. Mevcut kurulu uygulama, ileride hazırlanacak paketle bu değişiklikleri alacak.
