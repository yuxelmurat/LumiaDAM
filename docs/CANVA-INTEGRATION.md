# Canva bağlantısı — tasarım, henüz uygulanmadı

Amaç: kullanıcı galeride bir render seçip **Send to Canva** dediğinde görseli kendi Canva hesabına yüklemek, görselle bir tasarım oluşturmak ve Canva editör bağlantısını tarayıcıda açmak. Sonraki aşamada tasarımın çıktısı yeni LumiaDAM varlığı olarak alınabilir. Hiçbir render otomatik olarak yüklenmez.

Bu belge çalışan özellik iddiası değildir. Uygulamaya henüz Canva gönderme düğmesi, OAuth anahtarı veya sunucu eklenmedi.

## Önerilen akış

1. Settings > **Connect Canva** kullanıcının sistem tarayıcısında OAuth yetkilendirmesini başlatır. PKCE ve state doğrulanır.
2. Kullanıcı galeride JPG/PNG gibi desteklenen bir görsel için **Send to Canva** seçer. Dosya adı ve Canva'ya yükleme amacı görünür olur.
3. LumiaDAM'ın güvenilir bağlantı sunucusu dosyayı kullanıcı hesabına binary asset upload job ile yükler; yerel disk/NAS yolu public URL'ye dönüştürülmez.
4. Job başarıyla tamamlandığında dönen asset ID ile Canva create-design endpoint'i çağrılır; renderın oranına veya seçilen sunum biçimine uygun tasarım oluşturulur.
5. Dönen edit URL sistem tarayıcısında açılır. Başarısız yükleme/tasarım oluşturma durumu görünürdür; iptal, bağlantı kopması ve hız sınırları yönetilir.
6. Kullanıcı istediğinde tasarım çıktısını LumiaDAM'a alır. Orijinal dosya korunur; yeni dosya/asset kaydedilir.

## Uygulama için gerekenler

Canva Developer Portal'da LumiaDAM için kayıtlı integration, izin kapsamları, HTTPS redirect adresi ve güvenilir bir backend gerekir. Client secret masaüstü EXE'ye veya Git'e konulamaz. Canva'nın güvenlik belgesi desktop/public client ortamını doğrudan desteklemediğini belirtir; OAuth token değişimi secret saklayabilen backend'de yapılmalıdır. Hesap bağlama/token saklama kullanıcıya özel olmalı; bağlantıyı kaldırma işlemi sunucudaki yetki/token kaydını temizlemelidir.

Tüm LumiaDAM kullanıcılarına açık dağıtım Canva'nın public integration incelemesine bağlıdır. Yalnız Canva ana sayfasını bir iframe veya linkle açmak seçilmiş renderı aktarmış olmaz. İstenen deneyim asset upload + tasarım oluşturma API'lerini gerektirir.

## Resmî kaynaklar

Canva MCP ayrıca bir alternatif sunar: AI uygulamaları kullanıcı başına OAuth ile bağlanır.
Client secret gerektirmeyen CIMD yolu bulunur, ancak yeni uygulamanın redirect adresi için Canva allowlist/bekleme listesi onayı gerekir.
Bu yolun LumiaDAM'a uygunluğu ayrı doğrulanmalıdır; burada önerilen klasik **Send to Canva** düğmesi tasarımı REST/Connect API yolunu temel alır.

- [Canva Connect APIs](https://www.canva.dev/docs/connect/)
- [OAuth authentication](https://www.canva.dev/docs/apps/rest-apis/authentication/)
- [Security recommendations](https://www.canva.dev/docs/apps/rest-apis/security-recommendations/)
- [Create asset upload job](https://www.canva.dev/docs/connect/api-reference/assets/create-asset-upload-job/)
- [Create design](https://www.canva.dev/docs/connect/api-reference/designs/create-design/)
- [Canva MCP access and permissions](https://www.canva.dev/docs/apps/mcp/access/)
