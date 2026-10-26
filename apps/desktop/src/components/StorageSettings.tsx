import { useEffect, useRef, useState } from 'react';
import { open } from '@tauri-apps/plugin-dialog';
import { api, type StorageSettings as Settings } from '../api';
import styles from './StorageSettings.module.css';

export function StorageSettings({ onClose }: { onClose: () => void }) {
  const dialog = useRef<HTMLDialogElement>(null);
  const [settings, setSettings] = useState<Settings | null>(null);
  const [path, setPath] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    dialog.current?.showModal();
    let active = true;
    api.getStorageSettings().then((value) => {
      if (active) { setSettings(value); setPath(value.selectedPath); }
    }).catch((reason) => { if (active) setError(String(reason)); });
    return () => { active = false; };
  }, []);

  const browse = async () => {
    try {
      const selected = await open({ directory: true, multiple: false, title: 'Select Catalog Storage Folder' });
      if (typeof selected === 'string') setPath(selected);
    } catch (reason) { setError(String(reason)); }
  };

  const save = async () => {
    setBusy(true);
    setError(null);
    try { setSettings(await api.setStorageLocation(path)); }
    catch (reason) { setError(String(reason)); }
    finally { setBusy(false); }
  };

  return (
    <dialog ref={dialog} className={styles.dialog} aria-labelledby="storage-title" onClose={onClose}>
      <h2 id="storage-title">Catalog Storage</h2>
      <p>Databases and thumbnails are stored here. A local SSD is recommended for fast browsing.
        Choosing a NAS, external disk, or slower drive affects search and preview speed.</p>
      {settings && <>
        <p>Current location: <span className={styles.path}>{settings.activePath}</span></p>
        <label htmlFor="storage-path">New location</label>
        <input id="storage-path" value={path} readOnly className={styles.pathInput} />
        <div className={styles.actions}>
          <button type="button" disabled={busy} onClick={browse}>Choose Folder</button>
          <button type="button" disabled={busy} onClick={() => setPath(settings.defaultPath)}>Use Default</button>
        </div>
        <p>Each library has a separate subfolder with its name and unique ID.
          Location changes take effect after restarting LumiaDAM. Existing catalogs and previews are copied;
          the old copy and source files are preserved. Large catalogs may take time to transfer.</p>
        {settings.restartRequired && <p role="status">New location saved. Restart LumiaDAM to transfer your catalogs.</p>}
        {settings.errors.map((message, index) => <p key={index} role="alert">{message}</p>)}
      </>}
      {error && <p role="alert">{error}</p>}
      <div className={styles.actions}>
        <button type="button" disabled={busy || !settings || path === settings.selectedPath} onClick={save}>
          {busy ? 'Saving…' : 'Save'}
        </button>
        <button type="button" onClick={() => dialog.current?.close()}>Close</button>
      </div>
    </dialog>
  );
}
