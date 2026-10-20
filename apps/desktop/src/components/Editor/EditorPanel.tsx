import { forwardRef, useEffect, useImperativeHandle, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { confirm } from '@tauri-apps/plugin-dialog';
import { getCurrentWindow } from '@tauri-apps/api/window';
import type { Asset } from '../../types';
import styles from './EditorPanel.module.css';
import { editors, type EditorId } from './editors';
import { useTranslation, translateMessage } from '../../i18n';

export interface EditorPanelHandle { confirmClose: () => Promise<boolean> }
interface PendingExport { id: string; name: string; bytes: ArrayBuffer }
interface EditorStatus { message: string; values?: Record<string, string | number> }

export const EditorPanel = forwardRef<EditorPanelHandle, { asset: Asset; editor: EditorId; onBusyChange?: (busy: boolean) => void }>(
function EditorPanel({ asset, editor, onBusyChange }, ref) {
  const { t, language } = useTranslation();
  const initialLanguage = useRef(language);
  const currentLanguage = useRef(language);
  currentLanguage.current = language;
  const config = editors[editor];
  const isCadCraft = editor === 'cadcraft';
  const frame = useRef<HTMLIFrameElement>(null);
  const pending = useRef<PendingExport | null>(null);
  const saving = useRef(false);
  const [status, setStatus] = useState<EditorStatus>({ message: 'Loading {engine}…', values: { engine: config.engine } });
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [retry, setRetry] = useState(false);
  const [attempt, setAttempt] = useState(0);
  const [opened, setOpened] = useState(false);

  const send = (data: { type: string; [key: string]: unknown }, transfer: Transferable[] = []) => {
    if (isCadCraft) {
      if (data.type === 'open') frame.current?.contentWindow?.postMessage({ ...data, type: 'cadcraft:open' }, location.origin, transfer);
      return;
    }
    frame.current?.contentWindow?.postMessage({ channel: config.channel, ...data }, location.origin, transfer);
  };
  const allowClose = async () => {
    if (saving.current) return false;
    const approved = await confirm(
    t('Close this editor session? Unsaved changes and exports that have not finished will be lost. Save or export your work and wait for LumiaDAM to confirm it has been saved.'),
    { title: t('Close {engine}', { engine: config.engine }), kind: 'warning', okLabel: t('Close editor'), cancelLabel: t('Keep editing') },
    );
    return approved && !saving.current;
  };

  useImperativeHandle(ref, () => ({ confirmClose: allowClose }));
  useEffect(() => { onBusyChange?.(busy); }, [busy, onBusyChange]);
  // Dil değişirken iframe yenilenmez; açık belgenin kaydedilmemiş düzenlemeleri korunur.
  useEffect(() => { if (!isCadCraft) send({ type: 'language', language }); }, [language, editor, attempt]);

  const save = async (item: PendingExport) => {
    if (saving.current) return;
    saving.current = true;
    setBusy(true);
    setRetry(false);
    setError(null);
    setStatus({ message: 'Saving export and updating the library…' });
    try {
      const extension = item.name.split('.').at(-1)?.toLowerCase() ?? '';
      const result = await invoke<{ filePath: string }>('save_editor_export', item.bytes, { headers: {
        'x-source-asset': asset.id, 'x-export-id': item.id, 'x-export-extension': extension,
        'x-editor-kind': editor,
      } });
      pending.current = null;
      setStatus({ message: 'Saved: {path}', values: { path: result.filePath } });
      if (!isCadCraft) send({ type: 'saved', id: item.id });
    } catch (e) {
      setError(String(e));
      setRetry(true);
      setStatus({ message: 'Export has not finished. Keep this editor open and retry.' });
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
      if (!ready && !disposed) setStatus({ message: '{engine} did not start. Check the local editor bundle and WebGL2/WebGPU support, then retry loading.', values: { engine: config.engine } });
      if (!ready && !disposed) setError('Editor failed to start.');
    }, 60000);
    const receive = async (event: MessageEvent) => {
      if (event.source !== frame.current?.contentWindow || event.origin !== location.origin ||
          (isCadCraft ? typeof event.data?.type !== 'string' || !event.data.type.startsWith('cadcraft:') : event.data?.channel !== config.channel)) return;
      const data = event.data;
      if (isCadCraft) {
        if (data.type === 'cadcraft:failed') {
          clearTimeout(timer);
          setError(typeof data.error === 'string' ? data.error : 'CADCraft failed to start.');
          setStatus({ message: 'CADCraft failed to start.' });
          return;
        }
        if (data.type === 'cadcraft:ready' && !loading) {
          ready = true;
          loading = true;
          clearTimeout(timer);
          setError(null);
          setStatus({ message: 'Opening {name}…', values: { name: asset.fileName } });
          try {
            const bytes = await invoke<ArrayBuffer>('read_editor_asset', { id: asset.id, editor });
            if (!disposed) {
              send({ type: 'open', name: asset.fileName, bytes }, [bytes]);
              setOpened(true);
              setStatus({ message: '' });
            }
          } catch (e) { if (!disposed) { setError(String(e)); setStatus({ message: 'Could not open the source file.' }); } }
          return;
        }
        if (data.type === 'cadcraft:dirty' && typeof data.dirty === 'boolean' && !pending.current) {
          setStatus({ message: data.dirty ? 'CADCraft has unsaved changes.' : '' });
          return;
        }
        if (data.type === 'cadcraft:save' && !pending.current) {
          const extension = typeof data.name === 'string' ? data.name.split('.').at(-1)?.toLowerCase() : undefined;
          if (!['dwg', 'dxf'].includes(extension ?? '')) {
            setError(t('CADCraft can only save DWG or DXF files through LumiaDAM.'));
            setStatus({ message: 'CADCraft export format is not enabled.' });
          } else if (!(data.bytes instanceof ArrayBuffer) || data.bytes.byteLength === 0 || data.bytes.byteLength > 256 * 1024 * 1024) {
            setError(t('CADCraft export must be between 1 byte and 256 MB.'));
            setStatus({ message: 'CADCraft export could not be saved.' });
          } else {
            pending.current = { id: crypto.randomUUID(), name: data.name, bytes: data.bytes };
            await save(pending.current);
          }
        }
        return;
      }
      if (data.type === 'notice' && typeof data.message === 'string' && !pending.current) setStatus({ message: data.message });
      if (data.type === 'ready' && !loading) {
        ready = true;
        loading = true;
        clearTimeout(timer);
        setError(null);
        setStatus({ message: 'Opening {name}…', values: { name: asset.fileName } });
        send({ type: 'language', language: currentLanguage.current });
        try {
          const bytes = await invoke<ArrayBuffer>('read_editor_asset', { id: asset.id, editor });
          if (!disposed) send({ type: 'open', name: asset.fileName, bytes }, [bytes]);
        } catch (e) { if (!disposed) { setError(String(e)); setStatus({ message: 'Could not open the source file.' }); } }
      }
      if (data.type === 'opened') {
        setOpened(true);
        setStatus({ message: '' });
      }
      if (data.type === 'export' && !pending.current && typeof data.id === 'string' && typeof data.name === 'string' && data.bytes instanceof ArrayBuffer && data.bytes.byteLength > 0 && data.bytes.byteLength <= 256 * 1024 * 1024) {
        pending.current = { id: data.id, name: data.name, bytes: data.bytes };
        await save(pending.current);
      }
    };
    window.addEventListener('message', receive);
    return () => { disposed = true; clearTimeout(timer); window.removeEventListener('message', receive); };
  }, [asset.id, editor, attempt]);

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
  }, [language]);

  return <section className={styles.editor} aria-label={t('{engine} editor', { engine: config.engine })}>
    {isCadCraft && <div className={styles.experimentalNotice} role="note">{t(config.hint)}</div>}
    {(error || status.message) && <div className={styles.message} role={error ? 'alert' : 'status'}>
      <span>{error ? `${t(status.message, status.values)} ${translateMessage(error)}` : t(status.message, status.values)}</span>
      {retry && <button disabled={busy} onClick={() => pending.current && void save(pending.current)}>{t('Retry saving')}</button>}
      {error && !opened && !busy && <button onClick={() => { setError(null); setOpened(false); setStatus({ message: 'Loading {engine}…', values: { engine: config.engine } }); setAttempt((n) => n + 1); }}>{t('Retry loading')}</button>}
    </div>}
    <iframe key={attempt} ref={frame} title={config.engine} src={`${config.path}${config.path.includes('?') ? '&' : '?'}lang=${initialLanguage.current}`} className={styles.frame}
      onLoad={() => { if (!isCadCraft) { send({ type: 'language', language: currentLanguage.current }); send({ type: 'ping' }); } }} />
  </section>;
});
