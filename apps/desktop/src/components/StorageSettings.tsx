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
      const selected = await open({ directory: true, multiple: false, title: 'Katalog depolama klasörünü seçin' });
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
      <h2 id="storage-title">Katalog depolama</h2>
      <p>Veritabanları ve küçük resimler bu konumda tutulur. Hızlı gezinme için yerel SSD önerilir.
        NAS, harici disk veya yavaş bir sürücü seçmek arama ve önizleme hızını etkiler.</p>
      {settings && <>
        <p>Şu an kullanılan konum: <span className={styles.path}>{settings.activePath}</span></p>
        <label htmlFor="storage-path">Yeni konum</label>
        <input id="storage-path" value={path} readOnly className={styles.pathInput} />
        <div className={styles.actions}>
          <button type="button" disabled={busy} onClick={browse}>Klasör seç</button>
          <button type="button" disabled={busy} onClick={() => setPath(settings.defaultPath)}>Varsayılana dön</button>
        </div>
        <p>Her kütüphane için adı ve benzersiz kimliğiyle ayrı bir alt klasör oluşturulur.
          Konum değişikliği uygulamayı kapatıp açınca uygulanır. Mevcut katalog ve önizlemeler kopyalanır;
          eski kopya ve kaynak dosyalar korunur. Büyük kataloglarda ilk aktarım zaman alabilir.</p>
        {settings.restartRequired && <p role="status">Yeni konum kaydedildi. Aktarım için LumiaDAM’i kapatıp yeniden açın.</p>}
        {settings.errors.map((message, index) => <p key={index} role="alert">{message}</p>)}
      </>}
      {error && <p role="alert">{error}</p>}
      <div className={styles.actions}>
        <button type="button" disabled={busy || !settings || path === settings.selectedPath} onClick={save}>
          {busy ? 'Kaydediliyor…' : 'Kaydet'}
        </button>
        <button type="button" onClick={() => dialog.current?.close()}>Kapat</button>
      </div>
    </dialog>
  );
}
