export function observeVisiblePreviews(
  container: HTMLElement,
  requested: Set<string>,
  prioritize: (ids: string[]) => Promise<void>,
): () => void {
  const elements = container.querySelectorAll<HTMLElement>('[data-preview-id]');
  const currentKeys = new Set(Array.from(elements, (element) => element.getAttribute('data-preview-key') ?? ''));
  for (const key of requested) if (!currentKeys.has(key)) requested.delete(key);
  const pending = new Map<string, string>();
  let timer: ReturnType<typeof setTimeout> | undefined;
  const observer = new IntersectionObserver((entries) => {
    for (const entry of entries) {
      if (!entry.isIntersecting) continue;
      const id = entry.target.getAttribute('data-preview-id');
      const key = entry.target.getAttribute('data-preview-key') ?? id;
      if (id && key && !requested.has(key)) pending.set(id, key);
    }
    if (!pending.size || timer !== undefined) return;
    timer = setTimeout(() => {
      timer = undefined;
      const batch = Array.from(pending).slice(0, 500);
      pending.clear();
      batch.forEach(([, key]) => requested.add(key));
      void prioritize(batch.map(([id]) => id)).catch(() => {
        batch.forEach(([, key]) => requested.delete(key));
      });
    }, 50);
  }, { root: container, rootMargin: '100px' });
  elements.forEach((element) => observer.observe(element));
  return () => {
    observer.disconnect();
    if (timer !== undefined) clearTimeout(timer);
    pending.clear();
  };
}
