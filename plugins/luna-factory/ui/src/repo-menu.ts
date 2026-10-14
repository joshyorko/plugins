/**
 * The plan form's repo chip and its popover menu (WAI-ARIA menu button pattern).
 * Opening, closing and moving focus are view-only: they never read or send anything.
 * Picking, connecting and "+ New repo…" are handed to the caller.
 */
export interface RepoMenuHandlers {
  pick(alias: string): void;
  newRepository(): void;
  connect(fullName: string): void;
  more(): void;
}

const chipOf = (root: ParentNode): HTMLButtonElement | null => root.querySelector<HTMLButtonElement>("#repo-chip");
const popoverOf = (root: ParentNode): HTMLElement | null => root.querySelector<HTMLElement>("#repo-popover");

/** Menu items a person can reach with the arrow keys, in reading order. Filtered-out rows are skipped. */
export function repoMenuItems(root: ParentNode): HTMLElement[] {
  return Array.from(root.querySelectorAll<HTMLElement>('#repo-menu [role="menuitem"], #repo-menu [role="menuitemradio"]')).filter(item => !item.hidden);
}

export function isRepoMenuOpen(root: ParentNode): boolean {
  return chipOf(root)?.getAttribute("aria-expanded") === "true";
}

/** Open the popover. `focus` picks where keyboard focus lands; "none" only restores the open state after a re-render. */
export function openRepoMenu(root: ParentNode, focus: "selected" | "first" | "last" | "none" = "selected"): void {
  const chip = chipOf(root);
  const popover = popoverOf(root);
  if (!chip || !popover || chip.disabled) return;
  chip.setAttribute("aria-expanded", "true");
  popover.hidden = false;
  place(popover, chip);
  if (focus === "none") return;
  const items = repoMenuItems(root);
  const filter = popover.querySelector<HTMLInputElement>("#repo_filter");
  const target = focus === "last" ? items.at(-1)
    : focus === "first" ? items[0]
    : filter ?? items.find(item => item.getAttribute("aria-checked") === "true") ?? items[0];
  target?.focus();
}

export function closeRepoMenu(root: ParentNode, restoreFocus = true): void {
  const chip = chipOf(root);
  const popover = popoverOf(root);
  if (!chip || !popover) return;
  chip.setAttribute("aria-expanded", "false");
  popover.hidden = true;
  if (restoreFocus) chip.focus();
}

/** Open below the chip; flip above only when there is no room below and there is room above. */
function place(popover: HTMLElement, chip: HTMLElement): void {
  popover.dataset.placement = "bottom";
  const view = popover.ownerDocument.defaultView;
  if (!view) return;
  const box = popover.getBoundingClientRect();
  const anchor = chip.getBoundingClientRect();
  if (box.height > 0 && box.bottom > view.innerHeight - 8 && anchor.top - box.height - 8 > 0) popover.dataset.placement = "top";
}

function activate(root: ParentNode, item: HTMLElement, handlers: RepoMenuHandlers): void {
  if (item.getAttribute("aria-disabled") === "true") return;
  const action = item.dataset.repoAction;
  if (action === "more") { handlers.more(); return; }
  closeRepoMenu(root);
  if (action === "pick" && item.dataset.alias) handlers.pick(item.dataset.alias);
  else if (action === "connect" && item.dataset.repository) handlers.connect(item.dataset.repository);
  else if (action === "new") { handlers.newRepository(); root.querySelector<HTMLInputElement>("#new_repository")?.focus(); }
}

/**
 * Keyboard. The chip and items are native buttons, so Enter and Space activate them through the click
 * handler. Here: arrows open from the chip; arrows, Home and End move; Esc closes and returns focus to the chip.
 */
export function handleRepoMenuKey(root: ParentNode, event: KeyboardEvent): boolean {
  const target = event.target;
  if (!(target instanceof HTMLElement)) return false;
  if (target.id === "repo-chip") {
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      openRepoMenu(root, event.key === "ArrowUp" ? "last" : "selected");
      return true;
    }
    if (event.key === "Escape" && isRepoMenuOpen(root)) { event.preventDefault(); closeRepoMenu(root); return true; }
    return false;
  }
  if (!target.closest("#repo-popover")) return false;
  if (event.key === "Escape") { event.preventDefault(); event.stopPropagation(); closeRepoMenu(root); return true; }
  // Tab leaves the menu and lets focus move on naturally.
  if (event.key === "Tab") { closeRepoMenu(root, false); return false; }
  const items = repoMenuItems(root);
  if (target.id === "repo_filter") {
    if (event.key === "ArrowDown" && items.length) { event.preventDefault(); items[0]?.focus(); return true; }
    if (event.key === "Enter") { event.preventDefault(); return true; }
    return false;
  }
  const index = items.indexOf(target);
  if (index < 0) return false;
  const filter = root.querySelector<HTMLInputElement>("#repo_filter");
  let next: HTMLElement | undefined;
  switch (event.key) {
    case "ArrowDown": next = items[(index + 1) % items.length]; break;
    case "ArrowUp": next = index === 0 && filter ? filter : items[(index - 1 + items.length) % items.length]; break;
    case "Home": next = items[0]; break;
    case "End": next = items.at(-1); break;
    default: return false;
  }
  event.preventDefault();
  next?.focus();
  return true;
}

/** Wire the chip and menu into a mounted workbench. Returns a cleanup function. */
export function bindRepoMenu(root: HTMLElement, handlers: RepoMenuHandlers): () => void {
  const onKey = (event: KeyboardEvent): void => { handleRepoMenuKey(root, event); };
  const onClick = (event: MouseEvent): void => {
    if (!(event.target instanceof Element)) return;
    const chip = event.target.closest<HTMLButtonElement>("#repo-chip");
    if (chip && root.contains(chip)) {
      event.preventDefault();
      if (!chip.disabled) { if (isRepoMenuOpen(root)) closeRepoMenu(root); else openRepoMenu(root); }
      return;
    }
    const item = event.target.closest<HTMLElement>("#repo-menu [data-repo-action]");
    if (item && root.contains(item)) { event.preventDefault(); activate(root, item, handlers); return; }
    // A click anywhere outside the chip and popover closes the menu without moving focus.
    if (isRepoMenuOpen(root) && !event.target.closest(".repo-select")) closeRepoMenu(root, false);
  };
  const outside = (event: Event): void => {
    if (isRepoMenuOpen(root) && event.target instanceof Node && !root.contains(event.target)) closeRepoMenu(root, false);
  };
  root.addEventListener("keydown", onKey);
  root.addEventListener("click", onClick);
  root.ownerDocument.addEventListener("click", outside);
  return () => {
    root.removeEventListener("keydown", onKey);
    root.removeEventListener("click", onClick);
    root.ownerDocument.removeEventListener("click", outside);
  };
}
