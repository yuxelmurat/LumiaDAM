import { t as tr, useTranslation, translateMessage } from '../../i18n';
import { memo, useRef, useState, useEffect } from 'react';
import clsx from 'clsx';
import { convertFileSrc } from '@tauri-apps/api/core';
import type { Asset } from '../../types';
import { ThreeViewer } from '../DetailPanel/ThreeViewer';
import { IconStar, IconModel3D, IconImage, IconVideo, IconFileText, IconX } from '../Icons';
import styles from './QuickLookModal.module.css';
import { api } from '../../api';

interface QuickLookModalProps {
  asset: Asset | null;
  isOpen: boolean;
  persistent?: boolean;
  onClose: () => void;
}

const VIDEO_STEPS = [
  { label: '0%', factor: 0.0 },
  { label: '25%', factor: 0.25 },
  { label: '50%', factor: 0.50 },
  { label: '75%', factor: 0.75 },
  { label: '100%', factor: 1.0 },
];

function VideoQuickLook({ filePath, onError }: { filePath: string; onError: () => void }) {
  useTranslation();
  const videoRef = useRef<HTMLVideoElement>(null);
  const [activeStep, setActiveStep] = useState<number>(0);
  const [duration, setDuration] = useState<number>(0);
  const [currentTime, setCurrentTime] = useState<number>(0);

  const handleLoadedMetadata = () => {
    if (videoRef.current) {
      const dur = videoRef.current.duration;
      setDuration(dur);
      videoRef.current.currentTime = 0;
      videoRef.current.play().catch(() => {});
    }
  };

  // Loop through 0%, 25%, 50%, 75%, 100% every 1 second (1000ms)
  useEffect(() => {
    if (!duration || duration <= 0) return;

    let stepIdx = 0;
    const interval = setInterval(() => {
      stepIdx = (stepIdx + 1) % VIDEO_STEPS.length;
      setActiveStep(stepIdx);
      if (videoRef.current) {
        const factor = VIDEO_STEPS[stepIdx].factor;
        const seekTarget = factor === 1.0 ? Math.max(0, duration - 1.0) : duration * factor;
        videoRef.current.currentTime = seekTarget;
        videoRef.current.play().catch(() => {});
      }
    }, 1000);

    return () => clearInterval(interval);
  }, [duration]);

  const handleTimeUpdate = () => {
    if (videoRef.current) {
      setCurrentTime(videoRef.current.currentTime);
    }
  };

  const formatTime = (sec: number): string => {
    const m = Math.floor(sec / 60);
    const s = Math.floor(sec % 60);
    return `${m.toString().padStart(2, '0')}:${s.toString().padStart(2, '0')}`;
  };

  return (
    <div className={styles.videoWrapper}>
      <video
        ref={videoRef}
        src={convertFileSrc(filePath)}
        autoPlay
        muted
        playsInline
        className={styles.largeVideo}
        onLoadedMetadata={handleLoadedMetadata}
        onTimeUpdate={handleTimeUpdate}
        onError={onError}
      />
      <div className={styles.videoTimelineOverlay}>
        <div className={styles.stepPills}>
          {VIDEO_STEPS.map((st, idx) => (
            <div
              key={st.label}
              className={clsx(styles.stepPill, idx === activeStep && styles.stepPillActive)}
              onClick={() => {
                if (videoRef.current && duration > 0) {
                  setActiveStep(idx);
                  const target = st.factor === 1.0 ? Math.max(0, duration - 1.0) : duration * st.factor;
                  videoRef.current.currentTime = target;
                }
              }}
              title={tr("Jump to {p0}", { p0: st.label })}
            >
              <span className={styles.stepDot} />
              <span>{st.label}</span>
            </div>
          ))}
        </div>
        <div className={styles.videoTimeBadge}>
          <span>{formatTime(currentTime)} / {formatTime(duration)}</span>
        </div>
      </div>
    </div>
  );
}

export const QuickLookModal = memo(function QuickLookModal({
  asset,
  isOpen,
  persistent = false,
  onClose,
}: QuickLookModalProps) {
  useTranslation();
  const closeButton = useRef<HTMLButtonElement>(null);
  const [sourcePath, setSourcePath] = useState<string | null>(null);
  const [sourceError, setSourceError] = useState<string | null>(null);
  const [retry, setRetry] = useState(0);
  const [imageFailed, setImageFailed] = useState(false);
  useEffect(() => {
    setSourcePath(null); setSourceError(null); setImageFailed(false);
    if (!isOpen || !asset || !['jpg','jpeg','png','webp','bmp','gif','svg','avif','mp4','webm','mov','mkv','avi'].includes(asset.extension.toLowerCase())) return;
    let cancelled = false;
    void api.resolveAssetFile(asset.id).then(path => { if (!cancelled) setSourcePath(path); })
      .catch(error => { if (!cancelled) setSourceError(String(error)); });
    return () => { cancelled = true; };
  }, [isOpen, asset?.id, asset?.modifiedAt, retry]);
  useEffect(() => {
    if (!isOpen || !persistent) return;
    const previous = document.activeElement as HTMLElement | null;
    closeButton.current?.focus();
    return () => { if (previous?.isConnected) previous.focus(); };
  }, [isOpen, persistent]);
  if (!isOpen || !asset) return null;

  const modelPath =
    (asset.metadata?.preview_model as string) ||
    (asset.thumbnailPath?.endsWith('.glb') ? asset.thumbnailPath : undefined);

  const isVideo =
    asset.kind === 'video' ||
    ['mp4', 'webm', 'mov', 'mkv', 'avi'].includes(asset.extension.toLowerCase());

  const isNativeImg = ['jpg', 'jpeg', 'png', 'webp', 'bmp', 'gif', 'svg', 'avif'].includes(
    asset.extension.toLowerCase()
  );

  const imagePath = isNativeImg
    ? sourcePath ?? asset.thumbnailPath
    : asset.thumbnailPath && !asset.thumbnailPath.endsWith('.glb')
    ? asset.thumbnailPath
    : undefined;

  const formatSize = (bytes: number): string => {
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
    if (bytes < 1024 * 1024 * 1024) return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
    return `${(bytes / 1024 / 1024 / 1024).toFixed(2)} GB`;
  };

  return (
    <div className={styles.overlay} aria-modal="true" role="dialog" aria-label={tr("Preview: {p0}", { p0: asset.fileName })}
      onClick={(event) => { if (event.target === event.currentTarget) onClose(); }}
      onKeyDown={(event) => {
        if (event.key === 'Escape') { event.stopPropagation(); onClose(); }
        if (event.key !== 'Tab' || !persistent) return;
        const buttons = Array.from(event.currentTarget.querySelectorAll<HTMLElement>('button, [tabindex="0"]'));
        const first = buttons[0], last = buttons.at(-1);
        if (event.shiftKey && document.activeElement === first) { event.preventDefault(); last?.focus(); }
        else if (!event.shiftKey && document.activeElement === last) { event.preventDefault(); first?.focus(); }
      }}>
      <div className={styles.container}>
        {/* Top Floating Header */}
        <div className={styles.header}>
          <div className={styles.headerLeft}>
            <span className={styles.extBadge}>.{asset.extension.toUpperCase()}</span>
            <span className={styles.fileName} title={asset.fileName}>
              {asset.fileName}
            </span>
          </div>
          <div className={styles.headerRight}>
            {asset.colorLabel !== 'none' && (
              <span
                className={styles.colorDot}
                style={{ background: `var(--swatch-${asset.colorLabel})` }}
                title={tr("Label: {p0}", { p0: tr(asset.colorLabel) })}
              />
            )}
            {asset.rating > 0 && (
              <div className={styles.ratingStars}>
                {Array.from({ length: asset.rating }).map((_, i) => (
                  <IconStar key={i} size={13} className={styles.starFilled} />
                ))}
              </div>
            )}
            <span className={styles.sizeBadge}>{formatSize(asset.sizeBytes)}</span>
            <button ref={closeButton} type="button" className={styles.closeButton} onClick={onClose} aria-label={tr("Close preview")}><IconX size={18} /></button>
          </div>
        </div>

        {/* Center Main Preview */}
        <div className={styles.previewContent}>
          {modelPath ? (
            <div className={styles.threeWrapper}>
              <ThreeViewer modelPath={modelPath} />
            </div>
          ) : isVideo ? (
            sourcePath ? <VideoQuickLook filePath={sourcePath} onError={() => { setSourcePath(null); setSourceError("The video is unavailable or this format cannot be played by the viewer."); }} /> : <p role="status">{sourceError ?? tr("Opening video…")}</p>
          ) : imagePath && !imageFailed ? (
            <div className={styles.imgWrapper}>
              <img
                src={convertFileSrc(imagePath)}
                alt={asset.fileName}
                className={styles.largeImg}
                onError={() => {
                  if (imagePath === asset.thumbnailPath) setImageFailed(true);
                  setSourcePath(null); setSourceError("The image could not be loaded. Reconnect the source or refresh its offline copy.");
                }}
              />
            </div>
          ) : (
            <div className={styles.placeholder}>
              {['3d_model'].includes(asset.kind) ? (
                <IconModel3D size={64} />
              ) : ['video'].includes(asset.kind) ? (
                <IconVideo size={64} />
              ) : ['document'].includes(asset.kind) ? (
                <IconFileText size={64} />
              ) : (
                <IconImage size={64} />
              )}
              <p className={styles.placeholderExt}>.{asset.extension.toUpperCase()}</p>
              <p className={styles.placeholderText}>{tr("No preview generated yet")}</p>
            </div>
          )}
        </div>

        {/* Bottom Floating Footer / Hint */}
        <div className={styles.footer}>
          {sourceError && <span role="alert">{translateMessage(sourceError)} {asset.thumbnailPath ? tr("Showing cached preview when available.") : ''}
            <button type="button" onClick={() => setRetry(value => value + 1)}>{tr("Retry")}</button></span>}
          <span className={styles.filePath} title={asset.filePath}>
            {asset.filePath}
          </span>
          <div className={styles.hintBadge}>
            <kbd className={styles.kbd}>{persistent ? 'Esc' : 'Space'}</kbd>
            <span>{persistent ? tr("Close preview") : tr("Release to close")}</span>
          </div>
        </div>
      </div>
    </div>
  );
});
