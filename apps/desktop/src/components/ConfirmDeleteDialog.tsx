import { t as tr, useTranslation } from '../i18n';
import { useState, useEffect, useRef } from 'react';
import clsx from 'clsx';
import { IconTrash, IconX } from './Icons';
import styles from './ConfirmDeleteDialog.module.css';

interface ConfirmDeleteDialogProps {
  isOpen: boolean;
  itemType: 'tag' | 'collection' | 'library' | 'asset' | 'assets';
  itemName: string;
  onConfirm: () => void;
  onCancel: () => void;
}

export function ConfirmDeleteDialog({
  isOpen,
  itemType,
  itemName,
  onConfirm,
  onCancel,
}: ConfirmDeleteDialogProps) {
  useTranslation();
  const [typedText, setTypedText] = useState('');
  const inputRef = useRef<HTMLInputElement>(null);
  const confirmBtnRef = useRef<HTMLButtonElement>(null);

  const isAsset = itemType === 'asset' || itemType === 'assets';
  const isLibrary = itemType === 'library';
  const actionTitle = tr({ tag: 'Delete tag', collection: 'Delete collection', library: 'Remove library', asset: 'Delete file', assets: 'Delete files' }[itemType]);
  const confirmationWord = tr('confirm');

  useEffect(() => {
    if (isOpen) {
      setTypedText('');
      setTimeout(() => {
        if (isAsset) {
          confirmBtnRef.current?.focus();
        } else {
          inputRef.current?.focus();
        }
      }, 50);
    }
  }, [isOpen, isAsset]);

  if (!isOpen) return null;

  const normalized = typedText.trim().toLowerCase();
  const isMatched = isAsset || normalized === confirmationWord || normalized === `(${confirmationWord})`;

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    if (isMatched) {
      onConfirm();
    }
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Escape') {
      onCancel();
    }
  };

  return (
    <div className={styles.overlay} onClick={onCancel} onKeyDown={handleKeyDown}>
      <div
        className={styles.dialog}
        onClick={(e) => e.stopPropagation()}
        role="dialog"
        aria-modal="true"
      >
        <div className={styles.header}>
          <div className={styles.titleArea}>
            <div className={styles.iconCircle}>
              <IconTrash size={18} />
            </div>
            <h3 className={styles.title}>{actionTitle}</h3>
          </div>
          <button className={styles.closeBtn} onClick={onCancel} title={tr("Close")}>
            <IconX size={14} />
          </button>
        </div>

        <div className={styles.content}>
          <p className={styles.warningText}>
            {tr(isLibrary ? 'Remove this library?' : 'Delete the selected item(s)?')}
            {' '}<strong className={styles.targetName}>"{itemName}"</strong>
          </p>
          {isLibrary ? (
            <p className={styles.subText}>
              {tr("This will remove the library from LumiaDAM.")} <strong style={{ color: 'var(--color-text-main, #ffffff)' }}>{tr("Your original files on disk will NOT be deleted.")}</strong>
            </p>
          ) : isAsset ? (
            <p className={styles.subText}>
              {tr('Selected files will be moved to the Recycle Bin and removed from LumiaDAM.')}
            </p>
          ) : (
            <p className={styles.subText}>
              {tr('This action cannot be undone.')}
            </p>
          )}

          <form onSubmit={handleSubmit} className={styles.form}>
            {!isAsset && (
              <input
                ref={inputRef}
                type="text"
                className={styles.input}
                aria-label={tr('Type "{word}" to confirm', { word: confirmationWord })}
                placeholder={tr('Type "{word}" to confirm', { word: confirmationWord })}
                value={typedText}
                onChange={(e) => setTypedText(e.target.value)}
                autoComplete="off"
                spellCheck={false}
              />
            )}

            <div className={styles.actions}>
              <button
                type="button"
                className={clsx(styles.btn, styles.cancelBtn)}
                onClick={onCancel}
              >
                {tr("Cancel")} </button>
              <button
                ref={confirmBtnRef}
                type="submit"
                disabled={!isMatched}
                className={clsx(
                  styles.btn,
                  styles.deleteBtn,
                  isMatched && styles.deleteBtnActive
                )}
              >
                {actionTitle}
              </button>
            </div>
          </form>
        </div>
      </div>
    </div>
  );
}

