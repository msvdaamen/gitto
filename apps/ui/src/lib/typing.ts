/**
 * Whether a key pressed with `event` is typing, e.g. in the commit message or the editor, rather
 * than a shortcut: going by where it was pressed, inside a shadow root too.
 */
export function isTyping(event: KeyboardEvent): boolean {
  const target = event.composedPath()[0] ?? event.target;
  return (
    target instanceof HTMLElement &&
    (target.isContentEditable || ["INPUT", "TEXTAREA", "SELECT"].includes(target.tagName))
  );
}
