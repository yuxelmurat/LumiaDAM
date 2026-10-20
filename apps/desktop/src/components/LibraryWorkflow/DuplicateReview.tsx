import { t as tr, useTranslation, translateMessage } from '../../i18n';
import { useEffect, useRef, useState } from 'react';
import { convertFileSrc } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { api } from '../../api';
import { useAssetStore, useUIStore } from '../../store';
import type { Asset, DuplicateFile, DuplicateProgress, DuplicateResult } from '../../types';
import styles from './LibraryWorkflow.module.css';

export function DuplicateReview({ open, onClose, onOpen }: { open: boolean; onClose: () => void; onOpen: (asset: Asset) => Promise<void> }) {
  useTranslation();
  const dialog = useRef<HTMLDialogElement>(null);
  const query = useAssetStore(s => s.query);
  const [mode, setMode] = useState<'exact' | 'similar'>('exact');
  const [result, setResult] = useState<DuplicateResult | null>(null);
  const [running, setRunning] = useState(false);
  const [cancelling, setCancelling] = useState(false);
  const [progress, setProgress] = useState<DuplicateProgress | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [page, setPage] = useState(0);
  const [filePages, setFilePages] = useState<Record<number, number>>({});
  const [scope, setScope] = useState<string>("All libraries");
  useEffect(() => {
    if (open && !dialog.current?.open) dialog.current?.showModal();
    if (!open && dialog.current?.open) dialog.current?.close();
  }, [open]);
  useEffect(() => {
    const listener = listen<DuplicateProgress>('duplicates:progress', event => setProgress(event.payload));
    return () => { void listener.then(stop => stop()); };
  }, []);
  const scan = async () => {
    setRunning(true); setCancelling(false); setError(null); setResult(null); setProgress(null); setPage(0); setFilePages({});
    const libraries = query.libraryIds ?? (query.libraryId ? [query.libraryId] : []);
    setScope(libraries.length ? `${libraries.length} selected libraries` : "All libraries");
    try { setResult(await api.findDuplicates(mode, libraries)); }
    catch (reason) { setError(String(reason)); }
    finally { setRunning(false); }
  };
  const act = async (file: DuplicateFile, openFile: boolean) => {
    setError(null);
    try {
      const asset = await api.getAsset(file.id); useAssetStore.getState().rememberAsset(asset);
      const ui = useUIStore.getState(); ui.selectAsset(asset.id); if (!ui.detailPanelOpen) ui.toggleDetailPanel();
      if (openFile) await onOpen(asset); onClose();
    } catch (reason) { setError(String(reason)); }
  };
  return <dialog ref={dialog} className={styles.dialog} aria-label={tr("Compare library files")} onCancel={event => { event.preventDefault(); onClose(); }}>
    <div className={styles.dialogHeader}><h2>{tr("Compare library files")}</h2><button type="button" onClick={onClose}>{tr("Close")}</button></div>
    <div className={styles.row}>
      <select aria-label={tr("Comparison method")} value={mode} disabled={running} onChange={event => { setMode(event.target.value as typeof mode); setResult(null); }}>
        <option value="exact">{tr("Identical files")}</option><option value="similar">{tr("Similar renders")}</option>
      </select>
      <button type="button" disabled={running} onClick={() => void scan()}>{tr("Start comparison")}</button>
      {running && <button type="button" disabled={cancelling} onClick={() => void api.cancelDuplicateScan().then(() => setCancelling(true)).catch(reason => setError(String(reason)))}>{cancelling ? tr("Cancelling…") : tr("Cancel")}</button>}
    </div>
    <p className={styles.hint}>{mode === 'exact' ? tr("Reads files with matching sizes and verifies identical contents. Offline copies are preferred; unavailable sources are skipped. Large archives may take time.") : tr("Compares cached previews on this device. These are possible visual matches, not proof that the original files are identical.")}</p>
    <p className={styles.hint}>{tr("Uses selected libraries, or all libraries when none are selected. Files and folders are preserved.")}</p>
    {error && <p role="alert" className={styles.error}>{translateMessage(error)}</p>}
    {running && <div className={styles.progress} role="status"><span>{cancelling ? tr("Cancellation requested. Waiting for the current file operation to stop…") : translateMessage(progress?.phase ?? tr("Preparing comparison…"))}</span>
      <progress max={progress?.total || 1} value={progress?.processed ?? 0} /><span>{progress?.processed ?? 0} {tr("of")} {progress?.total ?? '…'} {tr("files ·")} {progress?.skipped ?? 0} {tr("skipped")}</span>
    </div>}
    {result && <>
      <p role="status">{result.cancelled ? tr("Comparison cancelled. Partial results:") : tr("Comparison complete:")} {result.groupCount} {tr("groups ·")} {result.processed} {tr("of")} {result.total} {tr("candidates checked ·")} {result.skipped} {tr("skipped")}</p>
      <p className={styles.hint}>{translateMessage(scope)}{result.limited ? ' · ' + tr('Showing the largest 500 groups. Select a library to narrow the comparison.') : ''}</p>
      {result.groups.length === 0 && <p>{tr("No matches found among the files that could be checked.")}</p>}
      {result.groups.slice(page * 10, page * 10 + 10).map((group, index) => <section key={`${page}:${index}`} className={styles.comparison}>
        <h3>{group.kind === 'exact' ? tr("Identical contents") : tr("Possible visual match")} · {group.files.length} {tr("files")}</h3>
        <div className={styles.comparisonFiles}>{group.files.slice((filePages[page * 10 + index] ?? 0) * 12, ((filePages[page * 10 + index] ?? 0) + 1) * 12).map(file => <article key={`${file.libraryId}:${file.id}`} className={styles.comparisonFile}>
          {file.thumbnailPath && <img className={styles.preview} src={convertFileSrc(file.thumbnailPath)} alt={file.fileName} loading="lazy" />}
          <strong>{file.fileName}</strong><p>{file.filePath}</p><p>{(file.sizeBytes / 1024 / 1024).toFixed(1)} MB</p>
          <div className={styles.row}><button onClick={() => void act(file, false)}>{tr("Show details")}</button><button onClick={() => void act(file, true)}>{tr("Open")}</button></div>
        </article>)}</div>
        {group.files.length > 12 && <div className={styles.row} style={{ marginTop: 10 }}>
          <button disabled={!filePages[page * 10 + index]} onClick={() => setFilePages(values => ({ ...values, [page * 10 + index]: (values[page * 10 + index] ?? 0) - 1 }))}>{tr("Previous files")}</button>
          <span>{(filePages[page * 10 + index] ?? 0) + 1} / {Math.ceil(group.files.length / 12)}</span>
          <button disabled={((filePages[page * 10 + index] ?? 0) + 1) * 12 >= group.files.length} onClick={() => setFilePages(values => ({ ...values, [page * 10 + index]: (values[page * 10 + index] ?? 0) + 1 }))}>{tr("Next files")}</button>
        </div>}
      </section>)}
      {result.groups.length > 10 && <div className={styles.row} style={{ marginTop: 16 }}>
        <button disabled={page === 0} onClick={() => setPage(value => value - 1)}>{tr("Previous groups")}</button><span>{page + 1} / {Math.ceil(result.groups.length / 10)}</span>
        <button disabled={(page + 1) * 10 >= result.groups.length} onClick={() => setPage(value => value + 1)}>{tr("Next groups")}</button>
      </div>}
    </>}
  </dialog>;
}
