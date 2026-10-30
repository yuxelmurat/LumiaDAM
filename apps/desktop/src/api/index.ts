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
  PathLabels, GroupFile, Availability, DuplicateResult,
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

export interface AiSettings {
  mode: 'disabled' | 'manual' | 'idle' | 'scheduled';
  provider: 'bundled' | 'ollama' | 'lmstudio';
  endpoint: string;
  model: string;
  idleMinutes: number;
  startHour: number;
  endHour: number;
  cpuOnly: boolean;
  batchSize: number;
  setupComplete: boolean;
}

export interface AiOverview {
  settings: AiSettings;
  runtimeInstalled: boolean;
  installRequested: boolean;
}

export interface AiStatus {
  state: string;
  phase: string;
  processed: number;
  total: number;
  suggestions: number;
  error: string | null;
  running: boolean;
  engineRunning: boolean;
  modelLoaded: boolean;
  lastRunAt: number | null;
  runId?: string | null;
  catalogAssets: number;
  analyzedAssets: number;
  pendingAssets: number;
  withoutPreview: number;
  skipped: number;
}

export interface AiLog {
  path: string;
  entries: { at: string; event: string; details: Record<string, unknown> }[];
}

export interface AiSuggestion {
  id: string;
  kind: 'collection' | 'duplicates';
  title: string;
  reason: string;
  assetIds: string[];
  collectionName: string | null;
  status: 'pending' | 'approved' | 'dismissed';
  totalItems?: number;
  items: { id: string; fileName: string; filePath: string; libraryName: string; thumbnailPath?: string | null; basedOn?: string | null }[];
}

export interface AiBulkApplyResult {
  approved: number;
  failed: number;
}

export const api = {
  acknowledgeCloseToTray: () => invoke<void>('acknowledge_close_to_tray'),
  getLocalAiSettings: () => invoke<AiOverview>('get_local_ai_settings'),
  saveLocalAiSettings: (settings: AiSettings) => invoke<AiOverview>('save_local_ai_settings', { settings }),
  getLocalAiStatus: () => invoke<AiStatus>('get_local_ai_status'),
  getLocalAiLog: () => invoke<AiLog>('get_local_ai_log'),
  openLocalAiLog: () => invoke<void>('open_local_ai_log'),
  startLocalAi: () => invoke<void>('start_local_ai'),
  stopLocalAi: () => invoke<void>('stop_local_ai'),
  prepareLocalAiModel: () => invoke<void>('prepare_local_ai_model'),
  listAiSuggestions: () => invoke<AiSuggestion[]>('list_ai_suggestions'),
  applyAiSuggestion: (id: string) => invoke<void>('apply_ai_suggestion', { id }),
  applyAllAiSuggestions: () => invoke<AiBulkApplyResult>('apply_all_ai_suggestions'),
  dismissAiSuggestion: (id: string) => invoke<void>('dismiss_ai_suggestion', { id }),
  getMaxInstances: () => invoke<MaxInstance[]>('get_max_instances'),
  mergeAssetToMax: (assetId: string, instanceId: string, scenePath: string, projectPath: string) =>
    invoke<string>('merge_to_max', { assetId, instanceId, scenePath, projectPath }),
  getMaxMergeStatus: (instanceId: string, requestId: string) =>
    invoke<MaxMergeStatus>('get_max_merge_status', { instanceId, requestId }),
  getPathFacets: (libraryIds?: string[]) => invoke<Record<keyof PathLabels, string[]>>('get_path_facets', { libraryIds }),
  setPathLabels: (id: string, labels: PathLabels | null) => invoke<void>('set_path_labels', { id, labels }),
  setRelatedImage: (id: string, mode: string, imageId?: string) => invoke<void>('set_related_image', { id, mode, imageId }),
  listRelatedImages: (id: string, text = '') => invoke<GroupFile[]>('list_related_images', { id, text }),
  resolveAssetFile: (id: string) => invoke<string>('resolve_asset_file', { id }),
  openAssetExternal: async (asset: Asset) => {
    const filePath = await invoke<string>('resolve_asset_file', { id: asset.id });
    await invoke<void>('open_with_default', { filePath });
  },
  checkAssetAvailability: (id: string) => invoke<Availability>('check_asset_availability', { id }),
  keepOffline: (id: string) => invoke<void>('keep_offline', { id }),
  removeOffline: (id: string) => invoke<void>('remove_offline', { id }),
  getProjectNotes: (collectionId: string) => invoke<Record<string,string>>('get_project_notes', { collectionId }),
  saveProjectNote: (collectionId: string, assetId: string, note: string) => invoke<void>('save_project_note', { collectionId, assetId, note }),
  updateProject: (id: string, description: string) => invoke<void>('update_project', { id, description }),
  findDuplicates: (mode: 'exact' | 'similar', libraryIds?: string[]) => invoke<DuplicateResult>('find_duplicates', { mode, libraryIds }),
  cancelDuplicateScan: () => invoke<void>('cancel_duplicate_scan'),
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

export interface MaxInstance {
  id: string;
  pid: number;
  maxVersion: string;
  scenePath: string;
  projectPath: string;
  busy: boolean;
}

export interface MaxMergeStatus {
  id: string;
  state: 'queued' | 'running' | 'complete' | 'error';
  phase: string;
  completed?: number;
  total?: number;
  importedNodes?: number;
  copiedFiles?: number;
  destinationPath?: string;
  warnings?: string[];
  incompleteMerge?: boolean;
  liveFileReferences?: number;
  relinkedFiles?: number;
  coronaConversion?: { materials: number; lights: number } | null;
}
