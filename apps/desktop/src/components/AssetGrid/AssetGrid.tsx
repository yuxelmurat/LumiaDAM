import { useCallback, useEffect, useRef, useState } from 'react';
import clsx from 'clsx';
import { open } from '@tauri-apps/plugin-dialog';
import { api } from '../../api';
import { useUIStore, useAssetStore, useLibraryStore } from '../../store';
import { activateAsset, canEditImage } from './assetActions';
import { observeVisiblePreviews } from './visiblePreviews';
import { AssetCard } from './AssetCard';
import { AssetListRow } from './AssetListRow';
import { FilterBar } from './FilterBar';
import { FolderNavigation, FolderResults } from './FolderBrowser';
import type { Asset } from '../../types';
import styles from './AssetGrid.module.css';

const GRID_COLS: Record<'sm' | 'md' | 'lg', number> = {
  sm: 180,
  md: 220,
  lg: 280,
};

export function AssetGrid({ onPreviewAsset, onOpenEditorAsset }: {
  onPreviewAsset: (asset: Asset) => void;
  onOpenEditorAsset: (asset: Asset) => void | Promise<void>;
}) {
  const { viewMode, gridSize, selectedAssetIds, selectAsset, clearSelection } = useUIStore();
  const { assets, folders, folderTotal, loading, total, searchError, folderLoading, folderError, searchFolders, search, page, pageSize, setPage, query } = useAssetStore();
  const [contextMenu, setContextMenu] = useState<{ asset: Asset; x: number; y: number } | null>(null);
  const [openError, setOpenError] = useState<string | null>(null);
  const contextRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!contextMenu) return;
    contextRef.current?.querySelector<HTMLButtonElement>('button:not(:disabled)')?.focus();
    const outside = (event: PointerEvent) => {
      if (!contextRef.current?.contains(event.target as Node)) setContextMenu(null);
    };
    const escape = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        setContextMenu(null);
        document.getElementById(`asset-${contextMenu.asset.id}`)?.focus();
        document.getElementById(`asset-row-${contextMenu.asset.id}`)?.focus();
      }
    };
    window.addEventListener('pointerdown', outside);
    window.addEventListener('keydown', escape);
    return () => { window.removeEventListener('pointerdown', outside); window.removeEventListener('keydown', escape); };
  }, [contextMenu]);
  const assetAt = (target: EventTarget) => {
    const card = (target as Element).closest<HTMLElement>('[data-asset-id]');
    return card?.dataset.assetId ? useAssetStore.getState().getById(card.dataset.assetId) : undefined;
  };
  const openAsset = async (asset: Asset) => {
    setContextMenu(null); setOpenError(null); selectAsset(asset.id);
    try { await activateAsset(asset, onPreviewAsset, onOpenEditorAsset); }
    catch (error) { setOpenError(`Could not open ${asset.fileName}: ${String(error)}`); }
  };
  const [activeTab, setActiveTab] = useState<'files' | 'folders'>('files');
  const previewViewport = useRef<HTMLDivElement>(null);
  const prioritizedPreviews = useRef(new Set<string>());

  useEffect(() => { setActiveTab('files'); }, [query.folderPath]);
  useEffect(() => {
    if (activeTab !== 'files' || !previewViewport.current) return;
    return observeVisiblePreviews(previewViewport.current, prioritizedPreviews.current, api.prioritizePreviews);
  }, [assets, activeTab, viewMode, gridSize, loading]);

  const tileSize = GRID_COLS[gridSize];

  const handleBackdropClick = useCallback(
    (e: React.MouseEvent) => {
      if (e.target === e.currentTarget) clearSelection();
    },
    [clearSelection]
  );

  const handleAssetClick = useCallback(
    (asset: Asset, e: React.MouseEvent) => {
      selectAsset(asset.id, e.metaKey || e.ctrlKey);
    },
    [selectAsset]
  );

  return (
    <div className={styles.container}>
      <FilterBar />
      {openError && <div className={styles.openError} role="alert">{openError}
        <button type="button" onClick={() => setOpenError(null)} aria-label="Dismiss file opening error">×</button>
      </div>}
      {contextMenu && <div ref={contextRef} className={styles.contextMenu} role="menu" aria-label="File actions"
        style={{ left: contextMenu.x, top: contextMenu.y }} onKeyDown={(event) => {
          if (!['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key)) return;
          event.preventDefault();
          const buttons = Array.from(event.currentTarget.querySelectorAll<HTMLButtonElement>('button:not(:disabled)'));
          const index = buttons.indexOf(document.activeElement as HTMLButtonElement);
          const next = event.key === 'Home' ? 0 : event.key === 'End' ? buttons.length - 1 :
            (index + (event.key === 'ArrowDown' ? 1 : -1) + buttons.length) % buttons.length;
          buttons[next]?.focus();
        }}>
        <button type="button" role="menuitem" onClick={() => void openAsset(contextMenu.asset)}>Open</button>
        <button type="button" role="menuitem" disabled={!canEditImage(contextMenu.asset)}
          onClick={() => {
            const asset = contextMenu.asset; setContextMenu(null); setOpenError(null);
            void Promise.resolve().then(() => onOpenEditorAsset(asset))
              .catch(error => setOpenError(`Could not edit ${asset.fileName}: ${String(error)}`));
          }}>Open in PhotoEditor</button>
        <button type="button" role="menuitem" onClick={() => {
          const asset = contextMenu.asset; setContextMenu(null);
          void api.revealInExplorer(asset.filePath).catch(error => setOpenError(`Could not show ${asset.fileName}: ${String(error)}`));
        }}>Show in Explorer</button>
      </div>}
      <FolderNavigation />
      <div className={styles.tabs} role="tablist" aria-label="Results" onKeyDown={(event) => {
        if (!['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(event.key)) return;
        event.preventDefault();
        const next = event.key === 'Home' ? 'files' : event.key === 'End' ? 'folders' :
          activeTab === 'files' ? 'folders' : 'files';
        setActiveTab(next);
        (event.currentTarget.querySelector(`[data-tab="${next}"]`) as HTMLButtonElement)?.focus();
      }}>
        <button type="button" role="tab" data-tab="files" id="files-tab" aria-controls="files-panel"
          tabIndex={activeTab === 'files' ? 0 : -1} aria-selected={activeTab === 'files'}
          className={clsx(styles.tab, activeTab === 'files' && styles.activeTab)}
          onClick={() => setActiveTab('files')}>
          Files <small>{total.toLocaleString()}</small>
        </button>
        <button type="button" role="tab" data-tab="folders" id="folders-tab" aria-controls="folders-panel"
          tabIndex={activeTab === 'folders' ? 0 : -1} aria-selected={activeTab === 'folders'}
          className={clsx(styles.tab, activeTab === 'folders' && styles.activeTab)}
          onClick={() => setActiveTab('folders')}>
          Folders <small aria-label={folderLoading ? 'Counting folders' : undefined}>{folderLoading ? '…' : folderError ? '!' : folderTotal.toLocaleString()}</small>
        </button>
      </div>
      {searchError ? (
        <div className={styles.empty} role="alert">
          <p className={styles.emptyText}>{searchError}</p>
          <button type="button" className={styles.emptyBtn} onClick={() => search()}>Try again</button>
        </div>
      ) : loading && assets.length === 0 && folders.length === 0 ? (
        <div className={styles.loadingGrid} style={{ '--tile-size': `${tileSize}px` } as React.CSSProperties}>
          {Array.from({ length: 24 }).map((_, i) => (
            <div key={i} className={clsx(styles.skeletonCard, 'skeleton')} />
          ))}
        </div>
      ) : !loading && !folderLoading && !folderError && total === 0 && folderTotal === 0 ? (
        <EmptyState />
      ) : activeTab === 'folders' ? (
        <div className={styles.scrollArea} role="tabpanel" id="folders-panel" aria-labelledby="folders-tab">
          {folderLoading ? <p className={styles.emptyText} role="status">Searching folders…</p> : folderError ? <div role="alert">
            <p className={styles.emptyText}>{folderError}</p><button type="button" onClick={() => void searchFolders()}>Try again</button>
          </div> : folderTotal ? <FolderResults /> : <p className={styles.emptyText}>No matching folders.</p>}
        </div>
      ) : (
      <div className={styles.scrollArea} role="tabpanel" ref={previewViewport} id="files-panel" aria-labelledby="files-tab" onClick={handleBackdropClick}
        onDoubleClick={(event) => {
          if ((event.target as Element).closest('button, a, input, select')) return;
          const asset = assetAt(event.target); if (asset) void openAsset(asset);
        }}
        onKeyDown={(event) => {
          if (event.key !== 'Enter' || (event.target as Element).closest('button, a, input, select')) return;
          const asset = assetAt(event.target); if (asset) { event.preventDefault(); void openAsset(asset); }
        }}
        onContextMenu={(event) => {
          const asset = assetAt(event.target); if (!asset) return;
          event.preventDefault();
          const selection = useUIStore.getState();
          if (!selection.selectedAssetIds.has(asset.id)) selectAsset(asset.id);
          else selection.focusAsset(asset.id);
          const rect = (event.target as Element).closest('[data-asset-id]')!.getBoundingClientRect();
          setContextMenu({ asset, x: Math.max(8, Math.min(event.clientX || rect.left + 12, window.innerWidth - 228)),
            y: Math.max(8, Math.min(event.clientY || rect.top + 12, window.innerHeight - 140)) });
        }}>
        {total === 0 ? <p className={styles.emptyText}>No matching files. Check the Folders tab or adjust your filters.</p> : viewMode === 'grid' ? (
          <div
            className={styles.grid}
            style={{ '--tile-size': `${tileSize}px` } as React.CSSProperties}
          >
            {assets.map((asset) => (
              <AssetCard
                key={asset.id}
                asset={asset}
                selected={selectedAssetIds.has(asset.id)}
                onClick={handleAssetClick}
              />
            ))}
          </div>
        ) : (
          <div className={styles.list}>
            {assets.map((asset) => (
              <AssetListRow
                key={asset.id}
                asset={asset}
                selected={selectedAssetIds.has(asset.id)}
                onClick={handleAssetClick}
              />
            ))}
          </div>
        )}
      </div>
      )}
      {activeTab === 'files' && !searchError && <div className={styles.footer}>
        {total > pageSize && <div className={styles.folderNavigation}>
          <button type="button" disabled={page === 0} onClick={() => { setPage(page - 1); search(); }}>Previous files</button>
          <button type="button" disabled={(page + 1) * pageSize >= total} onClick={() => { setPage(page + 1); search(); }}>Next files</button>
        </div>}
        <span className={styles.footerCount}>
          {assets.length} of {total.toLocaleString()} files
        </span>
      </div>}
    </div>
  );
}

function EmptyState() {
  const libraries = useLibraryStore((s) => s.libraries);
  const folderPath = useAssetStore((s) => s.query.folderPath);
  const addLibrary = useLibraryStore((s) => s.addLibrary);
  const resetFilter = useAssetStore((s) => s.resetFilter);
  const search = useAssetStore((s) => s.search);

  const handleAddLibrary = async () => {
    try {
      const selectedPath = await open({
        directory: true,
        multiple: false,
        title: 'Select Library Folder',
      });
      if (selectedPath && typeof selectedPath === 'string') {
        await addLibrary([selectedPath]);
      }
    } catch (err) {
      console.error('Failed to add library:', err);
    }
  };

  const handleClearFilters = () => {
    resetFilter();
    search();
  };

  const hasLibraries = libraries.length > 0;

  return (
    <div className={styles.empty}>
      <div className={styles.emptyIcon}>
        <svg viewBox="0 0 80 80" fill="none" xmlns="http://www.w3.org/2000/svg">
          <rect x="10" y="20" width="60" height="48" rx="6"
            fill="hsl(220 13% 16%)" stroke="hsl(220 10% 25%)" strokeWidth="1.5"/>
          <path d="M28 36l8 8 8-8" stroke="hsl(220 85% 50%)" strokeWidth="2"
            strokeLinecap="round" strokeLinejoin="round"/>
          <circle cx="40" cy="12" r="8" fill="hsl(220 85% 50% / 0.15)"
            stroke="hsl(220 85% 50%)" strokeWidth="1.5"/>
          <path d="M40 8v8M36 12h8" stroke="hsl(220 85% 50%)" strokeWidth="1.5"
            strokeLinecap="round"/>
        </svg>
      </div>
      <h2 className={styles.emptyTitle}>
        {folderPath !== undefined ? 'No matching files or folders here' : hasLibraries ? 'No matching assets found' : 'No libraries yet'}
      </h2>
      <p className={styles.emptyText}>
        {folderPath !== undefined
          ? 'This folder is empty or no files match your current filters.'
          : hasLibraries
          ? 'No assets match your current search or filter criteria.'
          : 'Add a library folder to start browsing your 3D assets, materials, textures, and videos.'}
      </p>
      <div className={styles.emptyActions}>
        {hasLibraries && (
          <button
            type="button"
            className={styles.emptyBtn}
            id="empty-clear-filters"
            onClick={handleClearFilters}
          >
            Clear Filters
          </button>
        )}
        {folderPath === undefined && <button
          type="button"
          className={hasLibraries ? styles.emptyBtnSecondary : styles.emptyBtn}
          id="empty-add-library"
          onClick={handleAddLibrary}
        >
          + Add Library
        </button>}
      </div>
    </div>
  );
}
