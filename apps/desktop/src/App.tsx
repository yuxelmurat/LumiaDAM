import { useEffect, useState, useRef } from 'react';
import { listen } from '@tauri-apps/api/event';
import { Sidebar } from './components/Sidebar/Sidebar';
import { Toolbar } from './components/Toolbar/Toolbar';
import { AssetGrid } from './components/AssetGrid/AssetGrid';
import { DetailPanel } from './components/DetailPanel/DetailPanel';
import { StatusBar } from './components/StatusBar/StatusBar';
import { QuickLookModal } from './components/QuickLook/QuickLookModal';
import { ConfirmDeleteDialog } from './components/ConfirmDeleteDialog';
import { IconCheck } from './components/Icons';
import { useUIStore } from './store';
import { useLibraryStore, useTagStore, useCollectionStore, useAssetStore } from './store';
import { api } from './api';
import styles from './App.module.css';
import { EditorPanel, type EditorPanelHandle } from './components/Editor/EditorPanel';
import { editors, canOpenInEditor, editorForAsset, type EditorId } from './components/Editor/editors';
import { message } from '@tauri-apps/plugin-dialog';
import type { Asset } from './types';

function isEditingText(): boolean {
  const active = document.activeElement;
  if (!active) return false;
  const tag = active.tagName.toLowerCase();
  return tag === 'input' || tag === 'textarea' || (active as HTMLElement).isContentEditable;
}

export default function App() {
  const { sidebarOpen, detailPanelOpen, focusedAssetId } = useUIStore();
  const focusedAsset = useAssetStore((s) =>
    focusedAssetId ? s.assets.find((a) => a.id === focusedAssetId) ?? s.assetMap.get(focusedAssetId) ?? null : null
  );
  const fetchLibraries = useLibraryStore((s) => s.fetchLibraries);
  const fetchTags = useTagStore((s) => s.fetchTags);
  const fetchCollections = useCollectionStore((s) => s.fetchCollections);
  const search = useAssetStore((s) => s.search);

  const [quickLookMode, setQuickLookMode] = useState<'hold' | 'persistent' | null>(null);
  const quickLookModeRef = useRef(quickLookMode);
  quickLookModeRef.current = quickLookMode;
  const editorRef = useRef<EditorPanelHandle>(null);
  const [editorBusy, setEditorBusy] = useState(false);
  const [editorAsset, setEditorAsset] = useState<Asset | null>(null);
  const [editorKind, setEditorKind] = useState<EditorId>('photo');
  const [workspace, setWorkspace] = useState<'library' | EditorId>('library');
  const workspaceRef = useRef(workspace);
  workspaceRef.current = workspace;
  const [copyToast, setCopyToast] = useState<string | null>(null);
  const [assetsToDelete, setAssetsToDelete] = useState<string[]>([]);
  const [deleteDialogOpen, setDeleteDialogOpen] = useState(false);
  const toastTimeoutRef = useRef<number | null>(null);

  useEffect(() => {
    // Bootstrap data on mount
    Promise.all([fetchLibraries(), fetchTags(), fetchCollections()]).then(
      () => search()
    );

    // Real-time preview updates from background engine
    const unlistenDone = listen<{ assetId: string; thumbnailPath?: string; previewModel?: string }>(
      'preview:done',
      (event) => {
        useAssetStore.getState().updateAssetPreview(
          event.payload.assetId,
          event.payload.thumbnailPath,
          event.payload.previewModel
        );
      }
    );

    const unlistenError = listen<{ assetId: string }>(
      'preview:error',
      (event) => {
        useAssetStore.getState().setPreviewStatus(event.payload.assetId, 'error');
      }
    );

    const unlistenLib = listen('library:updated', () => {
      useAssetStore.getState().search();
      useLibraryStore.getState().fetchLibraries();
    });

    // Keyboard shortcuts: F5, Space hold for QuickLook, Ctrl+C to copy path
    const handleKeyDown = (e: KeyboardEvent) => {
      // Açık diyalogda kullanılan tuşlar arka plandaki dosyalara işlem yapmaz.
      if (document.querySelector('dialog[open], [role="menu"]')) return;
      if (quickLookModeRef.current) {
        if (e.key === 'Escape') setQuickLookMode(null);
        // Önizleme açıkken silme, yeniden adlandırma ve tarama arkadaki dosyalara uygulanmaz.
        return;
      }
      if (workspaceRef.current !== 'library') {
        if (e.key === 'F5' || ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'r')) e.preventDefault();
        return;
      }
      // Refresh
      if (e.key === 'F5' || (e.ctrlKey && e.key.toLowerCase() === 'r')) {
        e.preventDefault();
        api.refreshAllLibraries().then(() => {
          useAssetStore.getState().search();
          useLibraryStore.getState().fetchLibraries();
        }).catch(console.error);
        return;
      }

      // Copy path with Ctrl+C / Cmd+C when asset is selected
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'c' && !isEditingText()) {
        const selectedIds = useUIStore.getState().selectedAssetIds;
        const focusedId = useUIStore.getState().focusedAssetId;
        const ids = selectedIds.size > 0 ? Array.from(selectedIds) : focusedId ? [focusedId] : [];
        if (ids.length > 0) {
          const paths = ids
            .map((id) => useAssetStore.getState().getById(id)?.filePath)
            .filter(Boolean) as string[];
          if (paths.length > 0) {
            e.preventDefault();
            navigator.clipboard.writeText(paths.join('\n'));
            setCopyToast(
              paths.length === 1
                ? 'Copied file path to clipboard'
                : `Copied ${paths.length} file paths to clipboard`
            );
            if (toastTimeoutRef.current) clearTimeout(toastTimeoutRef.current);
            toastTimeoutRef.current = window.setTimeout(() => setCopyToast(null), 2200);
          }
        }
      }

      // Toggle favorite on focused/selected asset(s) with 'f' or 'F'
      if ((e.key === 'f' || e.key === 'F') && !e.ctrlKey && !e.metaKey && !e.altKey && !isEditingText()) {
        const selectedIds = useUIStore.getState().selectedAssetIds;
        const focusedId = useUIStore.getState().focusedAssetId;
        const ids = selectedIds.size > 0 ? Array.from(selectedIds) : focusedId ? [focusedId] : [];
        if (ids.length > 0) {
          e.preventDefault();
          const assetStore = useAssetStore.getState();
          const firstAsset = assetStore.getById(ids[0]);
          const newFav = !(firstAsset?.isFavorite ?? false);
          for (const id of ids) {
            assetStore.patchAsset(id, { isFavorite: newFav });
          }
          setCopyToast(
            ids.length === 1
              ? newFav ? 'Added to favorites' : 'Removed from favorites'
              : newFav ? `Added ${ids.length} assets to favorites` : `Removed ${ids.length} assets from favorites`
          );
          if (toastTimeoutRef.current) clearTimeout(toastTimeoutRef.current);
          toastTimeoutRef.current = window.setTimeout(() => setCopyToast(null), 2200);
        }
      }

      // Delete selected asset(s) with Delete or Backspace key
      if ((e.key === 'Delete' || e.key === 'Backspace') && !isEditingText()) {
        const selectedIds = useUIStore.getState().selectedAssetIds;
        const focusedId = useUIStore.getState().focusedAssetId;
        const ids = selectedIds.size > 0 ? Array.from(selectedIds) : focusedId ? [focusedId] : [];
        if (ids.length > 0) {
          e.preventDefault();
          setAssetsToDelete(ids);
          setDeleteDialogOpen(true);
          return;
        }
      }

      // F2 to trigger rename on focused asset
      if (e.key === 'F2' && !isEditingText()) {
        const focusedId = useUIStore.getState().focusedAssetId;
        if (focusedId) {
          e.preventDefault();
          const renameBtn = document.getElementById('detail-rename');
          renameBtn?.click();
          return;
        }
      }

      // Hold Spacebar for Quick Look preview
      if (e.code === 'Space' && !isEditingText()) {
        const focusedId = useUIStore.getState().focusedAssetId;
        if (focusedId) {
          e.preventDefault();
          if (!e.repeat && quickLookModeRef.current !== 'persistent') {
            setQuickLookMode('hold');
          }
        }
      }

      // Escape closes Quick Look
      if (e.key === 'Escape') {
        setQuickLookMode(null);
      }
    };

    const handleKeyUp = (e: KeyboardEvent) => {
      if (e.code === 'Space' && quickLookModeRef.current === 'hold') {
        setQuickLookMode(null);
      }
    };

    const handleBlur = () => {
      if (quickLookModeRef.current === 'hold') setQuickLookMode(null);
    };

    window.addEventListener('keydown', handleKeyDown);
    window.addEventListener('keyup', handleKeyUp);
    window.addEventListener('blur', handleBlur);

    return () => {
      unlistenDone.then((fn) => fn());
      unlistenError.then((fn) => fn());
      unlistenLib.then((fn) => fn());
      window.removeEventListener('keydown', handleKeyDown);
      window.removeEventListener('keyup', handleKeyUp);
      window.removeEventListener('blur', handleBlur);
      if (toastTimeoutRef.current) clearTimeout(toastTimeoutRef.current);
    };
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  const handleConfirmAppDelete = async () => {
    if (assetsToDelete.length === 0) return;
    const count = assetsToDelete.length;
    setDeleteDialogOpen(false);
    try {
      await useAssetStore.getState().deleteAssets(assetsToDelete, true);
      setCopyToast(count === 1 ? 'Moved file to Recycle Bin' : `Moved ${count} files to Recycle Bin`);
      if (toastTimeoutRef.current) clearTimeout(toastTimeoutRef.current);
      toastTimeoutRef.current = window.setTimeout(() => setCopyToast(null), 2200);
    } catch (err) {
      console.error('Failed to delete assets:', err);
    } finally {
      setAssetsToDelete([]);
    }
  };

  const openEditorAsset = async (asset: Asset | null, editor = editorForAsset(asset) ?? 'photo') => {
    if (asset && canOpenInEditor(asset, editor) && (asset.id !== editorAsset?.id || editor !== editorKind)) {
      // Kütüphaneye dönüş oturumu saklar; farklı dosyaya geçiş önce kayıpsız kapanışı denetler.
      if (editorAsset && !(await editorRef.current?.confirmClose())) return;
      setEditorAsset(asset);
      setEditorKind(editor);
    }
    setWorkspace(editor);
    setQuickLookMode(null);
  };
  const closeEditor = async () => {
    if (!(await editorRef.current?.confirmClose())) return;
    setEditorAsset(null); setEditorBusy(false); setWorkspace('library');
  };
  const reportEditorError = (error: unknown) => message(`The editor could not complete the action: ${String(error)}`,
    { title: 'LumiaDAM editor', kind: 'error' }).catch(console.error);

  return (
    <div className={styles.layout}>
      <Toolbar
        workspace={workspace}
        editorBusy={editorBusy}
        onCloseEditor={editorAsset && workspace === editorKind ? () => { void closeEditor().catch(reportEditorError); } : undefined}
        onOpenLibraries={() => setWorkspace('library')}
        onOpenEditor={editor => { void openEditorAsset(focusedAsset, editor).catch(reportEditorError); }}
      />
      <div className={styles.body} style={{ display: workspace === 'library' ? undefined : 'none' }}>
        {sidebarOpen && <Sidebar />}
        <main className={styles.main}>
          <AssetGrid onPreviewAsset={() => setQuickLookMode('persistent')} onOpenEditorAsset={openEditorAsset} />
        </main>
        {detailPanelOpen && <DetailPanel />}
      </div>
      <div className={styles.editorWorkspace} style={{ display: workspace !== 'library' ? undefined : 'none' }}>
        <div style={{ height: '100%', display: editorAsset && workspace === editorKind ? undefined : 'none' }}>
          {editorAsset && <EditorPanel key={`${editorKind}:${editorAsset.id}`} ref={editorRef} asset={editorAsset} editor={editorKind} onBusyChange={setEditorBusy} />}
        </div>
        {workspace !== 'library' && (!editorAsset || workspace !== editorKind) && (
          <div className={styles.editorEmpty}>
            <img src="/brand/lumiadam-mark.png" alt="" />
            <h2>{editors[workspace].title}</h2>
            <p>{editors[workspace].hint}</p>
            <button type="button" onClick={() => setWorkspace('library')}>Open Libraries</button>
          </div>
        )}
      </div>
      {workspace === 'library' && <StatusBar />}

      {/* Spacebar Quick Look Modal */}
      <QuickLookModal asset={focusedAsset} isOpen={quickLookMode !== null} persistent={quickLookMode === 'persistent'} onClose={() => setQuickLookMode(null)} />

      {/* Delete Confirmation Dialog */}
      <ConfirmDeleteDialog
        isOpen={deleteDialogOpen}
        itemType={assetsToDelete.length > 1 ? 'assets' : 'asset'}
        itemName={
          assetsToDelete.length === 1
            ? useAssetStore.getState().getById(assetsToDelete[0])?.fileName || '1 file'
            : `${assetsToDelete.length} files`
        }
        onConfirm={handleConfirmAppDelete}
        onCancel={() => {
          setDeleteDialogOpen(false);
          setAssetsToDelete([]);
        }}
      />

      {/* Copy Path Feedback Toast */}
      {copyToast && (
        <div className={styles.copyToast}>
          <IconCheck size={14} className={styles.toastCheck} />
          <span>{copyToast}</span>
        </div>
      )}
    </div>
  );
}
