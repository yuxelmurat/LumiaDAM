// Bağlantı yalnız aynı uygulama kökenindeki üst pencereye açılır.
let ready = false;
let opened = false;
let pendingExport = null;
const send = (message, transfer = []) => parent.postMessage({ channel: 'lumiadam-editor-v1', ...message }, location.origin, transfer);
window.lumiadamReady = () => { ready = true; send({ type: 'ready' }); };
window.lumiadamNotice = () => send({ type: 'notice', message: 'To edit another image, close this editor and choose an image in Library. This session stays linked to its original file.' });
window.lumiadamExport = (name, bytes) => {
  if (!opened) throw new Error('Open a library image first.');
  if (pendingExport) throw new Error('Finish or retry the pending LumiaDAM export first.');
  if (!['jpg', 'jpeg', 'png', 'webp', 'bmp', 'tif', 'tiff', 'pcraft'].includes(name.split('.').at(-1)?.toLowerCase())) throw new Error('Export as JPG, PNG, WebP, BMP, TIFF or PhotoCraft project.');
  if (!bytes.length || bytes.length > 256 * 1024 * 1024) throw new Error('Export must be between 1 byte and 256 MB.');
  const copy = bytes.slice();
  pendingExport = crypto.randomUUID();
  send({ type: 'export', name, bytes: copy.buffer, id: pendingExport }, [copy.buffer]);
};
window.addEventListener('message', (event) => {
  if (event.source !== parent || event.origin !== location.origin || event.data?.channel !== 'lumiadam-editor-v1') return;
  const data = event.data;
  if (data.type === 'saved' && data.id === pendingExport) pendingExport = null;
  if (data.type === 'ping' && ready) send({ type: 'ready' });
  if (data.type === 'open' && ready && !opened && typeof data.name === 'string' && data.bytes instanceof ArrayBuffer && data.bytes.byteLength <= 256 * 1024 * 1024) {
    opened = true;
    window.lumiadamOpen(data.name, new Uint8Array(data.bytes));
    send({ type: 'opened' });
  }
});
