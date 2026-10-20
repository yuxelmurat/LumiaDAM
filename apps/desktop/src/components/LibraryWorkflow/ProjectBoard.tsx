import { t as tr, useTranslation, translateMessage } from '../../i18n';
import { useEffect, useState } from 'react';
import { api } from '../../api';
import { useAssetStore, useCollectionStore, useUIStore } from '../../store';
import { AssetCard } from '../AssetGrid/AssetCard';
import type { Asset, Collection } from '../../types';
import styles from './LibraryWorkflow.module.css';

function ProjectNote({ project, asset, note }: { project: string; asset: string; note: string }) {
  useTranslation();
  const key = `lumiadam-project-note:${project}:${asset}`;
  const [draft, setDraft] = useState(() => { try { return localStorage.getItem(key) ?? note; } catch { return note; } });
  const [saved, setSaved] = useState(note);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const save = async () => {
    setSaving(true); setError(null);
    try {
      await api.saveProjectNote(project, asset, draft);
      setSaved(draft); try { localStorage.removeItem(key); } catch { /* Katalog kaydı tamamlandı. */ }
    } catch (reason) { setError(String(reason)); }
    finally { setSaving(false); }
  };
  return <div className={styles.note}>
    <textarea aria-label={tr("Project note for this asset")} placeholder={tr("Material, dimensions, client feedback…")} value={draft} disabled={saving} maxLength={5000}
      onChange={event => {
        setDraft(event.target.value);
        try { localStorage.setItem(key, event.target.value); } catch { setError("The local draft could not be stored. Save the note before leaving this board."); }
      }} />
    <div className={styles.row}><button type="button" disabled={saving || draft === saved} onClick={() => void save()}>{saving ? tr("Saving…") : tr("Save note")}</button>
      <span className={styles.hint}>{draft === saved ? tr("Saved in project") : tr("Unsaved note")}</span></div>
    {error && <p role="alert" className={styles.error}>{translateMessage(error)}</p>}
  </div>;
}

export function ProjectBoard({ project, assets, onClick, grouped }: { project: Collection; assets: Asset[]; grouped?: boolean; onClick: (asset: Asset, event: React.MouseEvent) => void }) {
  useTranslation();
  const selected = useUIStore(s => s.selectedAssetIds);
  const [notes, setNotes] = useState<Record<string,string>>({});
  const [loading, setLoading] = useState(true);
  const briefKey = `lumiadam-project-brief:${project.id}`;
  const [description, setDescription] = useState(() => { try { return localStorage.getItem(briefKey) ?? project.description; } catch { return project.description; } });
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [reload, setReload] = useState(0);
  useEffect(() => {
    let stale = false; setLoading(true); setError(null);
    void api.getProjectNotes(project.id).then(values => { if (!stale) setNotes(values); })
      .catch(reason => { if (!stale) setError(String(reason)); }).finally(() => { if (!stale) setLoading(false); });
    return () => { stale = true; };
  }, [project.id, reload]);
  const saveDescription = async () => {
    setSaving(true); setError(null);
    try {
      await api.updateProject(project.id, description); await useCollectionStore.getState().fetchCollections();
      try { localStorage.removeItem(briefKey); } catch { /* Proje kaydı tamamlandı. */ }
    }
    catch (reason) { setError(String(reason)); } finally { setSaving(false); }
  };
  return <section className={styles.board} aria-label={tr("Project board: {p0}", { p0: project.name })}>
    <div className={styles.boardHeader}>
      <h2>{project.name}</h2><p className={styles.hint}>{tr("References stay in their libraries. Each project has its own notes for the same asset.")}</p>
      <textarea aria-label={tr("Project brief")} placeholder={tr("Project brief…")} value={description} maxLength={5000} disabled={saving} onChange={event => {
        setDescription(event.target.value);
        try { localStorage.setItem(briefKey,event.target.value); } catch { setError("The local draft could not be stored. Save the project brief before leaving."); }
      }} />
      <div><button type="button" disabled={saving || description === project.description} onClick={() => void saveDescription()}>{tr("Save project brief")}</button></div>
      {error && <p role="alert" className={styles.error}>{translateMessage(error)} <button onClick={() => setReload(value => value + 1)}>{tr("Retry loading notes")}</button></p>}
    </div>
    {!loading && assets.length === 0 && <p className={styles.hint}>{tr("Add references by dragging library files onto this project in the sidebar, or assign them in the Collections tab.")}</p>}
    {loading ? <p role="status">{tr("Loading project notes…")}</p> : <div className={styles.boardGrid}>
      {assets.map(asset => <article key={asset.id}>
        <AssetCard asset={asset} grouped={grouped} selected={selected.has(asset.id)} onClick={onClick} />
        <ProjectNote key={`${project.id}:${asset.id}`} project={project.id} asset={asset.id} note={notes[asset.id] ?? ''} />
        <button type="button" onClick={() => void useCollectionStore.getState().removeFromCollection(project.id, [asset.id])
          .then(() => useAssetStore.getState().search()).catch(reason => setError(String(reason)))}>{tr("Remove reference from project")}</button>
      </article>)}
    </div>}
  </section>;
}
