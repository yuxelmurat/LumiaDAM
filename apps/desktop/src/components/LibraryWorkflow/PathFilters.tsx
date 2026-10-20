import { t as tr, useTranslation } from '../../i18n';
import { useEffect, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import { api } from '../../api';
import { useAssetStore } from '../../store';
import type { PathLabels } from '../../types';
import styles from './LibraryWorkflow.module.css';

export const emptyLabels: PathLabels = { year: '', source: '', room: '', renderer: '' };
export const labelNames: Record<keyof PathLabels,string> = { year: 'Year', source: 'Source', room: 'Room', renderer: 'Renderer' };

export function PathFilters() {
  useTranslation();
  const { query, assets, setFilter, search } = useAssetStore();
  const [facets, setFacets] = useState<Partial<Record<keyof PathLabels,string[]>>>({});
  const [error, setError] = useState<string | null>(null);
  const [revision, setRevision] = useState(0);
  const scope = JSON.stringify(query.libraryIds ?? (query.libraryId ? [query.libraryId] : []));
  const labelsRevision = JSON.stringify(assets.map(asset => asset.pathLabels));
  useEffect(() => {
    const listener = listen('library:updated', () => setRevision(value => value + 1));
    return () => { void listener.then(stop => stop()); };
  }, []);
  useEffect(() => {
    let stale = false;
    void api.getPathFacets(JSON.parse(scope)).then(values => { if (!stale) { setFacets(values); setError(null); } })
      .catch(reason => { if (!stale) setError(String(reason)); });
    return () => { stale = true; };
  }, [scope, labelsRevision, revision]);
  const selected = query.filter.pathLabels ?? emptyLabels;
  return <div className={styles.filters} aria-label={tr("Folder labels")}>
    <span>{tr("Folder labels")}</span>
    {(Object.keys(labelNames) as (keyof PathLabels)[]).map(field => <label key={field}>
      {tr(labelNames[field])}<select aria-label={tr("Filter by {p0}", { p0: tr(labelNames[field]) })} value={selected[field]} onChange={event => {
        setFilter({ pathLabels: { ...selected, [field]: event.target.value } }); void search();
      }}>
        <option value="">{tr("All")}</option>
        {Array.from(new Set([...(facets[field] ?? []), ...(selected[field] ? [selected[field]] : [])])).map(value => <option key={value}>{value}</option>)}
      </select>
    </label>)}
    {Object.values(selected).some(Boolean) && <button type="button" onClick={() => { setFilter({ pathLabels: undefined }); void search(); }}>{tr("Clear labels")}</button>}
    <label><input type="checkbox" checked={!!query.filter.offlineOnly} onChange={event => { setFilter({ offlineOnly: event.target.checked || undefined }); void search(); }} />{tr("Available offline")}</label>
    {error && <span className={styles.error} role="alert">{tr("Unable to load folder labels.")} <button onClick={() => setRevision(value => value + 1)}>{tr("Retry")}</button></span>}
  </div>;
}
