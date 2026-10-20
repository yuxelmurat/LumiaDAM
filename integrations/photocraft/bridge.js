// Bağlantı yalnız aynı uygulama kökenindeki üst pencereye açılır.
let ready = false;
let opened = false;
let pendingExport = null;
let language = new URLSearchParams(location.search).get('lang') === 'tr' ? 'tr' : 'en';
const text = (en, tr) => language === 'tr' ? tr : en;
window.lumiadamLanguage = () => language;
const send = (message, transfer = []) => parent.postMessage({ channel: 'lumiadam-editor-v1', ...message }, location.origin, transfer);
window.lumiadamReady = () => { ready = true; send({ type: 'ready' }); };
window.lumiadamNotice = () => send({ type: 'notice', message: text('To edit another image, close this editor and choose an image in Library. This session stays linked to its original file.', 'Başka bir görseli düzenlemek için bu editörü kapatıp Kütüphane bölümünden bir görsel seçin. Bu oturum ilk açılan dosyaya bağlı kalır.') });
window.lumiadamExport = (name, bytes) => {
  if (!opened) throw new Error(text('Open a library image first.', 'Önce kütüphaneden bir görsel açın.'));
  if (pendingExport) throw new Error(text('Finish or retry the pending LumiaDAM export first.', 'Önce bekleyen LumiaDAM dışa aktarımını tamamlayın veya yeniden deneyin.'));
  if (!['jpg', 'jpeg', 'png', 'webp', 'bmp', 'tif', 'tiff', 'pcraft'].includes(name.split('.').at(-1)?.toLowerCase())) throw new Error(text('Export as JPG, PNG, WebP, BMP, TIFF or PhotoCraft project.', 'JPG, PNG, WebP, BMP, TIFF veya PhotoCraft projesi olarak dışa aktarın.'));
  if (!bytes.length || bytes.length > 256 * 1024 * 1024) throw new Error(text('Export must be between 1 byte and 256 MB.', 'Dışa aktarım boyutu 1 bayt ile 256 MB arasında olmalıdır.'));
  const copy = bytes.slice();
  pendingExport = crypto.randomUUID();
  send({ type: 'export', name, bytes: copy.buffer, id: pendingExport }, [copy.buffer]);
};
window.addEventListener('message', (event) => {
  if (event.source !== parent || event.origin !== location.origin || event.data?.channel !== 'lumiadam-editor-v1') return;
  const data = event.data;
  if (data.type === 'language' && (data.language === 'en' || data.language === 'tr')) {
    language = data.language;
    window.lumiadamRepaint?.();
  }
  if (data.type === 'saved' && data.id === pendingExport) pendingExport = null;
  if (data.type === 'ping' && ready) send({ type: 'ready' });
  if (data.type === 'open' && ready && !opened && typeof data.name === 'string' && data.bytes instanceof ArrayBuffer && data.bytes.byteLength <= 256 * 1024 * 1024) {
    opened = true;
    window.lumiadamOpen(data.name, new Uint8Array(data.bytes));
    send({ type: 'opened' });
  }
});
