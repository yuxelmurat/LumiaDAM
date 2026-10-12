import { invoke } from '@tauri-apps/api/core';
import type {
  Asset,
  Library,
  Collection,
  Tag,
  AddLibraryPayload,
  SearchPayload,
  SearchResult,
  PreviewJob,
} from '../types';

/** Thin wrapper around Tauri IPC commands */
export interface StorageSettings {
  activePath: string;
  selectedPath: string;
  defaultPath: string;
  restartRequired: boolean;
  errors: string[];
}

export interface LibraryWorkStatus {
  id: string;
  name: string;
  scanning: boolean;
  scannedFiles: number;
  estimatedRemaining: number | null;
  lastScanAt: number | null;
  lastError: string | null;
  previewTotal: number;
  previewCompleted: number;
  previewPending: number;
  previewFailed: number;
  previewPaused: boolean;
}

export const api = {
  getStorageSettings: () => invoke<StorageSettings>('get_storage_settings'),
  setStorageLocation: (path: string) => invoke<StorageSettings>('set_storage_location', { path }),
  // Libraries
  addLibrary: (payload: AddLibraryPayload) =>
    invoke<Library>('add_library', { payload }),
  listLibraries: () => invoke<Library[]>('list_libraries'),
  getLibraryWorkStatus: () => invoke<LibraryWorkStatus[]>('get_library_work_status'),
  removeLibrary: (id: string) => invoke<void>('remove_library', { id }),
  refreshLibrary: (id: string) => invoke<void>('refresh_library', { id }),
  refreshAllLibraries: () => invoke<void>('refresh_all_libraries'),

  // Assets
  searchAssets: (payload: SearchPayload) =>
    invoke<SearchResult>('search_assets', { payload }),
  getAsset: (id: string) => invoke<Asset>('get_asset', { id }),
  updateAsset: (id: string, patch: Partial<Asset>) =>
    invoke<Asset>('update_asset', { id, patch }),
  deleteAsset: (id: string, deleteFile?: boolean) =>
    invoke<void>('delete_asset', { id, deleteFile: deleteFile ?? true }),
  deleteAssets: (ids: string[], deleteFile?: boolean) =>
    invoke<void>('delete_assets', { ids, deleteFile: deleteFile ?? true }),
  renameAsset: (id: string, newName: string) =>
    invoke<Asset>('rename_asset', { id, newName }),

  // Tags
  listTags: () => invoke<Tag[]>('list_tags'),
  createTag: (name: string, parentId?: string) =>
    invoke<Tag>('create_tag', { name, parentId }),
  deleteTag: (id: string) => invoke<void>('delete_tag', { id }),

  // Collections
  listCollections: () => invoke<Collection[]>('list_collections'),
  createCollection: (
    name: string,
    description?: string,
    parentId?: string,
    isSmart?: boolean,
    filterSpec?: string,
  ) =>
    invoke<Collection>('create_collection', {
      name,
      description,
      parentId,
      isSmart,
      filterSpec,
    }),
  addToCollection: (collectionId: string, assetIds: string[]) =>
    invoke<void>('add_to_collection', { collectionId, assetIds }),
  removeFromCollection: (collectionId: string, assetIds: string[]) =>
    invoke<void>('remove_from_collection', { collectionId, assetIds }),
  deleteCollection: (id: string) =>
    invoke<void>('delete_collection', { id }),

  // Preview
  prioritizePreviews: (assetIds: string[]) =>
    invoke<void>('prioritize_previews', { assetIds }),
  requestPreview: (assetIds: string[]) =>
    invoke<void>('request_preview', { assetIds }),
  getPreviewJobs: () => invoke<PreviewJob[]>('get_preview_jobs'),

  // Reveal in explorer / open with default app
  revealInExplorer: (filePath: string) =>
    invoke<void>('reveal_in_explorer', { filePath }),
  openWithDefault: (filePath: string) =>
    invoke<void>('open_with_default', { filePath }),
};
