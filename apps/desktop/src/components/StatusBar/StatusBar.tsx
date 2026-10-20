import { t as tr, useTranslation, translateMessage, formatNumber, formatDate } from '../../i18n';
import { useCallback, useEffect, useRef, useState } from 'react';
import { api, type LibraryWorkStatus } from '../../api';
import { useAssetStore, useLibraryStore } from '../../store';
import { IconRefresh } from '../Icons';
import styles from './StatusBar.module.css';
import { listen } from '@tauri-apps/api/event';
import { createStatusPolling } from './statusPolling';

export function StatusBar() {
  useTranslation();
  const { total, query, loading } = useAssetStore();
  const libraries = useLibraryStore((s) => s.libraries);
  const [open, setOpen] = useState(false);
  const [statuses, setStatuses] = useState<LibraryWorkStatus[]>([]);
  const [statusLoaded, setStatusLoaded] = useState(false);
  const [rescanPending, setRescanPending] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const openRef = useRef(open);
  openRef.current = open;
  const polling = useRef<ReturnType<typeof createStatusPolling> | null>(null);
  const refreshStatus = useCallback(() => polling.current?.refresh() ?? Promise.resolve(), []);

  useEffect(() => {
    const poller = createStatusPolling({
      read: api.getLibraryWorkStatus,
      panelOpen: () => openRef.current,
      onStatus: (value) => { setStatuses(value); setStatusLoaded(true); setError(null); },
      onError: (cause) => setError(String(cause)),
    });
    polling.current = poller;
    void poller.refresh();
    const listeners = ['preview:done', 'preview:error', 'library:updated', 'library:activity']
      .map((name) => listen(name, poller.invalidate));
    return () => {
      poller.stop(); polling.current = null;
      listeners.forEach((unlisten) => { void unlisten.then((fn) => fn()).catch(console.error); });
    };
  }, []);

  const libraryIds = libraries.map((library) => library.id).join(',');
  useEffect(() => { polling.current?.invalidate(); }, [libraryIds]);

  const rescan = async () => {
    setError(null);
    setRescanPending(true);
    try {
      await api.refreshAllLibraries();
      await Promise.all([
        refreshStatus(),
        useAssetStore.getState().search(),
        useLibraryStore.getState().fetchLibraries(),
      ]);
    } catch (cause) {
      setError(`Rescan failed: ${String(cause)}`);
    } finally {
      setRescanPending(false);
    }
  };

  const scanning = statuses.some((status) => status.scanning);
  const previewPending = statuses.reduce((sum, status) => sum + status.previewPending, 0);
  const previewFailed = statuses.reduce((sum, status) => sum + status.previewFailed, 0);
  const previewPaused = statuses.some((status) => status.previewPaused);
  const preparing = statuses.some((status) => status.previewPending > 0 && !status.previewPaused);
  const label = error ? tr("Needs attention") : scanning || rescanPending ? tr("Scanning") :
    preparing ? tr('Preparing previews · {count} left', { count: formatNumber(previewPending) }) :
    previewPaused ? tr("Waiting for source · cached previews available") :
    statuses.some((status) => status.lastError) ? tr("Needs attention") :
    previewFailed ? tr("Preview errors") : loading ? tr("Searching catalog") :
    !statusLoaded ? tr("Checking activity") : libraries.length ? tr("All tasks complete") : tr("No libraries");

  return (
    <footer className={styles.statusBar} id="status-bar">
      <div className={styles.left}>
        <span className={styles.item}>{tr(total === 1 ? '{count} asset' : '{count} assets', { count: formatNumber(total) })}</span>
        {query.text && <span className={styles.item}>{tr("· Searching:")} <em>"{query.text}"</em></span>}
      </div>
      <div className={styles.right}>
        <span className={styles.item}>LumiaDAM v0.1.1</span>
        <button type="button" className={styles.statusButton} aria-expanded={open}
          aria-controls="work-status-panel" onClick={() => { setOpen(!open); void refreshStatus(); }}>
          <span className={scanning || previewPending ? styles.busyDot : styles.readyDot} />{label}
        </button>
      </div>
      {open && <section className={styles.panel} id="work-status-panel" aria-label={tr("Library activity")}>
        <div className={styles.panelHeader}>
          <strong>{tr("Activity")}</strong>
          <button type="button" className={styles.rescan} onClick={() => void rescan()}
            disabled={!libraries.length || scanning || rescanPending} title={tr("Rescan all libraries")}>
            <IconRefresh size={14} /> {tr("Rescan all")} </button>
        </div>
        {loading && <p className={styles.item}>{tr("Searching the local catalog…")}</p>}
        {error && <p className={styles.error} role="alert">{translateMessage(error)}</p>}
        {!statuses.length && <p className={styles.item}>{libraries.length ? tr("Loading library status…") : tr("Add a library to start indexing.")}</p>}
        {statuses.map((status) => <div className={styles.library} key={status.id}>
          <strong>{status.name}</strong>
          <span>{status.scanning
            ? tr('Scanning files: {count} checked', { count: formatNumber(status.scannedFiles) }) + ' · ' +
              (status.estimatedRemaining === null ? tr('Remaining count unknown') : status.estimatedRemaining === 0
                ? tr('Previous scan count reached; more files may remain')
                : tr('About {count} remaining based on the previous scan', { count: formatNumber(status.estimatedRemaining) }))
            : status.lastError ? tr("Library needs attention") : status.lastScanAt ? tr("File scan complete") : tr("Not scanned yet")}</span>
          <span>{tr('Previews: {done} / {total} complete', { done: formatNumber(status.previewCompleted), total: formatNumber(status.previewTotal) })}
            {status.previewPending > 0 && ' · ' + tr('{count} remaining', { count: formatNumber(status.previewPending) })}
            {status.previewFailed > 0 && ' · ' + tr('{count} failed', { count: formatNumber(status.previewFailed) })}</span>
          <span>{tr("Last full scan:")} {status.lastScanAt ? formatDate(status.lastScanAt) : tr("Not recorded yet")}</span>
          {status.previewPaused && <span>{tr("Source unavailable. Cached previews remain available. New previews will retry automatically.")}</span>}
          {status.lastError && <span className={styles.error}>{tr("Library issue:")} {translateMessage(status.lastError)}</span>}
        </div>)}
      </section>}
    </footer>
  );
}
