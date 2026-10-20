import { t as tr, useTranslation, translateMessage } from '../../i18n';
import { useEffect, useState } from 'react';
import { convertFileSrc } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { api } from '../../api';
import { useAssetStore } from '../../store';
import type { Asset, Availability, GroupFile } from '../../types';
import { canOpenInEditor, editorForAsset, editors, type EditorId } from '../Editor/editors';
import { emptyLabels, labelNames } from './PathFilters';
import styles from './LibraryWorkflow.module.css';

export function AssetWorkflow({ asset, onOpen, onEdit }: { asset: Asset; onOpen: (asset: Asset) => Promise<void>; onEdit: (asset: Asset, editor?: EditorId) => void | Promise<void> }) {
  useTranslation();
  const [labels, setLabels] = useState(asset.pathLabels ?? emptyLabels);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [availability, setAvailability] = useState<Availability | null>(null);
  const [copyProgress, setCopyProgress] = useState<number | null>(null);
  const [imageSearch, setImageSearch] = useState('');
  const [images, setImages] = useState<GroupFile[]>([]);
  const [chosen, setChosen] = useState(asset.relatedImage?.id ?? '');
  useEffect(() => { setLabels(asset.pathLabels ?? emptyLabels); setChosen(asset.relatedImage?.id ?? ''); }, [asset.pathLabels, asset.relatedImage?.id]);
  useEffect(() => {
    let stale = false;
    const timer = window.setTimeout(() => void api.listRelatedImages(asset.id, imageSearch)
      .then(values => { if (!stale) setImages(values); }).catch(reason => { if (!stale) setError(String(reason)); }), 200);
    return () => { stale = true; window.clearTimeout(timer); };
  }, [asset.id, imageSearch]);
  useEffect(() => {
    const pending = listen<{ assetId: string; bytes: number; total: number }>('offline:progress', event => {
      if (event.payload.assetId === asset.id) setCopyProgress(event.payload.total ? Math.round(event.payload.bytes / event.payload.total * 100) : 100);
    });
    return () => { void pending.then(stop => stop()); };
  }, [asset.id]);
  const run = async (work: () => Promise<unknown>, success?: string) => {
    setBusy(true); setError(null); setNotice(null);
    try {
      await work();
      useAssetStore.getState().rememberAsset(await api.getAsset(asset.id));
      if (success) setNotice(success);
    } catch (reason) { setError(String(reason)); }
    finally { setBusy(false); setCopyProgress(null); }
  };
  const openMember = (id: string, edit = false) => run(async () => {
    const file = await api.getAsset(id);
    if (edit) await onEdit(file, editorForAsset(file) ?? 'photo'); else await onOpen(file);
  });
  return <div className={styles.panel}>
    {error && <p role="alert" className={styles.error}>{translateMessage(error)}</p>}
    {notice && <p role="status" className={styles.hint}>{translateMessage(notice)}</p>}
    <section className={styles.section}>
      <h3>{tr("Folder labels")}</h3><p className={styles.hint}>{tr("Inferred from folder names. Your corrections stay saved when the library is rescanned.")}</p>
      <div className={styles.fields}>{(Object.keys(labelNames) as (keyof typeof labelNames)[]).map(field => <label key={field}>{tr(labelNames[field])}
        <input value={labels[field]} maxLength={120} onChange={event => setLabels(values => ({ ...values, [field]: event.target.value }))} disabled={busy} />
      </label>)}</div>
      <div className={styles.row}>
        <button type="button" disabled={busy} onClick={() => void run(() => api.setPathLabels(asset.id, labels), "Folder labels saved.")}>{tr("Save labels")}</button>
        <button type="button" disabled={busy} onClick={() => void run(() => api.setPathLabels(asset.id, null), "Automatic folder labels restored.")}>{tr("Use folder names")}</button>
        {Object.keys(asset.labelOverrides ?? {}).length > 0 && <span className={styles.badge}>{tr("Saved labels")}</span>}
      </div>
    </section>
    <section className={styles.section}>
      <h3>{tr("Offline access")}</h3>
      <p className={styles.hint}>{asset.offlinePath ? asset.offlineStale ? tr("The offline copy is older than the catalog version. Refresh it to use the current file.") : tr("A full read-only copy is kept on this device. Editor exports are saved as new files in local storage.") : tr("Only the preview is cached. Keep a full copy to open this file while its source is disconnected.")}</p>
      {asset.offlinePath && <p className={styles.hint} title={asset.offlinePath} style={{ overflowWrap: 'anywhere' }}>{asset.offlinePath}</p>}
      <div className={styles.row}>
        <button type="button" disabled={busy} onClick={() => void run(() => api.keepOffline(asset.id), "Offline copy is ready.")}>{asset.offlinePath ? tr("Refresh offline copy") : tr("Keep available offline")}</button>
        {asset.offlinePath && <button type="button" disabled={busy} onClick={() => void run(() => api.removeOffline(asset.id), "Local copy removed. The source file is unchanged.")}>{tr("Remove local copy")}</button>}
        <button type="button" disabled={busy} onClick={() => void run(async () => setAvailability(await api.checkAssetAvailability(asset.id)))}>{tr("Check source")}</button>
      </div>
      {copyProgress !== null && <div role="status"><progress max={100} value={copyProgress} /> {tr("Copying:")} {copyProgress}%</div>}
      {availability && <p role="status" className={styles.hint}>{tr("Source:")} {tr(availability.sourceAvailable ? 'Available' : 'Unavailable')} {tr("· Local copy:")} {tr(availability.offlineAvailable ? availability.offlineStale ? 'Out of date' : 'Available' : 'Unavailable')}</p>}
    </section>
    <section className={styles.section}>
      <h3>{tr("Model files")} {asset.groupFiles && asset.groupFiles.length > 1 ? `(${asset.groupFiles.length})` : ''}</h3>
      <p className={styles.hint}>{tr("Files with the same name stem in this folder.")}</p>
      {(asset.groupFiles ?? []).map(file => <div key={file.id} className={styles.file}>
        <span title={file.filePath}>{file.fileName}</span><div className={styles.row}>
          <button type="button" disabled={busy} onClick={() => void openMember(file.id)}>{tr("Open")}</button>
          {canOpenInEditor(file, editorForAsset(file) ?? 'photo') && <button type="button" disabled={busy} onClick={() => void openMember(file.id, true)}>{tr(editors[editorForAsset(file) ?? 'photo'].title)}</button>}
        </div>
      </div>)}
    </section>
    {['3d_model','material','other'].includes(asset.kind) && <section className={styles.section}>
      <h3>{tr("Related render")}</h3>
      <p className={styles.hint}>{asset.relatedMode === 'pinned' ? tr("This image is pinned to this file.") : tr("Automatic matching uses the same folder and filename. You can pin a different image from this folder.")}</p>
      {asset.relatedImage?.thumbnailPath && <img className={styles.preview} src={convertFileSrc(asset.relatedImage.thumbnailPath)} alt={asset.relatedImage.fileName} />}
      {asset.relatedImage && <p>{asset.relatedImage.fileName}</p>}
      {asset.relatedImage?.id && <div className={styles.row}>
        <button disabled={busy} onClick={() => void openMember(asset.relatedImage!.id!)}>{tr("Open render")}</button>
        <button disabled={busy} onClick={() => void openMember(asset.relatedImage!.id!, true)}>{tr("PhotoEditor")}</button>
      </div>}
      <input aria-label={tr("Find a render in this folder")} placeholder={tr("Find a render in this folder…")} value={imageSearch} onChange={event => setImageSearch(event.target.value)} />
      <select aria-label={tr("Related render image")} value={chosen} onChange={event => setChosen(event.target.value)}>
        <option value="">{tr("Select an image from this folder")}</option>
        {asset.relatedImage?.id && !images.some(image => image.id === asset.relatedImage?.id) && <option value={asset.relatedImage.id}>{asset.relatedImage.fileName}</option>}
        {images.map(image => <option key={image.id} value={image.id}>{image.fileName}</option>)}
      </select>
      {images.length === 200 && <p className={styles.hint}>{tr("Showing the first 200 images. Search by filename to narrow the list.")}</p>}
      <div className={styles.row}>
        <button disabled={busy || !chosen} onClick={() => void run(() => api.setRelatedImage(asset.id, 'pinned', chosen), "Related render pinned.")}>{tr("Pin render")}</button>
        <button disabled={busy} onClick={() => void run(() => api.setRelatedImage(asset.id, 'auto'), "Automatic matching restored.")}>{tr("Automatic")}</button>
        <button disabled={busy} onClick={() => void run(() => api.setRelatedImage(asset.id, 'none'), "Related render removed.")}>{tr("Unlink")}</button>
      </div>
    </section>}
  </div>;
}
