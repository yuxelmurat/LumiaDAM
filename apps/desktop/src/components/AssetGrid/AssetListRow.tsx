import { t as tr, useTranslation, getLanguage } from '../../i18n';
import { memo, useCallback, useEffect, useState } from 'react';
import clsx from 'clsx';
import { convertFileSrc } from '@tauri-apps/api/core';
import type { Asset } from '../../types';
import { useAssetStore } from '../../store';
import { IconImage, IconVideo, IconStar, IconHeart, IconHeartFilled } from '../Icons';
import styles from './AssetListRow.module.css';

interface AssetListRowProps {
  asset: Asset;
  selected: boolean;
  grouped?: boolean;
  onClick: (asset: Asset, e: React.MouseEvent) => void;
}

function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes}B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)}KB`;
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / 1024 / 1024).toFixed(1)}MB`;
  return `${(bytes / 1024 / 1024 / 1024).toFixed(2)}GB`;
}

function formatDate(ms: number): string {
  const d = new Date(ms);
  return d.toLocaleDateString(getLanguage(), { year: 'numeric', month: 'short', day: 'numeric' });
}

export const AssetListRow = memo(function AssetListRow({
  asset, selected, onClick, grouped = false,
}: AssetListRowProps) {
  useTranslation();
  const patchAsset = useAssetStore((s) => s.patchAsset);
  const [relatedError, setRelatedError] = useState(false);
  const imagePath = grouped && (asset.groupFiles?.length ?? 0) > 1 ? asset.relatedImage?.thumbnailPath ?? asset.thumbnailPath : asset.thumbnailPath;
  useEffect(() => setRelatedError(false), [asset.relatedImage?.filePath, asset.relatedImage?.thumbnailPath]);

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
      e.dataTransfer.setData('text/plain', asset.filePath);
      e.dataTransfer.effectAllowed = 'copy';
    },
    [asset.filePath]
  );

  return (
    <div
      id={`asset-row-${asset.id}`}
      data-asset-id={asset.id}
      className={clsx(styles.row, selected && styles.selected)}
      onClick={handleClick}
      draggable
      onDragStart={handleDragStart}
      role="button"
      tabIndex={0}
    >
      {/* Favorite Button */}
      <button
        id={`row-fav-${asset.id}`}
        type="button"
        className={clsx(styles.favoriteBtn, asset.isFavorite && styles.favoriteActive)}
        onClick={handleToggleFavorite}
        title={asset.isFavorite ? tr("Remove from favorites") : tr("Add to favorites")}
        aria-label={asset.isFavorite ? tr("Remove from favorites") : tr("Add to favorites")}
      >
        {asset.isFavorite ? (
          <IconHeartFilled size={14} className={styles.favoriteHeartFilled} />
        ) : (
          <IconHeart size={14} />
        )}
      </button>

      {/* Thumbnail */}
      <div className={styles.thumb} data-preview-id={!imagePath ? grouped && asset.relatedImage?.id ? asset.relatedImage.id : asset.id : undefined}
        data-preview-key={`${asset.id}:${asset.modifiedAt}`}>
        {imagePath && !imagePath.endsWith('.glb') ? (
          <img src={convertFileSrc(imagePath)} alt={asset.fileName} className={styles.thumbImg} loading="lazy" />
        ) : (
          <div className={styles.thumbPlaceholder}>
            {asset.kind === 'video' ? <IconVideo size={14} /> : <IconImage size={14} />}
          </div>
        )}
      </div>

      {asset.relatedImage && !(imagePath && imagePath === asset.relatedImage.thumbnailPath) && (
        <div className={styles.relatedImage} title={tr("Related image: {p0}", { p0: asset.relatedImage.fileName })}
          data-preview-id={!asset.relatedImage.thumbnailPath ? asset.relatedImage.id : undefined}
          data-preview-key={`related:${asset.relatedImage.id}:${asset.relatedImage.filePath}`}>
          {asset.relatedImage.thumbnailPath && !relatedError ? <img
            src={convertFileSrc(asset.relatedImage.thumbnailPath)}
            alt={tr("Related image: {p0}", { p0: asset.relatedImage.fileName })}
            loading="lazy"
            onError={() => setRelatedError(true)}
          /> : <div className={styles.thumbPlaceholder}><IconImage size={14} /></div>}
        </div>
      )}

      {/* Color label */}
      {asset.colorLabel !== 'none' && (
        <div
          className={styles.colorDot}
          style={{ background: `var(--swatch-${asset.colorLabel})` }}
        />
      )}

      {/* Name */}
      <div className={styles.nameCol}>
        <span className={clsx(styles.name, 'truncate')}>{asset.fileName}</span>
        <span className={styles.path}>{asset.filePath}</span>
        {grouped && (asset.groupFiles?.length ?? 0) > 1 && <small>{asset.groupFiles!.length} {tr("files")}</small>}
        {asset.offlinePath && <small>{asset.offlineStale ? tr("Offline copy outdated") : tr("Offline")}</small>}
      </div>

      {/* Extension */}
      <span className={styles.ext}>{asset.extension.toUpperCase()}</span>

      {/* Rating */}
      <div className={styles.stars}>
        {Array.from({ length: 5 }).map((_, i) => (
          <IconStar key={i} size={10} className={clsx(styles.star, i < asset.rating && styles.starFilled)} />
        ))}
      </div>

      {/* Size */}
      <span className={styles.size}>{formatSize(asset.sizeBytes)}</span>

      {/* Date */}
      <span className={styles.date}>{formatDate(asset.modifiedAt)}</span>
    </div>
  );
});
