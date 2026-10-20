// Aynı köken ve doğru üst pencere dışında hiçbir dosya veya kayıt onayı kabul edilmez.
const { channel, inputs, outputs } = window.LUMIADAM_EDITOR;
let ready = false, opened = false, pendingExport = null;
let language = new URLSearchParams(location.search).get('lang') === 'tr' ? 'tr' : 'en';
const text = (en, tr) => language === 'tr' ? tr : en;
window.lumiadamLanguage = () => language;
const extension = name => name.split('.').at(-1)?.toLowerCase();
const send = (message, transfer = []) => parent.postMessage({ channel, ...message }, location.origin, transfer);
window.lumiadamReady = () => { ready = true; send({ type: 'ready' }); };
window.lumiadamNotice = () => send({ type: 'notice', message: text('Choose another source in Libraries. This editor stays linked to the selected library file.', 'Kütüphaneler bölümünden başka bir kaynak seçin. Bu editör seçilen kütüphane dosyasına bağlı kalır.') });
window.lumiadamExport = (name, bytes) => {
  if (!opened) throw new Error(text('Open a library file first.', 'Önce kütüphaneden bir dosya açın.'));
  if (pendingExport) throw new Error(text('Finish or retry the pending LumiaDAM export first.', 'Önce bekleyen LumiaDAM dışa aktarımını tamamlayın veya yeniden deneyin.'));
  if (!outputs.includes(extension(name))) throw new Error(text('This output format is not enabled in LumiaDAM.', 'Bu çıktı biçimi LumiaDAM içinde etkin değil.'));
  if (!bytes.length || bytes.length > 256 * 1024 * 1024) throw new Error(text('Export must be between 1 byte and 256 MB.', 'Dışa aktarım boyutu 1 bayt ile 256 MB arasında olmalıdır.'));
  const copy = bytes.slice();
  pendingExport = crypto.randomUUID();
  send({ type: 'export', name, bytes: copy.buffer, id: pendingExport }, [copy.buffer]);
};
window.addEventListener('message', event => {
  if (event.source !== parent || event.origin !== location.origin || event.data?.channel !== channel) return;
  const data = event.data;
  if (data.type === 'language' && (data.language === 'en' || data.language === 'tr')) {
    language = data.language;
    window.lumiadamRepaint?.();
  }
  if (data.type === 'saved' && data.id === pendingExport) pendingExport = null;
  if (data.type === 'ping' && ready) send({ type: 'ready' });
  if (data.type === 'open' && ready && !opened && typeof data.name === 'string' &&
      inputs.includes(extension(data.name)) && data.bytes instanceof ArrayBuffer &&
      data.bytes.byteLength > 0 && data.bytes.byteLength <= 256 * 1024 * 1024) {
    window.lumiadamOpen(data.name, new Uint8Array(data.bytes));
    opened = true;
    send({ type: 'opened' });
  }
});
