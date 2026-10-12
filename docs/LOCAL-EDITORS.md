# Yerel PDF ve vektör editörleri

LumiaDAM menüsündeki **PDFEditor** ve **VectorEditor**, PdfCraft ve VectorCraft'ın sabit commit'lerini yerel WebAssembly paketleri olarak çalıştırır. Kaynak belgeler bir web servisine yüklenmez. PhotoEditor ile aynı ince uygulama çubuğu, dosya aktarım/kayıt köprüsü ve kapanış uyarısı kullanılır. Aynı anda tek kaynak oturumu tutulur.

## Kullanım

- PDF'ye çift tıklama veya sağ tık > **Open in PDFEditor** ile PDF çalışma alanı açılır.
- SVG, PDF-compatible AI, PDF, `.vectorcraft`/eski `.drawcraft` veya desteklenen raster görsel sağ tık > **Open in VectorEditor** ile açılır.
- PDF ve görsellerde uygun birden fazla editör bulunabilir; sağ tık menüsü seçimi kullanıcıya bırakır. Vektör proje dosyaları ve SVGZ çift tıklamada VectorEditor'a gider. Standart görsellerin çift tıklaması büyük önizlemeyi korur.
- **Libraries** oturumu saklar. Farklı kaynak veya editör seçimi ve **Close editor** mevcut çalışmayı kaybetmeden önce onay ister. Uyumsuz seçim ilgili editörün dosya seçme açıklamasını gösterir; arka plandaki oturum silinmez.
- PdfCraft'ta **Save / Save As**, VectorCraft'ta **Save / Save As / Export** sonucu LumiaDAM'a aktarılır. LumiaDAM'ın **Saved** bildirimini bekleyin; motorun kendi kayıt bildirimi yalnız aktarımı başlatmış olabilir.
- Çıktı `kaynak_edited_<uuid>.uzantı` olarak aynı klasöre yazılır, kataloğa eklenir ve uygun biçimlerde küçük resim kuyruğuna girer. Orijinal değiştirilmez. Yazılmış çıktı katalog hatasında silinmez; **Retry saving** aynı kayıt kimliğiyle işlemi tamamlar.
- Bekleyen bir çıktı onaylanmadan ikinci çıktı kabul edilmez. Bu davranış başarısız yazım sırasında çalışma baytlarının korunmasını sağlar.

## Kapsam

Kaynak ve çıktı başına 256 MB sınırı korunur. PDFEditor kaynak/çıktısı PDF'dir. VectorEditor kaynakları SVG/SVGZ, PDF, PDF-compatible AI, vectorcraft/drawcraft ve JPG/JPEG/PNG/WebP/BMP/TIFF/GIF'tir; çıktılar SVG/SVGZ, PDF, vectorcraft, JPG/JPEG, PNG ve WebP'dir.

AI desteği her Illustrator belgesi anlamına gelmez. Motorlar alpha/geliştirme aşamasındadır; karmaşık tipografi, bazı efektler ve ithal edilen belgeler farklı görünebilir. VectorCraft tanımadığı yazı tipleri için alternatif font kullanır ve düzenleme sırasında bunları pembe işaretleyebilir; özgün tipografi korunmuş gibi sunulmaz. Optional OCR modelleri bu pakete eklenmez. PDF'deki ek dönüştürme/çoklu indirme akışları ve LumiaDAM kayıt köprüsünün dışındaki browser download işlemleri doğrulanmış kapsamda değildir.

Kütüphanedeki kaynakla ilişkiyi korumak için editör içindeki ana **File > Open** ve dosya sürükleme yeni kaynak açmaz; kullanıcı Libraries'e yönlendirilir. VectorCraft'ın **Place** işlemi mevcut çalışma içine öğe yerleştirmek için kullanılabilir.

## Geliştirme ve güncelleme

`pnpm --filter @lumiadam/desktop editor:prepare` üç editörü hazırlar. PhotoCraft'ın mevcut betiği korunur; `scripts/prepare-craft-editors.mjs` PDF ve vektör paketlerini ekler. İlk hazırlama internet, Rust 1.95+, wasm32-unknown-unknown hedefi ve Trunk 0.21.14 ister. Paketten çalışırken bu araçlar gerekmez.

Commit ve bridge sürümleri `integrations/pdfcraft/upstream.json` ve `integrations/vectorcraft/upstream.json` içinde sabitlenir. Kaynaklar `.cache/<motor>/source`, ortak Cargo çıktıları `.cache/craft-target` altında tutulur; büyük üretilmiş web dosyaları Git'e girmez. Geçerli paketler fingerprint ile tekrar kullanılır.

Tek motoru hazırlamak için:
```powershell
node scripts/prepare-craft-editors.mjs pdfcraft
node scripts/prepare-craft-editors.mjs vectorcraft
node scripts/prepare-craft-editors.mjs --source-only
```

Upstream güncellemesi otomatik yayınlanmaz. Önce pin bir feature branch'te değiştirilmeli, adaptör/biçim sınırları incelenmeli, paketler yeniden oluşturulmalı ve açma/kayıt/kapanış testleri çalıştırılmalıdır. Adaptör upstream işaretlerini bulamazsa açık hata ile durur. Eski cache checkout'undaki değişiklikler yeni commit checkout'unu engellerse reset/delete yapılmaz; önce cache'deki değişiklikler incelenmelidir.

## Lisans ve kaynaklar

- [PdfCraft](https://github.com/storytold/pdfcraft): `7163ba3dee8becfff6871c9a65d0d8314314742b`, 0.7.1.
- [VectorCraft](https://github.com/storytold/vectorcraft): `9f0a91b638490ec5a56d4b7b440ec418f74ba93d`, 0.10.0.
- Her iki motor MIT OR Apache-2.0. Kaynak LICENSE, NOTICE, ATTRIBUTION/ASSETS ve gömülü varlık lisansları üretilmiş paketlerde korunur.
- PdfCraft'ın `docs/brand` altındaki özel lisanslı ArtCraft logo görselleri değiştirilmiş sürüme gömülmez. Kaynak kredileri korunur; motorun içinde metin olarak PdfCraft adı gösterilir.
- VectorCraft'ın açık lisanslı uygulama simgesinin kendi lisans bildirimi pakete eklenir.
- LumiaDAM'ın özgün OpenDAM MIT telif/lisans bildirimi korunur.
