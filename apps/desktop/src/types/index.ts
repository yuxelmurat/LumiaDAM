// ── Asset types ─────────────────────────────────────────────────────────
export type AssetKind =
  | '3d_model'
  | 'material'
  | 'texture'
  | 'hdri'
  | 'ies'
  | 'video'
  | 'image'
  | 'document'
  | 'other';

export type ColorLabel =
  | 'none'
  | 'red'
  | 'orange'
  | 'yellow'
  | 'green'
  | 'teal'
  | 'blue'
  | 'purple'
  | 'pink';

export interface Tag {
  id: string;
  name: string;
  color?: ColorLabel;
  parentId?: string;
}

export interface AssetMeta {
  /** Format-specific extracted metadata (renderer, poly count, resolution…) */
  [key: string]: unknown;
}

export interface Asset {
  libraryId?: string;
  pathLabels?: PathLabels;
  inferredLabels?: PathLabels;
  labelOverrides?: Partial<PathLabels>;
  relatedMode?: 'auto' | 'pinned' | 'none';
  groupFiles?: GroupFile[];
  offlinePath?: string;
  offlineStale?: boolean;
  id: string;
  /** Absolute path on the local filesystem */
  filePath: string;
  fileName: string;
  /** File extension, lowercase, no dot */
  extension: string;
  kind: AssetKind;
  /** Size in bytes */
  sizeBytes: number;
  /** Last-modified timestamp (ms) */
  modifiedAt: number;
  /** When it was added to the library */
  indexedAt: number;
  /** Star rating 0–5 */
  rating: number;
  colorLabel: ColorLabel;
  isFavorite: boolean;
  description: string;
  tags: string[];       // tag IDs
  collections?: string[]; // collection IDs
  /** Relative path to cached thumbnail (inside DAM cache dir) */
  thumbnailPath?: string;
  /** Aynı klasördeki eşleşen görsel; dosyanın kendi önizlemesi değildir. */
  relatedImage?: { id?: string; filePath: string; fileName: string; thumbnailPath?: string };
  /** Whether a preview job is pending/running */
  previewStatus: 'none' | 'pending' | 'generating' | 'done' | 'error';
  metadata: AssetMeta;
}

export interface Library {
  id: string;
  name: string;
  /** Root paths this library watches */
  rootPaths: string[];
  createdAt: number;
  assetCount: number;
}

export interface Collection {
  id: string;
  name: string;
  description: string;
  parentId?: string;
  assetIds: string[];
  isSmart: boolean;
  /** JSON-serialized filter spec for smart collections */
  filterSpec?: string;
  createdAt: number;
}

// ── Search & Filter ──────────────────────────────────────────────────────
export type SortField = 'name' | 'date' | 'size' | 'rating' | 'kind' | 'favorite';
export type SortOrder = 'asc' | 'desc';

export interface FilterSpec {
  pathLabels?: PathLabels;
  offlineOnly?: boolean;
  kinds?: AssetKind[];
  tags?: string[];
  rating?: { min: number; max: number };
  colorLabels?: ColorLabel[];
  isFavorite?: boolean;
  dateRange?: { from?: number; to?: number };
  sizeRange?: { min?: number; max?: number };
  extensions?: string[];
  previewStatus?: Asset['previewStatus'];
  unorganized?: boolean;
  directory?: string;
  namePrefix?: string;
  nameSuffix?: string;
  tagLogic?: 'and' | 'or' | 'and_or' | 'none';
  collectionId?: string;
  collectionIds?: string[];
  collectionLogic?: 'and' | 'or' | 'and_or' | 'none';
  includeSubcollections?: boolean;
}

export interface SearchQuery {
  groupRelated?: boolean;
  folderPath?: string;
  folderPage?: number;
  text: string;
  filter: FilterSpec;
  sort: { field: SortField; order: SortOrder };
  libraryId?: string;
  libraryIds?: string[];
  collectionId?: string;
  includeSubcollections?: boolean;
}

// ── Preview job ──────────────────────────────────────────────────────────
export interface PreviewJob {
  id: string;
  assetId: string;
  status: 'queued' | 'running' | 'done' | 'error';
  progress?: number; // 0–100
  errorMessage?: string;
  createdAt: number;
}

// ── IPC / Command interfaces (Tauri invoke payloads) ─────────────────────
export interface AddLibraryPayload {
  name: string;
  rootPaths: string[];
}

export interface SearchPayload {
  resultScope?: 'files' | 'folders';
  query: SearchQuery;
  page: number;
  pageSize: number;
}

export interface FolderResult {
  libraryId: string;
  path: string;
  fullPath: string;
  name: string;
}

export interface SearchResult {
  folders: FolderResult[];
  folderTotal: number;
  assets: Asset[];
  total: number;
  page: number;
  pageSize: number;
}

export interface PathLabels { year: string; source: string; room: string; renderer: string }
export interface GroupFile { id: string; fileName: string; filePath: string; extension: string; kind: string; thumbnailPath?: string }
export interface Availability { sourceAvailable: boolean; offlineAvailable: boolean; offlineStale: boolean }
export interface DuplicateFile { id: string; libraryId: string; fileName: string; filePath: string; sizeBytes: number; thumbnailPath?: string }
export interface DuplicateGroup { kind: 'exact' | 'similar'; files: DuplicateFile[]; distance?: number }
export interface DuplicateResult { groups: DuplicateGroup[]; groupCount: number; processed: number; total: number; skipped: number; cancelled: boolean; limited: boolean }
export interface DuplicateProgress { processed: number; total: number; skipped: number; phase: string }
