// Aynı köken ve doğru üst pencere dışında hiçbir dosya veya kayıt onayı kabul edilmez.
const { channel, inputs, outputs } = window.LUMIADAM_EDITOR;
let ready = false, opened = false, pendingExport = null;
const extension = name => name.split('.').at(-1)?.toLowerCase();
const send = (message, transfer = []) => parent.postMessage({ channel, ...message }, location.origin, transfer);
window.lumiadamReady = () => { ready = true; send({ type: 'ready' }); };
window.lumiadamNotice = () => send({ type: 'notice', message: 'Choose another source in Libraries. This editor stays linked to the selected library file.' });
window.lumiadamExport = (name, bytes) => {
  if (!opened) throw new Error('Open a library file first.');
  if (pendingExport) throw new Error('Finish or retry the pending LumiaDAM export first.');
  if (!outputs.includes(extension(name))) throw new Error('This output format is not enabled in LumiaDAM.');
  if (!bytes.length || bytes.length > 256 * 1024 * 1024) throw new Error('Export must be between 1 byte and 256 MB.');
  const copy = bytes.slice();
  pendingExport = crypto.randomUUID();
  send({ type: 'export', name, bytes: copy.buffer, id: pendingExport }, [copy.buffer]);
};
window.addEventListener('message', event => {
  if (event.source !== parent || event.origin !== location.origin || event.data?.channel !== channel) return;
  const data = event.data;
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
