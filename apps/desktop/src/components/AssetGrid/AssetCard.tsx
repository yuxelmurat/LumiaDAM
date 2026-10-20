import { t as tr, useTranslation } from '../../i18n';
import { memo, useCallback, useState, useEffect, useMemo } from 'react';
import clsx from 'clsx';
import { convertFileSrc } from '@tauri-apps/api/core';
import type { Asset, AssetKind } from '../../types';
import { useTagStore, useCollectionStore, useAssetStore } from '../../store';
import { IconModel3D, IconImage, IconVideo, IconStar, IconCollection, IconHeart, IconHeartFilled } from '../Icons';
import styles from './AssetCard.module.css';

export interface AssetCardProps {
  asset: Asset;
  selected: boolean;
  grouped?: boolean;
  onClick: (asset: Asset, e: React.MouseEvent) => void;
}

const KIND_ICON: Record<AssetKind, React.FC<{ size?: number }>> = {
  '3d_model': IconModel3D,
  material: IconImage,
  texture: IconImage,
  hdri: IconImage,
  ies: IconImage,
  video: IconVideo,
  image: IconImage,
  document: IconImage,
  other: IconImage,
};

const KIND_COLOR: Record<AssetKind, string> = {
  '3d_model':  'hsl(220 85% 55%)',
  material:    'hsl(268 72% 60%)',
  texture:     'hsl(178 64% 46%)',
  hdri:        'hsl( 38 92% 56%)',
  ies:         'hsl( 45 95% 50%)',
  video:       'hsl(  4 80% 55%)',
  image:       'hsl(140 64% 46%)',
  document:    'hsl(213 84% 55%)',
  other:       'hsl(220  8% 50%)',
};

export const AssetCard = memo(function AssetCard({
  asset, selected, onClick, grouped = false,
}: AssetCardProps) {
  useTranslation();
  const [imgError, setImgError] = useState(false);
  const [relatedError, setRelatedError] = useState(false);
  const patchAsset = useAssetStore((s) => s.patchAsset);
  const displayImage = grouped && (asset.groupFiles?.length ?? 0) > 1 && asset.relatedImage?.thumbnailPath ? asset.relatedImage.thumbnailPath : asset.thumbnailPath;

  useEffect(() => {
    setImgError(false);
  }, [displayImage, asset.filePath]);

  useEffect(() => {
    setRelatedError(false);
  }, [asset.relatedImage?.filePath, asset.relatedImage?.thumbnailPath]);

  const KindIcon = KIND_ICON[asset.kind] ?? IconImage;
  const kindColor = KIND_COLOR[asset.kind];

  const handleClick = useCallback(
    (e: React.MouseEvent) => {
      e.stopPropagation();
      onClick(asset, e);
    },
    [asset, onClick]
  );

  const handleToggleFavorite = useCallback(
    (e: React.MouseEvent) => {
      e.stopPropagation();
      patchAsset(asset.id, { isFavorite: !asset.isFavorite });
    },
    [asset.id, asset.isFavorite, patchAsset]
  );

  const handleDragStart = useCallback(
    (e: React.DragEvent) => {
      e.dataTransfer.setData('application/opendam-asset-id', asset.id);
      e.dataTransfer.setData('text/plain', asset.id);
      e.dataTransfer.effectAllowed = 'copy';
    },
    [asset.id]
  );

  const tagMap = useTagStore((s) => s.tagMap);
  const collections = useCollectionStore((s) => s.collections);

  const collectionMap = useMemo(() => {
    const map = new Map<string, string>();
    for (const c of collections) {
      map.set(c.id, c.name);
    }
    return map;
  }, [collections]);

  const chips = useMemo(() => {
    const list: { id: string; name: string; type: 'collection' | 'tag'; color?: string }[] = [];
    if (asset.collections && asset.collections.length > 0) {
      for (const colId of asset.collections) {
        const name = collectionMap.get(colId);
        if (name) {
          list.push({ id: `col-${colId}`, name, type: 'collection' });
        }
      }
    }
    if (asset.tags && asset.tags.length > 0) {
      for (const tagId of asset.tags) {
        const tag = tagMap.get(tagId);
        if (tag) {
          list.push({ id: `tag-${tagId}`, name: tag.name, type: 'tag', color: tag.color });
        } else {
          list.push({ id: `tag-${tagId}`, name: tagId, type: 'tag' });
        }
      }
    }
    return list;
  }, [asset.collections, asset.tags, collectionMap, tagMap]);

  return (
    <div
      id={`asset-${asset.id}`}
      data-asset-id={asset.id}
      className={clsx(styles.card, selected && styles.selected)}
      onClick={handleClick}
      draggable
      onDragStart={handleDragStart}
      title={asset.filePath}
      role="button"
      tabIndex={0}
    >
      {/* Thumbnail / preview area */}
      <div className={styles.thumb} data-preview-id={!displayImage ? grouped && asset.relatedImage?.id ? asset.relatedImage.id : asset.id : undefined}
        data-preview-key={`${asset.id}:${asset.modifiedAt}`}>
        {displayImage && !displayImage.endsWith('.glb') && !imgError ? (
          <img
            src={convertFileSrc(displayImage)}
            alt={asset.fileName}
            className={styles.thumbImg}
            loading="lazy"
            onError={() => setImgError(true)}
          />
        ) : (
          <div className={styles.thumbPlaceholder}>
            <KindIcon size={36} />
          </div>
        )}

        {/* Preview status badge */}
        {asset.previewStatus === 'generating' && (
          <div className={clsx(styles.previewBadge, styles.generating)}>
            <span className="animate-pulse">⟳</span>
          </div>
        )}
        {asset.previewStatus === 'error' && (
          <div className={clsx(styles.previewBadge, styles.error)}>!</div>
        )}

        {asset.relatedImage && !(displayImage && displayImage === asset.relatedImage.thumbnailPath) && (
          <div className={styles.relatedImage} title={tr("Related image: {p0}", { p0: asset.relatedImage.fileName })}
            data-preview-id={!asset.relatedImage.thumbnailPath ? asset.relatedImage.id : undefined}
            data-preview-key={`related:${asset.relatedImage.id}:${asset.relatedImage.filePath}`}>
            {asset.relatedImage.thumbnailPath && !relatedError ? <img
              src={convertFileSrc(asset.relatedImage.thumbnailPath)}
              alt={tr("Related image: {p0}", { p0: asset.relatedImage.fileName })}
              loading="lazy"
              onError={() => setRelatedError(true)}
            /> : <div className={styles.relatedPending}><IconImage size={20} /></div>}
            <span>{tr("Related image")}</span>
          </div>
        )}

        {/* Favorite toggle button */}
        <button
          id={`card-fav-${asset.id}`}
          type="button"
          className={clsx(styles.favoriteBtn, asset.isFavorite && styles.favoriteActive)}
          onClick={handleToggleFavorite}
          title={asset.isFavorite ? tr("Remove from favorites") : tr("Add to favorites")}
          aria-label={asset.isFavorite ? tr("Remove from favorites") : tr("Add to favorites")}
        >
          {asset.isFavorite ? (
            <IconHeartFilled size={13} className={styles.favoriteHeartFilled} />
          ) : (
            <IconHeart size={13} />
          )}
        </button>

        {/* Color label stripe */}
        {asset.colorLabel !== 'none' && (
          <div
            className={styles.colorStripe}
            style={{ background: `var(--swatch-${asset.colorLabel})` }}
          />
        )}

        {/* Kind chip */}
        <div className={styles.kindChip} style={{ color: kindColor }}>
          <KindIcon size={10} />
          <span>{asset.extension.toUpperCase()}</span>
        </div>
      </div>

      {/* Card body */}
      <div className={styles.body}>
        <p className={clsx(styles.name, 'truncate')} title={asset.filePath}>
          {asset.fileName}
        </p>
        <p className={styles.filePath}>{asset.filePath}</p>
        <div className={styles.meta}>
          {grouped && (asset.groupFiles?.length ?? 0) > 1 && <span title={tr("Open the Workflow tab to use the model, archive or image")}>{asset.groupFiles!.length} {tr("files")}</span>}
          {asset.offlinePath && <span title={asset.offlinePath}>{asset.offlineStale ? tr("Offline copy outdated") : tr("Offline")}</span>}
          {asset.rating > 0 && (
            <div className={styles.stars}>
              {Array.from({ length: 5 }).map((_, i) => (
                <IconStar
                  key={i}
                  size={10}
                  className={clsx(styles.star, i < asset.rating && styles.starFilled)}
                />
              ))}
            </div>
          )}
          <span className={styles.size}>{formatSize(asset.sizeBytes)}</span>
        </div>

        {/* Scrollable tags and collections chips on bottom */}
        {chips.length > 0 && (
          <div
            className={styles.chipsScroll}
            onWheel={(e) => {
              if (e.deltaY !== 0) {
                e.currentTarget.scrollLeft += e.deltaY;
              }
            }}
          >
            {chips.map((chip) => (
              <span
                key={chip.id}
                className={clsx(
                  styles.chip,
                  chip.type === 'collection' ? styles.collectionChip : styles.tagChip
                )}
                title={chip.type === 'collection' ? tr("Collection: {p0}", { p0: chip.name }) : `Tag: ${chip.name}`}
              >
                {chip.type === 'collection' ? (
                  <IconCollection size={9} className={styles.chipIcon} />
                ) : (
                  <span
                    className={styles.chipDot}
                    style={{ background: chip.color ? `var(--swatch-${chip.color})` : 'hsl(220 85% 60%)' }}
                  />
                )}
                <span className={styles.chipText}>{chip.name}</span>
              </span>
            ))}
          </div>
        )}
      </div>
    </div>
  );
});

function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes}B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)}KB`;
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / 1024 / 1024).toFixed(1)}MB`;
  return `${(bytes / 1024 / 1024 / 1024).toFixed(2)}GB`;
}
