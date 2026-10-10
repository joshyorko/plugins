// Lunar iconography. Every glyph is decorative (aria-hidden); state is always also given as text.

const svg = (body: string, size = 20, view = 20, className = ""): string =>
  `<svg class="glyph${className ? ` ${className}` : ""}" width="${size}" height="${size}" viewBox="0 0 ${view} ${view}" aria-hidden="true" focusable="false">${body}</svg>`;

/**
 * The Luna Factory mark: a crescent held by a thin orbit, with one satellite riding the orbit's
 * opening. Works from 12px up. `assets/logo.svg` draws this exact 20-unit geometry on its night tile.
 */
export function lunaMark(size = 20): string {
  return svg(`<path d="M18 6.84A8.6 8.6 0 1 1 14.79 2.85" fill="none" stroke="currentColor" stroke-opacity=".28" stroke-width="1" stroke-linecap="round"/><path d="M12.9 4.2a6.2 6.2 0 1 0 2.9 9.9A5 5 0 0 1 12.9 4.2Z" fill="currentColor"/><circle cx="16.7" cy="4.6" r="1.25" fill="currentColor"/>`, size, 20, "luna-mark");
}

/**
 * A moon whose lit fraction is an evidence ratio (proven criteria over mandatory criteria).
 * The terminator is drawn with two arcs so any fraction renders exactly; 0 is a new moon outline.
 */
export function phaseGlyph(proven: number, total: number, size = 18): string {
  const fraction = total > 0 ? Math.min(1, Math.max(0, proven / total)) : 0;
  const r = 7;
  const c = 9;
  const base = `<circle cx="${c}" cy="${c}" r="${r}" class="phase-disc"/>`;
  if (fraction <= 0) return svg(`${base}<circle cx="${c}" cy="${c}" r="${r}" class="phase-rim"/>`, size, 18, "phase");
  if (fraction >= 1) return svg(`<circle cx="${c}" cy="${c}" r="${r}" class="phase-lit"/>`, size, 18, "phase full");
  // Lit region grows from the right limb (waxing). Terminator x-radius runs r..0..r.
  const k = Math.abs(1 - 2 * fraction) * r;
  const sweep = fraction < 0.5 ? 0 : 1;
  const lit = `<path class="phase-lit" d="M${c} ${c - r} A${r} ${r} 0 0 1 ${c} ${c + r} A${k.toFixed(3)} ${r} 0 0 ${sweep} ${c} ${c - r}Z"/>`;
  return svg(`${base}${lit}<circle cx="${c}" cy="${c}" r="${r}" class="phase-rim"/>`, size, 18, "phase");
}

export type TaskTone = "done" | "running" | "verify" | "ready" | "blocked" | "candidate" | "unknown";
/** Normalizes the backend task state (`candidate|ready|running|verify|done|blocked`, any case). */
export function taskTone(state: string): TaskTone {
  const value = state.toLowerCase();
  return value === "done" || value === "running" || value === "verify" || value === "ready" || value === "blocked" || value === "candidate" ? value : "unknown";
}
export const taskToneLabel: Record<TaskTone, string> = {
  done: "Done", running: "In progress", verify: "Verifying", ready: "Ready", blocked: "Blocked", candidate: "Planned", unknown: "State unknown",
};

/** Task state glyphs: distinct shapes so state never depends on color. */
export function taskGlyph(tone: TaskTone, size = 16): string {
  const shapes: Record<TaskTone, string> = {
    done: `<circle cx="8" cy="8" r="6.5" class="g-fill"/><path d="m5.2 8.2 1.9 1.9 3.7-4" class="g-knock"/>`,
    running: `<circle cx="8" cy="8" r="6" class="g-ring"/><path d="M8 2a6 6 0 0 1 0 12Z" class="g-fill"/>`,
    verify: `<circle cx="8" cy="8" r="6" class="g-ring"/><circle cx="8" cy="8" r="2.4" class="g-fill"/>`,
    ready: `<circle cx="8" cy="8" r="6" class="g-ring"/><path d="m6.6 5.4 3.2 2.6-3.2 2.6" class="g-line"/>`,
    blocked: `<circle cx="8" cy="8" r="6" class="g-ring"/><path d="M5 8h6" class="g-line"/>`,
    candidate: `<circle cx="8" cy="8" r="6" class="g-ring g-dashed"/>`,
    unknown: `<circle cx="8" cy="8" r="6" class="g-ring g-dashed"/><path d="M6.6 6.3a1.5 1.5 0 1 1 2.1 1.4c-.5.2-.7.6-.7 1v.3M8 11.2v.1" class="g-line"/>`,
  };
  return svg(shapes[tone], size, 16, `task-glyph tone-${tone}`);
}

export type Liveness = "active" | "idle" | "unknown";
/** Coordinator token: a crescent. Liveness changes the rim, never the identity. */
export function coordinatorToken(liveness: Liveness, size = 28): string {
  return svg(`<circle cx="14" cy="14" r="12.5" class="tok-rim live-${liveness}"/><path d="M16.6 7.4a7 7 0 1 0 3.9 11.2 5.6 5.6 0 0 1-3.9-11.2Z" class="tok-moon"/>`, size, 28, "token coordinator");
}

/** Worker token: a small satellite on its own orbit, numbered by stable roster order. */
export function workerToken(ordinal: number, liveness: Liveness, size = 24): string {
  return svg(`<circle cx="12" cy="12" r="10.5" class="tok-rim live-${liveness}"/><text x="12" y="15.6" text-anchor="middle" class="tok-label">${Math.max(1, Math.min(99, ordinal))}</text>`, size, 24, "token worker");
}

export function icon(name: "back" | "close" | "refresh" | "map" | "lanes" | "chat" | "open" | "plus" | "settings" | "arrow" | "clock" | "alert", size = 16): string {
  const paths: Record<typeof name, string> = {
    back: '<path d="m10 4-4 4 4 4"/>', close: '<path d="m4.5 4.5 7 7m0-7-7 7"/>', refresh: '<path d="M12.6 6A5 5 0 1 0 13 9.5M12.8 2.8V6H9.6"/>',
    map: '<circle cx="3.5" cy="4.5" r="1.6"/><circle cx="3.5" cy="11.5" r="1.6"/><circle cx="12.5" cy="8" r="1.6"/><path d="M5 4.8c3 .4 4 2 6 2.8M5 11.2c3-.4 4-2 6-2.8"/>',
    lanes: '<path d="M2.5 4h11M2.5 8h11M2.5 12h11"/><path d="M5 4h4M7 8h5M4 12h3" stroke-width="2.6"/>',
    chat: '<path d="M3 3.5h10v7H7.5L4.5 13v-2.5H3Z"/>', open: '<path d="M9.5 3H13v3.5M13 3 8 8M11 9.5V13H3V5h3.5"/>',
    plus: '<path d="M8 3v10M3 8h10"/>', settings: '<path d="M3 5h10M3 11h10M6 3v4M10 9v4"/>', arrow: '<path d="m6 4 4 4-4 4"/>',
    clock: '<circle cx="8" cy="8" r="5.5"/><path d="M8 5v3.2l2 1.3"/>', alert: '<path d="M8 2.5 14 13H2Z"/><path d="M8 6.5v3M8 11.3v.1"/>',
  };
  return `<svg class="icon" width="${size}" height="${size}" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true" focusable="false">${paths[name]}</svg>`;
}
