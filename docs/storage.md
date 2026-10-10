# Katalog depolama

Windows'ta varsayılan düzen:

```text
%LOCALAPPDATA%/LumiaDAM/
  data/opendam.sqlite                 # kütüphane listesi ve ayarlar
  libraries/
    Model_Arşivi--<benzersiz-kimlik>/
      library.sqlite                  # dosya kataloğu, etiketler, koleksiyonlar
      thumbs/                         # küçük resimler ve üretilen önizlemeler
```

Kaynak sürücü/klasör ile katalog depolaması ayrıdır. Aynı adlı kütüphaneler benzersiz kimlikleriyle ayrılır. macOS/Linux'ta işletim sisteminin yerel veri konumu kullanılır.

## Konumu değiştirme

Üst çubuktaki **Depolama → Klasör seç → Kaydet** yolunu kullanın. Seçilen mevcut klasörün yazılabilir olması gerekir. **Varsayılana dön** yerel konumu seçer; ardından Kaydet'e basın. Hızlı arama ve önizleme için yerel SSD önerilir; NAS veya yavaş disk seçmek bu işlemleri yavaşlatabilir.

Değişiklik uygulamayı kapatıp açınca uygulanır. Kayıtlı kataloglar ve önizlemeler yeni konuma kopyalanır. Ana kütüphane listesi/ayar veritabanı yerel `data` klasöründe kalır. Büyük katalogların ilk aktarımı açılışı uzatabilir. Eski konum otomatik silinmez; tekrar aynı hedefe dönülürse mevcut eski kopyanın üzerine yazılmaz, farklı son ekli klasör oluşturulur.

Depolama konumu kaynak arşivin veya mevcut katalog alt klasörünün içinde seçilemez. Tüm sistem sürücüsü kütüphane olarak eklenmişse varsayılan yerel veri konumuna izin verilir; tarayıcı kendi katalog/önizleme klasörlerini atlar.

## Önceki verilerin alınması

- Önceki LumiaDAM'in uygulama verisindeki `opendam.sqlite` ilk açılışta yerel `data` konumuna kopyalanır.
- Önceki arşivlerdeki `.opendam/library.sqlite` ve `thumbs` yerel kütüphane klasörüne alınır. Etiketler, koleksiyon ilişkileri, puanlar ve önizleme yolları korunur. Aktarım için ilk sefer kaynak erişilebilir olmalıdır.
- Eski veritabanı ham dosya kopyasıyla değil, SQLite'ın [VACUUM INTO](https://www.sqlite.org/lang_vacuum.html#vacuum_with_an_into_clause) işlemiyle tutarlı bir anlık görüntü olarak alınır. Önizlemeler ayrıca kopyalanır; aktarım sırasında eski uygulamayı kapatın.
- Hazırlık geçici klasörde yapılır; başarılı kopya etkinleştirilir. Başarısız aktarım kaynak verileri silmez ve var olan kayıp bir katalog yerine boş veritabanı oluşturmaz. Hatalar Depolama ekranında görünür; kaynak bağlantısını/izinleri düzelttikten sonra uygulamayı yeniden açın.
- OpenDAM'in uygulama içindeki kütüphane listesi alınmaz; OpenDAM arşiv klasörlerini LumiaDAM'e yeniden ekleyin. Sonraki etiket/koleksiyon değişiklikleri diğer bilgisayarlar veya eski OpenDAM ile paylaşılmaz.

## Çevrimdışı ve güncelleme davranışı

Yerel katalog ve oluşturulmuş küçük resimler, NAS bağlı değilken de kullanılabilir. Kaynak dosyayı açmak, eksik önizleme üretmek veya dosyayı değiştirmek için arşivin erişilebilir olması gerekir. Küçük resmi henüz oluşmamış bazı görsellerde arayüz kaynak dosyayı okumaya çalışabilir.

Açılışta otomatik tam arşiv taraması ve NAS kataloğu tarihini izleme kaldırıldı. Yeni/değişen dosyalar **Yenile / F5** ile alınır. Eksik veya hatalı tarama, görülmeyen kayıtları silme aşamasına geçmez. Arşiv kaynağına indeks kilidi, veritabanı veya önizleme yazılmaz.

Kataloglar yalnız yeniden üretilebilir önbellek değildir: kullanıcı etiketlerini ve koleksiyonlarını da içerir. Yedekleme için uygulama kapalıyken yerel `data` klasörünü ve seçili depolama klasörünü birlikte kopyalayın. Temizleme aracı SQLite/katalog içeren klasörleri korur. Uygulama içinden kütüphaneyi kaldırmak yerel katalog dosyalarını silmez.

Gerçek NAS/SMB performans ölçümü ve temiz bilgisayarda kurulum testi henüz yapılmadı. Aynı adlı varlıkların küçük resim çakışmaları ayrı geliştirme konusudur.
