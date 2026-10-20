import { t as tr, useTranslation, formatNumber } from '../../i18n';
import { useAssetStore, useLibraryStore } from '../../store';
import { IconFolder } from '../Icons';
import styles from './AssetGrid.module.css';

export function FolderNavigation() {
  useTranslation();
  const { query, openFolder, backFromFolder } = useAssetStore();
  const library = useLibraryStore((s) => s.libraries.find((lib) => lib.id === query.libraryId));
  if (query.folderPath === undefined) return null;
  const root = library?.rootPaths[0]?.replace(/[/\\]+$/, '') ?? '';
  const fullPath = `${root}/${query.folderPath}`;
  const parent = query.folderPath.includes('/') ? query.folderPath.slice(0, query.folderPath.lastIndexOf('/')) : '';
  return (
    <nav className={styles.folderNavigation} aria-label={tr("Folder navigation")}>
      <button type="button" onClick={() => backFromFolder()}>{tr("Back")}</button>
      <button type="button" disabled={!query.folderPath} onClick={() => openFolder({
        libraryId: query.libraryId!, path: parent, fullPath: `${root}/${parent}`, name: parent.split('/').pop() || library?.name || '',
      })}>{tr("Up one level")}</button>
      <span className={styles.fullPath}>{fullPath}</span>
    </nav>
  );
}

export function FolderResults() {
  useTranslation();
  const { folders, folderTotal, query, pageSize, setFolderPage, searchFolders, openFolder } = useAssetStore();
  if (!folderTotal) return null;
  const page = query.folderPage ?? 0;
  const changePage = (folderPage: number) => { setFolderPage(folderPage); void searchFolders(); };
  return (
    <section className={styles.folderResults} aria-label={tr("Folder results")}>
      <h2>{tr("Folders (")}{formatNumber(folderTotal)})</h2>
      {folders.map((folder) => (
        <button type="button" className={styles.folderRow} key={`${folder.libraryId}:${folder.path}`}
          onClick={() => openFolder(folder)}>
          <IconFolder size={24} />
          <span><strong>{folder.name}</strong><span className={styles.fullPath}>{folder.fullPath}</span></span>
        </button>
      ))}
      {folderTotal > pageSize && <div className={styles.folderNavigation}>
        <button type="button" disabled={page === 0} onClick={() => changePage(page - 1)}>{tr("Previous folders")}</button>
        <span>{page * pageSize + 1}–{Math.min((page + 1) * pageSize, folderTotal)} {tr("of")} {folderTotal}</span>
        <button type="button" disabled={(page + 1) * pageSize >= folderTotal} onClick={() => changePage(page + 1)}>{tr("Next folders")}</button>
      </div>}
    </section>
  );
}
