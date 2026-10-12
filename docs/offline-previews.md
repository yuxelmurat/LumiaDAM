# Çevrimdışı önizlemeler ve Windows desteği

Katalog SQLite dosyasında, üretilmiş küçük resimler aynı yerel katalog klasörünün `thumbs/<asset>/<version-job>/` dizininde saklanır. Kart, liste, ilgili görsel ve ayrıntı panelinin küçük resmi kaynak NAS dosyasını açmaz. NAS'ın kapanması hazır önizlemeyi geçersiz kılmaz. Ayarlarda katalog konumu ağ diski olarak seçilmişse bu konumun erişilebilir olması gerekir; çevrimdışı kullanım için yerel disk seçilmelidir.

## Üretim ve bağlantı kesilmesi

- Yeni küçük resmi üretmek için kaynak okunmalıdır. Kuyruk kaynak erişimini kontrol eder; kaynağa erişilemiyorsa varlık veya mevcut küçük resim değiştirilmez, iş `queued` olarak tutulur ve kütüphanenin üretimi yaklaşık bir dakika bekler. Diğer kütüphanelerin katalogları kullanılabilir.
- `Status`, bu durumu **Waiting for source · cached previews available** olarak gösterir. Kaynak yeniden erişilebilir olduğunda bekleyen işler devam eder. Tek bir dosyanın silinmesi, kök klasör erişilebilirken tüm kütüphaneyi çevrimdışı saymaz.
- Kaynak denetiminde ve görüntü çözümlemede bloklayıcı iş sayısı sınırlıdır. Zaman aşımından sonra hâlâ işletim sistemini bekleyen bir iş kendi kotasını tutar; sınırsız yeni iş parçacığı oluşturulmaz.
- Eski sürümde hata olarak kalan işler katalog sürümü 2 geçişinde bir kez yeniden denenir. Tekrar başarısız olan codec/dosya hatası kaydedilir; uygulamanın her açılışında sonsuz yeniden deneme yapılmaz.
- Önceki ortak `thumbs/ad_thumb.jpg` düzeninde farklı klasörlerin aynı adlı dosyaları çakışabiliyordu. Bu güvenilmez çıktılar yeniden kullanılmaz. Görünür olduklarında kaynak erişimiyle varlığa özel yeni önizleme hazırlanır. Hiç üretilmemiş, silinmiş veya bu eski düzende geçersiz sayılan önizleme, NAS kapalıyken yoktan oluşturulamaz.

Büyük görseli, videoyu veya editörü açmak hâlâ özgün dosyayı gerektirebilir. Bu değişiklik kaynak arşivin tamamını bilgisayara kopyalamaz; kullanıcı dosyalarını ve mevcut kataloğu sıfırlamaz.

## Windows ve HDR/EXR

Windows'ta `IShellItemImageFactory::GetImage` ve `SIIGBF_THUMBNAILONLY` kullanılır. Explorer'a küçük resim sağlayan kurulu codec/sağlayıcılar kullanılabilir; sıradan dosya ikonu başarılı önizleme sayılmaz. HDR/EXR için önce Windows, ardından mevcut Rust görüntü çözümleyicisi denenir. Diğer native görüntülerde önce Rust çalışır. Ağır/diğer biçimlerde Windows ve mevcut Python üreticisi sırayla denenir. Bilinen formatlar taramada, diğer uzantılar görünür olduğunda veya açık önizleme isteğinde kuyruğa alınır.

Windows sağlayıcısı aynı EXE'nin `--lumiadam-shell-thumbnail` alt sürecinde çalışır. Bu süreç uygulama penceresini veya kullanıcı veritabanını açmaz. Yirmi saniyede bitmeyen alt süreç sonlandırılır; başarılı sonuç PNG olarak katalog önbelleğine yazılır. Native görüntü üretiminde 60 saniye, kaynak denetiminde 8 saniye bekleme sınırı vardır. Python/FFmpeg bağımlılıkları mevcut yedek yol için hâlâ isteğe bağlıdır.

Windows API için zaten Tauri bağımlılık ağacında bulunan `windows` crate'i doğrudan gerekli özelliklerle kullanılır. Ek bir harici küçük resim programı indirilmez. Her EXR kanal düzeni, sıkıştırma biçimi veya üçüncü taraf sağlayıcı garanti edilmez. Dosyanın yalnız Windows'ta genel bir ikonla görünmesi, küçük resim desteği anlamına gelmez.

Microsoft başvurusu: [IShellItemImageFactory::GetImage](https://learn.microsoft.com/en-us/windows/win32/api/shobjidl_core/nf-shobjidl_core-ishellitemimagefactory-getimage).

## Doğrulama

```powershell
# apps/desktop
npm test
npx tsc -b
# apps/desktop/src-tauri
cargo test --lib
cargo test --test windows_thumbnail_process
cargo clippy --lib --tests -- -D warnings
```

Testler geçici kaynak klasörünü bağlantı kesilmiş gibi taşıyıp kataloğu yeniden açar; katalog aramasını, JPEG küçük resmini ve ZIP'e bağlı görseli doğrular. Ayrıntı panelinde kaynak NAS adresinin görüntü/video isteği olarak kullanılmadığı denetlenir. Windows sağlayıcısı gerçek JPEG ve bilinmeyen uzantıyla, EXE alt süreç girişi de gerçek PNG çıktısıyla sınanır. Native HDR/EXR testleri sentetik kayan noktalı görüntülerden okunabilir JPEG üretir. Bu testler kullanıcı arşivini değiştirmez; gerçek NAS dosyalarıyla codec kapsamı testi yerine geçmez.
