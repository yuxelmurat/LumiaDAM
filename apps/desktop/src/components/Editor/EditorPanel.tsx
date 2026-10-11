import { useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { confirm } from '@tauri-apps/plugin-dialog';
import { getCurrentWindow } from '@tauri-apps/api/window';
import type { Asset } from '../../types';
import styles from './EditorPanel.module.css';

export const canEditImage = (asset: Asset | null) => !!asset && ['jpg', 'jpeg', 'png', 'webp', 'bmp', 'tif', 'tiff', 'pcraft'].includes(asset.extension.toLowerCase());
const CHANNEL = 'lumiadam-editor-v1';
interface PendingExport { id: string; name: string; bytes: ArrayBuffer }

export function EditorPanel({ asset, onClose }: { asset: Asset; onClose: () => void }) {
  const frame = useRef<HTMLIFrameElement>(null);
  const pending = useRef<PendingExport | null>(null);
  const saving = useRef(false);
  const [status, setStatus] = useState('Loading PhotoCraft…');
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [retry, setRetry] = useState(false);
  const [attempt, setAttempt] = useState(0);
  const [opened, setOpened] = useState(false);

  const send = (data: object, transfer: Transferable[] = []) => frame.current?.contentWindow?.postMessage({ channel: CHANNEL, ...data }, location.origin, transfer);
  const allowClose = async () => {
    if (saving.current) return false;
    const approved = await confirm(
    'Close this editor session? Unsaved changes and exports that have not finished will be lost. Use File > Save As to keep a layered project, or File > Export to save an image.',
    { title: 'Close PhotoCraft', kind: 'warning', okLabel: 'Close editor', cancelLabel: 'Keep editing' },
    );
    return approved && !saving.current;
  };

  const save = async (item: PendingExport) => {
    if (saving.current) return;
    saving.current = true;
    setBusy(true);
    setRetry(false);
    setError(null);
    setStatus('Saving export and updating the library…');
    try {
      const extension = item.name.split('.').at(-1)?.toLowerCase() ?? '';
      const result = await invoke<{ filePath: string }>('save_editor_export', item.bytes, { headers: {
        'x-source-asset': asset.id, 'x-export-id': item.id, 'x-export-extension': extension,
      } });
      pending.current = null;
      setStatus(`Saved: ${result.filePath}`);
      send({ type: 'saved', id: item.id });
    } catch (e) {
      setError(String(e));
      setRetry(true);
      setStatus('Export has not finished. Keep this editor open and retry.');
    } finally {
      saving.current = false;
      setBusy(false);
    }
  };

  useEffect(() => {
    let disposed = false;
    let loading = false;
    let ready = false;
    const timer = window.setTimeout(() => {
      if (!ready && !disposed) setError('PhotoCraft did not start. Check the local editor bundle and WebGL2/WebGPU support, then retry loading.');
    }, 60000);
    const receive = async (event: MessageEvent) => {
      if (event.source !== frame.current?.contentWindow || event.origin !== location.origin || event.data?.channel !== CHANNEL) return;
      const data = event.data;
      if (data.type === 'notice' && typeof data.message === 'string' && !pending.current) setStatus(data.message);
      if (data.type === 'ready' && !loading) {
        ready = true;
        loading = true;
        clearTimeout(timer);
        setError(null);
        setStatus(`Opening ${asset.fileName}…`);
        try {
          const bytes = await invoke<ArrayBuffer>('read_editor_asset', { id: asset.id });
          if (!disposed) send({ type: 'open', name: asset.fileName, bytes }, [bytes]);
        } catch (e) { if (!disposed) { setError(String(e)); setStatus('Could not open the source image.'); } }
      }
      if (data.type === 'opened') {
        setOpened(true);
        setStatus('Edit locally. File > Export saves a new image beside the original; Save As keeps a layered project.');
      }
      if (data.type === 'export' && !pending.current && typeof data.id === 'string' && typeof data.name === 'string' && data.bytes instanceof ArrayBuffer && data.bytes.byteLength > 0 && data.bytes.byteLength <= 256 * 1024 * 1024) {
        pending.current = { id: data.id, name: data.name, bytes: data.bytes };
        await save(pending.current);
      }
    };
    window.addEventListener('message', receive);
    return () => { disposed = true; clearTimeout(timer); window.removeEventListener('message', receive); };
  }, [asset.id, attempt]);

  useEffect(() => {
    let prompting = false;
    const beforeUnload = (event: BeforeUnloadEvent) => { event.preventDefault(); event.returnValue = ''; };
    window.addEventListener('beforeunload', beforeUnload);
    const unlisten = getCurrentWindow().onCloseRequested(async (event) => {
      event.preventDefault();
      if (prompting || saving.current) return;
      prompting = true;
      try { if (await allowClose()) await getCurrentWindow().destroy(); }
      finally { prompting = false; }
    });
    return () => { window.removeEventListener('beforeunload', beforeUnload); unlisten.then((fn) => fn()).catch(console.error); };
  }, []);

  return <section className={styles.editor} aria-label="PhotoCraft image editor">
    <div className={styles.bar}>
      <strong>PhotoCraft</strong><span title={asset.filePath}>{asset.fileName}</span>
      <span>Local editing · Original preserved</span>
      <button disabled={busy} onClick={async () => { if (await allowClose()) onClose(); }}>Close editor</button>
    </div>
    <div className={styles.message} role={error ? 'alert' : 'status'}>
      {error || status}
      {retry && <button disabled={busy} onClick={() => pending.current && void save(pending.current)}>Retry saving</button>}
      {error && !opened && !busy && <button onClick={() => { setError(null); setStatus('Loading PhotoCraft…'); setAttempt((n) => n + 1); }}>Retry loading</button>}
    </div>
    <iframe key={attempt} ref={frame} title="PhotoCraft" src="/photocraft/index.html" className={styles.frame}
      onLoad={() => send({ type: 'ping' })} />
  </section>;
}
