import type { LibraryWorkStatus } from '../../api';

// Durum olayları birleşir; devam eden isteğin üzerine ikinci SQL sorgusu başlamaz.
export function createStatusPolling(options: {
  read: () => Promise<LibraryWorkStatus[]>;
  onStatus: (statuses: LibraryWorkStatus[]) => void;
  onError: (error: unknown) => void;
  panelOpen: () => boolean;
}) {
  let stopped = false;
  let inFlight: Promise<void> | null = null;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let eventQueued = false;
  let active = false;
  const schedule = (delay: number) => {
    if (timer !== undefined) clearTimeout(timer);
    if (!stopped) timer = setTimeout(() => { timer = undefined; void refresh(); }, delay);
  };
  const refresh = (): Promise<void> => {
    if (stopped) return Promise.resolve();
    if (inFlight) return inFlight;
    if (timer !== undefined) clearTimeout(timer);
    timer = undefined;
    eventQueued = false;
    inFlight = (async () => {
      try {
        const statuses = await options.read();
        if (stopped) return;
        active = statuses.some((s) => s.scanning || (s.previewPending > 0 && !s.previewPaused));
        options.onStatus(statuses);
      } catch (error) {
        if (!stopped) { active = false; options.onError(error); }
      } finally {
        inFlight = null;
        schedule(eventQueued ? 1000 : active || options.panelOpen() ? 2000 : 30000);
      }
    })();
    return inFlight;
  };
  return {
    refresh,
    invalidate: () => {
      if (stopped || eventQueued) return;
      eventQueued = true;
      if (!inFlight) schedule(1000);
    },
    stop: () => { stopped = true; if (timer !== undefined) clearTimeout(timer); },
  };
}
