import { useCallback, useEffect, useRef, useState } from 'react';
import { api, type LibraryWorkStatus } from '../../api';
import { useAssetStore, useLibraryStore } from '../../store';
import { IconRefresh } from '../Icons';
import styles from './StatusBar.module.css';
import { listen } from '@tauri-apps/api/event';
import { createStatusPolling } from './statusPolling';

export function StatusBar() {
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
  const label = error ? 'Needs attention' : scanning || rescanPending ? 'Scanning' :
    preparing ? `Preparing previews · ${previewPending.toLocaleString()} left` :
    previewPaused ? 'Waiting for source · cached previews available' :
    statuses.some((status) => status.lastError) ? 'Needs attention' :
    previewFailed ? 'Preview errors' : loading ? 'Searching catalog' :
    !statusLoaded ? 'Checking activity' : libraries.length ? 'All tasks complete' : 'No libraries';

  return (
    <footer className={styles.statusBar} id="status-bar">
      <div className={styles.left}>
        <span className={styles.item}>{total.toLocaleString()} asset{total !== 1 ? 's' : ''}</span>
        {query.text && <span className={styles.item}>· Searching: <em>"{query.text}"</em></span>}
      </div>
      <div className={styles.right}>
        <span className={styles.item}>LumiaDAM v0.1.0</span>
        <button type="button" className={styles.statusButton} aria-expanded={open}
          aria-controls="work-status-panel" onClick={() => { setOpen(!open); void refreshStatus(); }}>
          <span className={scanning || previewPending ? styles.busyDot : styles.readyDot} />{label}
        </button>
      </div>
      {open && <section className={styles.panel} id="work-status-panel" aria-label="Library activity">
        <div className={styles.panelHeader}>
          <strong>Activity</strong>
          <button type="button" className={styles.rescan} onClick={() => void rescan()}
            disabled={!libraries.length || scanning || rescanPending} title="Rescan all libraries">
            <IconRefresh size={14} /> Rescan all
          </button>
        </div>
        {loading && <p className={styles.item}>Searching the local catalog…</p>}
        {error && <p className={styles.error} role="alert">{error}</p>}
        {!statuses.length && <p className={styles.item}>{libraries.length ? 'Loading library status…' : 'Add a library to start indexing.'}</p>}
        {statuses.map((status) => <div className={styles.library} key={status.id}>
          <strong>{status.name}</strong>
          <span>{status.scanning
            ? `Scanning files: ${status.scannedFiles.toLocaleString()} checked${status.estimatedRemaining === null ? '; remaining count unknown' : status.estimatedRemaining === 0 ? '; previous scan count reached, more files may remain' : `, about ${status.estimatedRemaining.toLocaleString()} remaining based on the previous scan`}`
            : status.lastError ? 'Library needs attention' : status.lastScanAt ? 'File scan complete' : 'Not scanned yet'}</span>
          <span>Previews: {status.previewCompleted.toLocaleString()} / {status.previewTotal.toLocaleString()} complete
            {status.previewPending > 0 && ` · ${status.previewPending.toLocaleString()} remaining`}
            {status.previewFailed > 0 && ` · ${status.previewFailed.toLocaleString()} failed`}</span>
          <span>Last full scan: {status.lastScanAt ? new Date(status.lastScanAt).toLocaleString() : 'Not recorded yet'}</span>
          {status.previewPaused && <span>Source unavailable. Cached previews remain available. New previews will retry automatically.</span>}
          {status.lastError && <span className={styles.error}>Library issue: {status.lastError}</span>}
        </div>)}
      </section>}
    </footer>
  );
}
