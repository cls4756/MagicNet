export type ScrollMetrics = {
  scrollTop: number;
  scrollHeight: number;
  clientHeight: number;
};

export type ScrollRailGeometry = {
  visible: boolean;
  height: number;
  offset: number;
};

/** A short list must still show a grabbable-looking thumb. */
export const SCROLL_RAIL_MIN_THUMB = 32;

/**
 * Overlay scrollbars stay invisible until the user touches them, so a bounded list
 * looks like it cannot scroll. The rail is drawn from the scroller metrics instead.
 */
export function scrollRailGeometry(metrics: ScrollMetrics): ScrollRailGeometry {
  const { scrollTop, scrollHeight, clientHeight } = metrics;
  if (clientHeight <= 0 || scrollHeight <= clientHeight + 1) {
    return { visible: false, height: 0, offset: 0 };
  }
  const height = Math.min(
    clientHeight,
    Math.max(SCROLL_RAIL_MIN_THUMB, Math.round((clientHeight / scrollHeight) * clientHeight)),
  );
  const maxOffset = Math.max(0, clientHeight - height);
  const scrollable = scrollHeight - clientHeight;
  const progress = scrollable > 0 ? scrollTop / scrollable : 0;
  const offset = Math.round(Math.min(1, Math.max(0, progress)) * maxOffset);
  return { visible: true, height, offset };
}
