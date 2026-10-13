import { useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { createViewer, type Controllers, type Status } from '@exav/viewer';
import { dwg, dxf, isCadSupported } from '@exav/viewer/cad';
import type { Asset } from '../../types';
import { api } from '../../api';
import { IconExternal, IconLayers, IconRefresh } from '../Icons';
import styles from './CadViewer.module.css';

const viewer = createViewer({
  plugins: [
    dwg({ ground: 'dark', maxPrimitives: 4_000_000, maxDecompressedBytes: 512 * 1024 * 1024 }),
    dxf({ ground: 'dark', maxPrimitives: 4_000_000, maxDecompressedBytes: 512 * 1024 * 1024 }),
  ],
});

const statusText = (status: Status) => {
  if (status.phase === 'loading') return status.label ?? 'Reading drawing…';
  if (status.phase === 'converting') return status.label ?? 'Preparing drawing…';
  if (status.phase === 'empty') return 'The drawing contains no visible geometry.';
  if (status.phase === 'error') {
    if (status.error.code === 'drawing_version') return 'This DWG version is not supported. R13–R2018 drawings are supported.';
    return status.error.message ?? 'The drawing could not be displayed.';
  }
  return '';
};

export function CadViewer({ asset }: { asset: Asset }) {
  const host = useRef<HTMLDivElement>(null);
  const [status, setStatus] = useState<string>('Reading source file…');
  const [failed, setFailed] = useState(false);
  const [retry, setRetry] = useState(0);
  const [controllers, setControllers] = useState<Controllers>({});
  const [layers, setLayers] = useState<ReturnType<NonNullable<Controllers['layers']>['get']> | null>(null);
  const [layouts, setLayouts] = useState<ReturnType<NonNullable<Controllers['layouts']>['get']> | null>(null);
  const [zoom, setZoom] = useState<ReturnType<NonNullable<Controllers['zoom']>['get']> | null>(null);
  const [ground, setGround] = useState<'light' | 'dark'>('dark');
  const [layersOpen, setLayersOpen] = useState(false);

  useEffect(() => {
    const subscriptions = [
      controllers.layers && (() => { setLayers(controllers.layers!.get()); return controllers.layers!.subscribe(setLayers); })(),
      controllers.layouts && (() => { setLayouts(controllers.layouts!.get()); return controllers.layouts!.subscribe(setLayouts); })(),
      controllers.zoom && (() => { setZoom(controllers.zoom!.get()); return controllers.zoom!.subscribe(setZoom); })(),
      controllers.ground && (() => { setGround(controllers.ground!.get()); return controllers.ground!.subscribe(setGround); })(),
    ].filter(Boolean) as Array<() => void>;
    return () => subscriptions.forEach(unsubscribe => unsubscribe());
  }, [controllers]);

  useEffect(() => {
    if (!host.current) return;
    if (!isCadSupported()) {
      setFailed(true);
      setStatus('WebGL2 is unavailable. Update the graphics driver or open the file in the default CAD application.');
      return;
    }
    let disposed = false;
    let session: ReturnType<typeof viewer.mount> | null = null;
    let unsubscribeStatus: (() => void) | null = null;
    let unsubscribeControllers: (() => void) | null = null;
    const container = host.current;
    container.replaceChildren();
    setFailed(false);
    setStatus('Reading source file…');
    setControllers({});
    setLayers(null);
    setLayouts(null);
    setZoom(null);

    void invoke<ArrayBuffer>('read_editor_asset', { id: asset.id, editor: 'cad' }).then(raw => {
      if (disposed) return;
      const bytes = raw instanceof ArrayBuffer ? new Uint8Array(raw) : new Uint8Array(raw as ArrayBuffer);
      session = viewer.mount(container, {
        id: `${asset.id}:${asset.modifiedAt}:${retry}`,
        name: asset.fileName,
        path: asset.filePath,
        size: asset.sizeBytes,
        format: asset.extension.toLowerCase(),
        source: { bytes },
      });
      const initialStatus = session.status.get();
      setFailed(initialStatus.phase === 'error');
      setStatus(statusText(initialStatus));
      setControllers(session.controllers.get());
      unsubscribeStatus = session.status.subscribe(next => {
        if (disposed) return;
        setFailed(next.phase === 'error');
        setStatus(statusText(next));
      });
      unsubscribeControllers = session.controllers.subscribe(next => !disposed && setControllers(next));
    }).catch(error => {
      if (!disposed) {
        setFailed(true);
        setStatus(`Source library is offline or the drawing could not be read: ${String(error)}`);
      }
    });

    return () => {
      disposed = true;
      unsubscribeStatus?.();
      unsubscribeControllers?.();
      session?.destroy();
      container.replaceChildren();
    };
  }, [asset.id, asset.modifiedAt, retry]);

  const scale = zoom?.scale ?? 1;
  return <section className={styles.workspace} aria-label={`CAD Viewer: ${asset.fileName}`}>
    <div className={styles.commandBar}>
      <div className={styles.identity}>
        <span className={styles.badge}>{asset.extension.toUpperCase()}</span>
        <span className={styles.fileName} title={asset.filePath}>{asset.fileName}</span>
      </div>
      <div className={styles.tools}>
        {layouts && layouts.items.length > 1 && <select aria-label="Drawing layout" value={layouts.current}
          onChange={event => void controllers.layouts?.select(event.target.value)}>
          {layouts.items.map(layout => <option key={layout.id} value={layout.id}>{layout.isModel ? 'Model Space' : layout.name}</option>)}
        </select>}
        <button type="button" onClick={() => controllers.zoom?.setScale(scale / 1.25)} disabled={!zoom} title="Zoom out">−</button>
        <span className={styles.zoom}>{Math.round(scale * 100)}%</span>
        <button type="button" onClick={() => controllers.zoom?.setScale(scale * 1.25)} disabled={!zoom} title="Zoom in">+</button>
        <button type="button" onClick={() => controllers.zoom?.fit()} disabled={!zoom}>Fit</button>
        <button type="button" className={layersOpen ? styles.active : undefined} onClick={() => setLayersOpen(open => !open)} disabled={!layers}>
          <IconLayers size={15} /> Layers{layers ? ` (${layers.items.length})` : ''}
        </button>
        <button type="button" onClick={() => controllers.ground?.set(ground === 'dark' ? 'light' : 'dark')} disabled={!controllers.ground}>
          {ground === 'dark' ? 'Light background' : 'Dark background'}
        </button>
        <button type="button" onClick={() => void api.openWithDefault(asset.filePath)} title="Open with the default CAD application">
          <IconExternal size={14} /> Open externally
        </button>
      </div>
    </div>
    <div className={styles.content}>
      {layersOpen && layers && <aside className={styles.layerPanel} aria-label="Drawing layers">
        <div className={styles.layerHeader}><strong>Layers</strong><span>
          <button type="button" onClick={() => controllers.layers?.setAll(true)}>All</button>
          <button type="button" onClick={() => controllers.layers?.setAll(false)}>None</button>
        </span></div>
        <div className={styles.layerList}>{layers.items.map(layer => <label key={layer.id}>
          <input type="checkbox" checked={layer.visible} onChange={event => controllers.layers?.setVisible(layer.id, event.target.checked)} />
          <i style={{ background: layer.color }} /><span title={layer.name}>{layer.name}</span>
        </label>)}</div>
      </aside>}
      <div ref={host} className={styles.viewerHost} />
      {status && <div className={failed ? styles.error : styles.status} role={failed ? 'alert' : 'status'}>
        <span>{status}</span>
        {failed && <button type="button" onClick={() => setRetry(value => value + 1)}><IconRefresh size={14} /> Retry</button>}
      </div>}
    </div>
  </section>;
}
