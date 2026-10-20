import { t as tr, useTranslation, translateMessage, setLanguage, type Language } from '../i18n';
import { useEffect, useRef, useState } from 'react';
import { open } from '@tauri-apps/plugin-dialog';
import { api, type StorageSettings as Settings } from '../api';
import styles from './StorageSettings.module.css';

export function StorageSettings({ onClose }: {
  onClose: () => void;
}) {
  const { language } = useTranslation();
  const dialog = useRef<HTMLDialogElement>(null);
  const [settings, setSettings] = useState<Settings | null>(null);
  const [path, setPath] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [languageSaving, setLanguageSaving] = useState(false);

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
      const selected = await open({ directory: true, multiple: false, title: tr("Select Catalog Storage Folder") });
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
      <h2 id="storage-title">{tr("Settings")}</h2>
      <h3><label htmlFor="app-language">{tr('Language')}</label></h3>
      <p>{tr('Choose the language for LumiaDAM and its editors. Changes apply immediately.')}</p>
      <select id="app-language" className={styles.pathInput} value={language} disabled={languageSaving}
        onChange={event => {
          setLanguageSaving(true); setError(null);
          void setLanguage(event.target.value as Language).catch(reason => setError(String(reason)))
            .finally(() => setLanguageSaving(false));
        }}>
        <option value="en" lang="en">English</option>
        <option value="tr" lang="tr">Türkçe</option>
      </select>
      <h3>{tr("Catalog Storage")}</h3>
      <p>{tr("Databases and thumbnails are stored here. A local SSD is recommended for fast browsing. Choosing a NAS, external disk, or slower drive affects search and preview speed.")}</p>
      {settings && <>
        <p>{tr("Current location:")} <span className={styles.path}>{settings.activePath}</span></p>
        <label htmlFor="storage-path">{tr("New location")}</label>
        <input id="storage-path" value={path} readOnly className={styles.pathInput} />
        <div className={styles.actions}>
          <button type="button" disabled={busy} onClick={browse}>{tr("Choose Folder")}</button>
          <button type="button" disabled={busy} onClick={() => setPath(settings.defaultPath)}>{tr("Use Default")}</button>
        </div>
        <p>{tr("Each library has a separate subfolder with its name and unique ID. Location changes take effect after restarting LumiaDAM. Existing catalogs and previews are copied; the old copy and source files are preserved. Large catalogs may take time to transfer.")}</p>
        {settings.restartRequired && <p role="status">{tr("New location saved. Restart LumiaDAM to transfer your catalogs.")}</p>}
        {settings.errors.map((message, index) => <p key={index} role="alert">{translateMessage(message)}</p>)}
      </>}
      {error && <p role="alert">{translateMessage(error)}</p>}
      <div className={styles.actions}>
        <button type="button" disabled={busy || !settings || path === settings.selectedPath} onClick={save}>
          {busy ? tr("Saving…") : tr("Save")}
        </button>
        <button type="button" onClick={() => dialog.current?.close()}>{tr("Close")}</button>
      </div>
    </dialog>
  );
}
