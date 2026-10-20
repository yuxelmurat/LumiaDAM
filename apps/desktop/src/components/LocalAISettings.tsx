import { useEffect, useRef, useState } from 'react';
import { convertFileSrc } from '@tauri-apps/api/core';
import { api, type AiLog, type AiOverview, type AiSettings, type AiStatus, type AiSuggestion } from '../api';
import { useCollectionStore } from '../store';
import { formatDate, formatNumber, translateMessage, useTranslation } from '../i18n';
import styles from './LocalAISettings.module.css';

const modes: { value: AiSettings['mode']; title: string; description: string }[] = [
  { value: 'disabled', title: 'Disabled', description: 'No runs, automatic analysis, or model startup. Recommended default.' },
  { value: 'manual', title: 'Manual', description: 'Run only when you choose Run now. Analysis temporarily uses RAM.' },
  { value: 'idle', title: 'When idle', description: 'Start after inactivity and a low CPU/GPU load check. Pause on user input; analysis uses RAM.' },
  { value: 'scheduled', title: 'Scheduled', description: 'Run within a daily time window while LumiaDAM is open. Analysis may use RAM during that window.' },
];

export function LocalAISettings() {
  const { t } = useTranslation();
  const [view, setView] = useState('overview');
  const [log, setLog] = useState<AiLog | null>(null);
  const [logError, setLogError] = useState<string | null>(null);
  const [overview, setOverview] = useState<AiOverview | null>(null);
  const [settings, setSettings] = useState<AiSettings | null>(null);
  const [status, setStatus] = useState<AiStatus | null>(null);
  const [suggestions, setSuggestions] = useState<AiSuggestion[]>([]);
  const [expanded, setExpanded] = useState<Record<string, boolean>>({});
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [refresh, setRefresh] = useState(0);
  const mounted = useRef(false);
  const actionId = useRef(0);

  useEffect(() => {
    mounted.current = true;
    api.getLocalAiSettings().then(value => {
      if (mounted.current) {
        setOverview(value); setSettings(value.settings);
        if (!value.settings.setupComplete) setView('model');
      }
    }).catch(reason => { if (mounted.current) setError(String(reason)); });
    return () => { mounted.current = false; };
  }, []);

  const savedMode = overview?.settings.mode;
  const preparing = busy === 'prepare';
  useEffect(() => {
    if (!savedMode) return;
    let active = true;
    let timer: number | undefined;
    let suggestionCount: number | undefined;
    const read = async () => {
      try {
        const value = await api.getLocalAiStatus();
        if (!active) return;
        setStatus(value);
        if (suggestionCount !== value.suggestions) {
          const pending = await api.listAiSuggestions();
          if (!active) return;
          setSuggestions(pending);
          suggestionCount = value.suggestions;
        }
        // Kapalı panelde sorgu yok; kapalı ve manuel mod boşta timer oluşturmaz.
        if (active && (value.running || preparing || savedMode === 'idle' || savedMode === 'scheduled')) {
          timer = window.setTimeout(() => { void read(); }, 2500);
        }
      } catch (reason) { if (active) setError(String(reason)); }
    };
    void read();
    return () => { active = false; if (timer !== undefined) window.clearTimeout(timer); };
  }, [savedMode, refresh, preparing]);

  useEffect(() => {
    if (view !== 'log') return;
    let active = true;
    api.getLocalAiLog().then(value => {
      if (active) { setLog(value); setLogError(null); }
    }).catch(reason => { if (active) setLogError(String(reason)); });
    return () => { active = false; };
  }, [view, refresh, status?.processed, status?.state]);

  const perform = async (action: string, run: () => Promise<unknown>, success?: string | ((result: unknown) => string)) => {
    const request = ++actionId.current;
    setBusy(action); setError(null); setNotice(null);
    try {
      const result = await run();
      if (mounted.current && request === actionId.current) {
        setNotice(typeof success === 'function' ? success(result) : success ?? null);
        setRefresh(value => value + 1);
      }
    } catch (reason) { if (mounted.current && request === actionId.current) setError(String(reason)); }
    finally { if (mounted.current && request === actionId.current) setBusy(null); }
  };

  if (!settings || !overview) return <div className={styles.panel}>
    {error ? <p role="alert">{translateMessage(error)}</p> : <p role="status">{t('Loading Local AI settings…')}</p>}
  </div>;

  const update = (patch: Partial<AiSettings>) => setSettings(current => current ? { ...current, ...patch } : current);
  const dirty = JSON.stringify(settings) !== JSON.stringify(overview.settings);
  let validation: string | null = null;
  try {
    const endpoint = new URL(settings.endpoint);
    if (endpoint.protocol !== 'http:' ||
      (!['localhost', '[::1]'].includes(endpoint.hostname) && !/^127(?:\.\d+){3}$/.test(endpoint.hostname)) ||
      endpoint.username || endpoint.password || endpoint.port === '0' ||
      endpoint.search || endpoint.hash || endpoint.pathname !== '/') {
      validation = 'Use a loopback HTTP address without credentials or an API path, such as http://127.0.0.1:1234.';
    }
  } catch { validation = 'Enter a valid local server address.'; }
  if (!settings.model.trim()) validation = 'Enter the model identifier loaded in LM Studio.';
  if (settings.mode === 'scheduled' && (settings.startHour === settings.endHour ||
    ![settings.startHour, settings.endHour].every(hour => Number.isInteger(hour) && hour >= 0 && hour <= 23))) {
    validation = 'Choose different start and end hours between 00:00 and 23:00.';
  }
  if (!Number.isInteger(settings.idleMinutes) || settings.idleMinutes < 1 || settings.idleMinutes > 240) validation = 'Idle time must be between 1 and 240 minutes.';
  if (!Number.isInteger(settings.batchSize) || settings.batchSize < 1 || settings.batchSize > 1000) validation = 'Batch size must be between 1 and 1,000 assets.';
  const save = () => perform('save', async () => {
    if (validation) throw new Error(validation);
    const value = await api.saveLocalAiSettings({ ...settings, setupComplete: true });
    if (mounted.current) { setOverview(value); setSettings(value.settings); }
  }, 'Settings saved.');
  const stop = () => perform('stop', async () => {
    await api.stopLocalAi();
    const value = await api.getLocalAiSettings();
    if (mounted.current) { setOverview(value); setSettings(value.settings); }
  }, 'Analysis stopped and automatic runs paused. Choose When idle or Scheduled and save settings to resume automatic runs.');
  const review = (suggestion: AiSuggestion, approve: boolean) => perform(suggestion.id, async () => {
    if (approve) {
      await api.applyAiSuggestion(suggestion.id);
      await useCollectionStore.getState().fetchCollections();
    } else await api.dismissAiSuggestion(suggestion.id);
    const pending = await api.listAiSuggestions();
    if (mounted.current) setSuggestions(pending);
  }, approve ? `Added the reviewed files to the ${suggestion.collectionName ?? 'review'} collection. Source files stayed in place.` : 'Suggestion dismissed.');
  const approveAll = () => perform('approve-all', async () => {
    const result = await api.applyAllAiSuggestions();
    if (result.approved > 0) await useCollectionStore.getState().fetchCollections();
    const pending = await api.listAiSuggestions();
    if (mounted.current) setSuggestions(pending);
    return result;
  }, value => {
    const result = value as { approved: number; failed: number };
    return t('{approved} suggestions approved; {failed} could not be added.', {
      approved: formatNumber(result.approved), failed: formatNumber(result.failed),
    });
  });

  return <section className={styles.panel} aria-labelledby="local-ai-heading">
    <div className={styles.heading}>
      <div><span className={styles.eyebrow}>{t("LUMIADAM / LOCAL AI")}</span>
        <h1 id="local-ai-heading">{t("Local AI organizer")}</h1>
        <p>{t("Analyze local previews, review suggestions, and group files into collections.")}</p>
      </div>
      <span className={styles.badge}>{t(status?.running ? 'Working' : overview.settings.mode === 'disabled' ? 'Disabled' : 'Standby')}</span>
    </div>
    <nav className={styles.tabs} aria-label={t("Local AI sections")}>
      {Object.entries({ overview: 'Overview', model: 'Model & schedule', suggestions: 'Suggestions', log: 'Activity log' }).map(([id, label]) =>
        <button type="button" key={id} aria-pressed={view === id} onClick={() => setView(id)}>{t(label)}</button>)}
    </nav>
    {status && <div className={styles.status} role="status" aria-live="polite">
      <strong>{translateMessage(status.phase || status.state)}</strong>
      <span>{t(status.state === 'downloading' ? 'This batch: {processed} / {total} MiB downloaded · {count} suggestions' : 'This batch: {processed} / {total} assets · {count} suggestions', {
        processed: formatNumber(status.processed), total: formatNumber(status.total), count: formatNumber(suggestions.filter(suggestion => suggestion.status === 'pending').length),
      })}</span>
      {status.running && status.total > 0 && <progress value={Math.min(status.processed, status.total)} max={status.total}
        aria-label={t(status.state === 'downloading' ? 'Model download progress' : 'Local AI analysis progress')} />}
      {overview.settings.provider === 'bundled' ?
        <span>{t('LumiaDAM runtime: {runtime} · Model loaded by LumiaDAM: {loaded}', { runtime: t(status.engineRunning ? 'running' : 'stopped'), loaded: t(status.modelLoaded ? 'yes' : 'no') })}</span> :
        <span>{t('Managed by {provider} · LumiaDAM job: {state}', { provider: overview.settings.provider === 'lmstudio' ? 'LM Studio' : 'Ollama', state: t(status.running ? 'running' : 'stopped') })}</span>}
      {status.error && <span className={styles.error} role="alert">{translateMessage(status.error)}</span>}
    </div>}
    <div className={styles.actions}>
      <button type="button" className={styles.primary} disabled={!!busy || dirty || settings.mode === 'disabled' || !overview.settings.setupComplete || !!validation || !!status?.running}
        onClick={() => perform('run', api.startLocalAi)}>{t("Run now")}</button>
      <button type="button" disabled={!!busy && busy !== 'prepare' && busy !== 'run'} onClick={stop}>{t("Stop and pause automatic runs")}</button>
      <button type="button" disabled={!!busy} onClick={() => { setError(null); setRefresh(value => value + 1); }}>{t("Refresh status")}</button>
    </div>
    {error && <p className={styles.error} role="alert">{translateMessage(error)}</p>}
    {notice && <p className={styles.notice} role="status">{translateMessage(notice)}</p>}
    {dirty && <p className={styles.notice}>{t("Save your changes in Model & schedule before downloading a model or running analysis.")}</p>}
    <section hidden={view !== 'overview'} aria-label={t("Overview")}>
      <div className={styles.metrics}>
        <div><strong>{formatNumber(status?.catalogAssets ?? 0)}</strong><span>{t("Catalog files")}</span></div>
        <div><strong>{formatNumber(status?.analyzedAssets ?? 0)}</strong><span>{t("Reviewed")}</span></div>
        <div><strong>{formatNumber(status?.pendingAssets ?? 0)}</strong><span>{t("Waiting for analysis")}</span></div>
        <div><strong>{formatNumber(status?.withoutPreview ?? 0)}</strong><span>{t("No preview yet")}</span></div>
      </div>
      <p className={styles.hint}>{t("Catalog counts are a snapshot from the latest analysis. After changing models, run a new batch to update them. Files without a preview are not analyzed. A finished batch does not mean the whole library is finished.")}</p>
      {!status?.runId && !status?.lastRunAt && <p className={styles.notice}>{t("No analysis summary yet. Run a batch to calculate library progress.")}</p>}
      <div className={styles.grid}>
        <article className={styles.card}><h2>{t("What will happen?")}</h2>
          <ol><li>{t("Read cached previews from your local catalog.")}</li><li>{t("Suggest one broad category per preview and combine results by category.")}</li>
            <li>{t("Wait for your approval before adding files to a collection.")}</li></ol>
          <p className={styles.hint}>{t("Collections work like playlists. Your folders, source files, and model references stay in place.")}</p>
        </article>
        <article className={styles.card}><h2>{t("Current setup")}</h2>
          <p><strong>{overview.settings.model}</strong></p>
          <p>{t('{mode} · Up to {count} assets per batch', { mode: t(modes.find(mode => mode.value === overview.settings.mode)?.title ?? overview.settings.mode), count: formatNumber(overview.settings.batchSize) })}</p>
          <p className={styles.hint}>{t(overview.settings.provider === 'lmstudio' ? 'LM Studio manages memory and GPU use.' : overview.settings.cpuOnly ? 'CPU only. GPU acceleration is disabled.' : 'GPU acceleration is allowed.')}</p>
          <p className={styles.hint}>{t('Last operation ended: {date}', { date: status?.lastRunAt ? formatDate(status.lastRunAt) : t('No completed operation recorded') })}</p>
          {!!status?.skipped && <p className={styles.hint}>{t('{count} entries skipped in this batch because no usable local preview was available.', { count: formatNumber(status.skipped) })}</p>}
        </article>
      </div>
    </section>
    <section hidden={view !== 'model'} aria-label={t("Model and schedule")}>
    <h2>{t("Model & schedule")}</h2>
    {!overview.settings.setupComplete && <p className={styles.notice}>{t("Choose a mode and save your choice to finish setup. Disabled is selected by default.")}</p>}
    <fieldset className={styles.modes} disabled={!!busy}>
      <legend>{t("When should Local AI run?")}</legend>
      {modes.map(mode => <label key={mode.value} className={styles.mode}>
        <input type="radio" name="local-ai-mode" value={mode.value} checked={settings.mode === mode.value}
          onChange={() => update({ mode: mode.value })} />
        <span><strong>{t(mode.title)}</strong><span>{t(mode.description)}</span></span>
      </label>)}
    </fieldset>
    {settings.mode === 'idle' && <label className={styles.field} htmlFor="local-ai-idle">{t("Idle time (minutes)")}<input id="local-ai-idle" type="number" min={1} max={240} value={settings.idleMinutes} disabled={!!busy}
        onChange={event => update({ idleMinutes: Number(event.target.value) })} />
    </label>}
    {settings.mode === 'scheduled' && <div className={styles.grid}>
      <label className={styles.field} htmlFor="local-ai-start">{t("Start hour (local time)")}<input id="local-ai-start" type="number" min={0} max={23} value={settings.startHour} disabled={!!busy}
          onChange={event => update({ startHour: Number(event.target.value) })} />
      </label>
      <label className={styles.field} htmlFor="local-ai-end">{t("End hour (local time)")}<input id="local-ai-end" type="number" min={0} max={23} value={settings.endHour} disabled={!!busy}
          onChange={event => update({ endHour: Number(event.target.value) })} />
      </label>
      <p className={styles.hint}>{t("Daily window; a start hour after the end hour continues past midnight. LumiaDAM must remain open.")}</p>
    </div>}
    <div className={styles.grid}>
      <label className={styles.field} htmlFor="local-ai-provider">{t("Local engine")}<select id="local-ai-provider" value={settings.provider} disabled={!!busy} onChange={event => {
          const provider = event.target.value as AiSettings['provider'];
          update({ provider, endpoint: provider === 'lmstudio' ? 'http://127.0.0.1:1234' : 'http://127.0.0.1:11434',
            model: provider === 'lmstudio' ? '' : 'qwen3.5:0.8b' });
        }}>
          <option value="bundled">{t("LumiaDAM runtime (optional install)")}</option>
          <option value="ollama">{t("Existing Ollama server")}</option>
          <option value="lmstudio">{t("Existing LM Studio server")}</option>
        </select>
      </label>
      <label className={styles.field} htmlFor="local-ai-endpoint">{t("Local server address")}<input id="local-ai-endpoint" type="url" value={settings.endpoint} readOnly={settings.provider === 'bundled'} disabled={!!busy}
          onChange={event => update({ endpoint: event.target.value })} />
      </label>
      <label className={styles.field} htmlFor="local-ai-model">{t("Model identifier")}{settings.provider === 'lmstudio' ? <input id="local-ai-model" value={settings.model} disabled={!!busy}
          placeholder={t("Installed model key from LM Studio")} onChange={event => update({ model: event.target.value })} /> :
          <select id="local-ai-model" value={settings.model} disabled={!!busy} onChange={() => update({ model: 'qwen3.5:0.8b' })}>
            <option value="qwen3.5:0.8b">Qwen 3.5 0.8B</option>
          </select>}
      </label>
      <label className={styles.field} htmlFor="local-ai-batch">{t("Assets per batch")}<input id="local-ai-batch" type="number" min={1} max={1000} value={settings.batchSize} disabled={!!busy}
          onChange={event => update({ batchSize: Number(event.target.value) })} />
      </label>
    </div>
    {settings.provider === 'bundled' && <p className={styles.hint}>{t(overview.runtimeInstalled ?
      'The optional LumiaDAM runtime is installed. The model is a separate download.' :
      'To add the runtime, run the LumiaDAM installer and select Local AI. You can also use an existing Ollama or LM Studio server.')}</p>}
    {settings.provider === 'lmstudio' && <p className={styles.hint}>{t("Load a vision model in LM Studio, start its local server, and enter the installed model key. LumiaDAM uses the loaded instance and does not load or unload LM Studio models. LM Studio is installed and licensed separately. Its own settings control CPU, GPU, and memory use.")}</p>}
    <label className={styles.check}>
      <input type="checkbox" checked={!settings.cpuOnly} disabled={!!busy || settings.provider === 'lmstudio'}
        onChange={event => update({ cpuOnly: !event.target.checked })} />{t("Allow GPU acceleration (uses VRAM)")}</label>
    <p className={styles.hint}>{t("CPU only is the default for the LumiaDAM runtime and Ollama. No model starts when LumiaDAM opens. A permitted run loads a model into RAM. Existing servers may retain models under their own settings.")}</p>
    {validation && <p role="alert" className={styles.error}>{t(validation)}</p>}
    <div className={styles.actions}>
      <button type="button" disabled={!!busy || !!validation} onClick={save}>{t(busy === 'save' ? 'Saving…' : 'Save settings')}</button>
      {settings.provider !== 'lmstudio' && <button type="button" disabled={!!busy || dirty || !!validation || !!status?.running ||
        settings.mode === 'disabled' || !overview.settings.setupComplete ||
        (settings.provider === 'bundled' && !overview.runtimeInstalled)}
        onClick={() => perform('prepare', api.prepareLocalAiModel, 'Model download started. Progress is shown below.')}>
        {t(preparing ? 'Downloading…' : 'Download model')}
      </button>}
    </div>
    </section>
    <section hidden={view !== 'suggestions'} aria-labelledby="local-ai-review">
      <h2 id="local-ai-review">{t("Review suggestions")}</h2>
      <p>{t("Approve a card to create or update its collection in LumiaDAM. This feature never moves or deletes source files.")}</p>
      {suggestions.some(suggestion => suggestion.status === 'pending') && <div className={styles.actions}>
        <button type="button" className={styles.primary} disabled={!!busy} onClick={approveAll}>
          {t("Accept all pending suggestions")}
        </button>
      </div>}
      <div className={styles.suggestions}>{suggestions.filter(suggestion => suggestion.status === 'pending').map(suggestion => {
        const totalItems = suggestion.totalItems ?? suggestion.items.length;
        return <article className={styles.suggestion} key={suggestion.id}>
        <span className={styles.hint}>{t(suggestion.kind === 'duplicates' ? 'Possible duplicate or similar group — review source files' : 'Suggested collection')}</span>
        <h3>{translateMessage(suggestion.collectionName ?? suggestion.title)} <span className={styles.badge}>{t('{count} files', { count: formatNumber(totalItems) })}</span></h3>
        {suggestion.kind === 'duplicates' && <p>{translateMessage(suggestion.reason)}</p>}
        <p className={styles.hint}>{t('On approval: add {count} files to “{collection}” in Collections.', { count: formatNumber(totalItems), collection: translateMessage(suggestion.collectionName ?? 'Review') })}
          {' '}{t(suggestion.kind === 'duplicates' ? 'Matching previews do not prove identical source files.' : 'This is a model suggestion, not a verified classification.')}</p>
        <div className={styles.previews} aria-label={t("Sample previews")}>
          {suggestion.items.slice(0, 4).map(item => <figure key={item.id}>
            {item.thumbnailPath ? <img loading="lazy" src={convertFileSrc(item.thumbnailPath)} alt={item.fileName}
              onError={event => { event.currentTarget.style.display = 'none'; }} /> : <span>{t("No cached preview")}</span>}
            <figcaption title={item.filePath}>{item.fileName}</figcaption>
          </figure>)}
        </div>
        <details onToggle={event => {
          const open = event.currentTarget.open;
          setExpanded(current => ({ ...current, [suggestion.id]: open }));
        }}><summary>{t('Review {count} assets', { count: formatNumber(totalItems) })}</summary>
          {expanded[suggestion.id] && <>
            {totalItems > 100 && <p className={styles.hint}>{t('Showing the first 100 of {count} assets. Approval includes all {count} assets.', { count: formatNumber(totalItems) })}</p>}
            <ul>{suggestion.items.slice(0, 100).map(item => <li key={item.id}><strong>{item.fileName}</strong> <span>({item.libraryName})</span>
              <span className={styles.path}>{item.filePath}</span>
              <span className={styles.hint}>{item.basedOn ? t('Linked by filename group to {file}; contents not analyzed.', { file: item.basedOn }) : t('Suggested from a cached preview or a linked filename group.')}</span></li>)}</ul>
          </>}
        </details>
        <div className={styles.actions}>
          <button type="button" disabled={!!busy} onClick={() => review(suggestion, true)}>
            {t(suggestion.kind === 'duplicates' ? 'Approve grouping' : 'Approve collection')}
          </button>
          <button type="button" disabled={!!busy} onClick={() => review(suggestion, false)}>{t("Dismiss")}</button>
        </div>
      </article>; })}</div>
      {!suggestions.some(suggestion => suggestion.status === 'pending') && <p>{t("No pending suggestions.")}</p>}
    </section>
    <section hidden={view !== 'log'} aria-label={t("Activity log")}>
      <h2>{t("Activity log")}</h2>
      <p>{t("Model and schedule changes, analysis results, skipped previews, approvals, and errors are recorded locally. The latest 200 events are shown below, newest first.")}</p>
      <div className={styles.actions}>
        <button type="button" disabled={!log || !!busy} onClick={() => perform('open-log', api.openLocalAiLog)}>{t("Open log file")}</button>
        <button type="button" onClick={() => setRefresh(value => value + 1)}>{t("Refresh log")}</button>
      </div>
      {log && <p className={styles.path}>{log.path}</p>}
      <p className={styles.hint}>{t("JSON Lines format. Five rotating files use up to about 25 MiB in total. Image data is not written to the log.")}</p>
      {logError && <p role="alert" className={styles.error}>{translateMessage(logError)}</p>}
      {!log && !logError && <p role="status">{t("Loading activity log…")}</p>}
      {log?.entries.length === 0 && <p>{t("No events recorded yet. Existing suggestions created before logging was added have no historical log.")}</p>}
      <div className={styles.events}>{log?.entries.map((entry, index) => <details key={`${entry.at}:${index}`}>
        <summary><time dateTime={entry.at}>{formatDate(entry.at)}</time>
          <strong>{translateMessage(entry.event.replaceAll('_', ' '))}</strong>
          {typeof entry.details.fileName === 'string' && <span>{entry.details.fileName}</span>}
          {typeof entry.details.category === 'string' && <span>{translateMessage(entry.details.category)}</span>}
        </summary>
        <pre>{JSON.stringify(entry.details, (key, value) => typeof value === 'string' && ['phase', 'state', 'error', 'reason', 'category', 'collection', 'mode'].includes(key) ? translateMessage(value) : value, 2)}</pre>
      </details>)}</div>
    </section>
  </section>;
}
