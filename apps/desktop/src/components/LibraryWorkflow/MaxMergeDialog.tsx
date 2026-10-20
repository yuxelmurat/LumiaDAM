import { t as tr, useTranslation, translateMessage } from '../../i18n';
import { useEffect, useRef, useState } from 'react';
import { open as chooseFolder } from '@tauri-apps/plugin-dialog';
import { api, type MaxInstance, type MaxMergeStatus } from '../../api';
import type { Asset } from '../../types';
import styles from './LibraryWorkflow.module.css';

export function MaxMergeDialog({ asset, open, onClose, onBusyChange }: {
  asset: Asset; open: boolean; onClose: () => void; onBusyChange: (busy: boolean) => void;
}) {
  useTranslation();
  const dialog = useRef<HTMLDialogElement>(null);
  const autoStartedAsset = useRef<string | null>(null);
  const [instances, setInstances] = useState<MaxInstance[]>([]);
  const [selected, setSelected] = useState('');
  const [projectPath, setProjectPath] = useState('');
  const [loading, setLoading] = useState(true);
  const [starting, setStarting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [job, setJob] = useState<{ instanceId: string; requestId: string } | null>(null);
  const [status, setStatus] = useState<MaxMergeStatus | null>(null);
  const [refresh, setRefresh] = useState(0);
  const instance = instances.find(value => value.id === selected);
  const busy = loading || starting || !!job && !['complete', 'error'].includes(status?.state ?? 'queued');

  useEffect(() => { onBusyChange(busy); }, [busy, onBusyChange]);
  useEffect(() => {
    if (open) dialog.current?.showModal();
    else dialog.current?.close();
  }, [open]);

  const begin = async (target: MaxInstance, destination: string) => {
    setStarting(true); setError(null);
    try {
      const requestId = await api.mergeAssetToMax(asset.id, target.id, target.scenePath, destination);
      setJob({ instanceId: target.id, requestId });
      setStatus({ id: requestId, state: 'queued', phase: 'Waiting for 3ds Max' });
    } catch (reason) { setError(String(reason)); }
    finally { setStarting(false); }
  };

  useEffect(() => {
    let disposed = false;
    setLoading(true); setJob(null); setStatus(null); setError(null);
    void api.getMaxInstances().then(async found => {
      if (disposed) return;
      setInstances(found);
      const target = found.length === 1 ? found[0] : undefined;
      setSelected(target?.id ?? ''); setProjectPath(target?.projectPath ?? '');
      // Tek açık, kaydedilmiş sahnede bir tıklamayla aktarım başlar.
      const firstAttempt = autoStartedAsset.current !== asset.id;
      autoStartedAsset.current = asset.id;
      if (firstAttempt && target && !target.busy && target.scenePath && target.projectPath) await begin(target, target.projectPath);
    }).catch(reason => { if (!disposed) setError(String(reason)); })
      .finally(() => { if (!disposed) setLoading(false); });
    return () => { disposed = true; };
  }, [asset.id, refresh]);

  useEffect(() => {
    if (!job) return;
    let disposed = false;
    let timer: ReturnType<typeof setTimeout>;
    const poll = async () => {
      try {
        const next = await api.getMaxMergeStatus(job.instanceId, job.requestId);
        if (disposed) return;
        setStatus(next); setError(null);
        if (['complete', 'error'].includes(next.state)) return;
      } catch (reason) { if (!disposed) setError(`Could not read merge status: ${String(reason)}. The operation may still be running in 3ds Max.`); }
      if (!disposed) timer = setTimeout(() => void poll(), 1000);
    };
    void poll();
    return () => { disposed = true; clearTimeout(timer); };
  }, [job]);

  return <dialog ref={dialog} className={styles.dialog} onCancel={event => { event.preventDefault(); onClose(); }}>
    <div className={styles.dialogHeader}><h2>{tr("Merge to 3ds Max")}</h2><button type="button" onClick={onClose}>{busy ? tr("Hide") : tr("Close")}</button></div>
    <p>{asset.fileName}</p>
    {loading && <p role="status">{tr("Finding connected 3ds Max sessions…")}</p>}
    {!loading && instances.length === 0 && <>
      <p>{tr("No connected 3ds Max session.")}</p>
      <p className={styles.hint}>{tr("Run the LumiaDAM installer, enable “Install the 3ds Max plugin”, then start or restart 3ds Max. The plugin supports 3ds Max 2022–2027.")}</p>
    </>}
    {!job && !loading && instances.length > 0 && <div className={styles.section}>
      <select aria-label={tr("Target 3ds Max session")} value={selected} disabled={starting} onChange={event => {
        setSelected(event.target.value); setProjectPath(instances.find(value => value.id === event.target.value)?.projectPath ?? '');
      }}>
        <option value="">{tr("Select a 3ds Max session")}</option>
        {instances.map(value => <option key={value.id} value={value.id}>3ds Max {Number(value.maxVersion) / 1000 + 1998} · {value.scenePath || tr("Unsaved scene")} {tr("· PID")} {value.pid}{value.busy ? tr(' · Busy') : ''}</option>)}
      </select>
      <div className={styles.row}><span className={styles.hint}>{tr("Project folder:")} {projectPath || tr("Choose a project folder")}</span>
        <button type="button" disabled={starting} onClick={() => void chooseFolder({ directory: true, multiple: false, title: tr("Select the destination project folder") }).then(path => {
          if (typeof path === 'string') setProjectPath(path);
        }).catch(reason => setError(String(reason)))}>{tr("Choose folder")}</button></div>
      {instance && !instance.scenePath && <p className={styles.hint}>{tr("The scene has not been saved. Choose where its project assets should be collected.")}</p>}
      <button type="button" disabled={starting || !instance || instance.busy || !projectPath} onClick={() => { if (instance) void begin(instance, projectPath); }}>{starting ? tr("Starting…") : tr("Merge")}</button>
    </div>}
    {status && <div className={styles.progress} role={status.state === 'error' ? 'alert' : 'status'}>
      <span className={status.state === 'error' || status.incompleteMerge ? styles.error : undefined}>{translateMessage(status.phase)}</span>
      {busy && <progress {...(status.total ? { max: status.total, value: status.completed ?? 0 } : {})} />}
      {status.total !== undefined && <span>{status.completed ?? 0} {tr("of")} {status.total}</span>}
      {status.state === 'complete' && <span>{status.importedNodes ?? 0} {tr("objects merged ·")} {status.copiedFiles ?? 0} {tr("project files")}</span>}
      {status.liveFileReferences !== undefined && <span>{status.liveFileReferences} {tr("live texture / IES references verified ·")} {status.relinkedFiles ?? 0} {tr("paths updated")}</span>}
      {status.coronaConversion && <span>{tr("Corona conversion:")} {status.coronaConversion.materials} {tr("materials ·")} {status.coronaConversion.lights} {tr("lights")}</span>}
      {status.incompleteMerge && <p className={styles.error}>{tr("Objects were imported with missing plugins. Materials and lights are incomplete; copied image files cannot restore their missing definitions. Install the required plugins or use a compatible model variant before importing again.")}</p>}
      {status.destinationPath && <p className={styles.hint}>{tr("Project assets:")} {status.destinationPath}</p>}
      {!!status.warnings?.length && <details open={status.state === 'error' || status.incompleteMerge || undefined}><summary>{status.warnings.length} {tr("dependency warnings")}</summary>
        {status.warnings.map((warning, index) => <p key={index} className={styles.error}>{translateMessage(warning)}</p>)}
      </details>}
    </div>}
    {error && <p className={styles.error} role="alert">{translateMessage(error)}</p>}
    {!busy && !loading && <button type="button" onClick={() => setRefresh(value => value + 1)}>{tr("Refresh connection")}</button>}
    <p className={styles.hint}>{tr("Archive contents and available referenced files are copied into the project. Original library files are preserved. Source materials and lights are kept unless you approve Corona conversion in the 3ds Max window. The current renderer, environment, and render elements (including LightMix) are preserved. The scene is not saved automatically.")}</p>
    {busy && <p className={styles.hint}>{tr("You can hide this window and continue browsing. Open “Merge status” to return. Model selection, if needed, appears in 3ds Max.")}</p>}
  </dialog>;
}
