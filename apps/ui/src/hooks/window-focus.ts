import { createSignal, onCleanup } from "solid-js";

/** Whether the window has focus, i.e. someone's looking at it rather than at another app. */
export function useWindowFocus() {
  const [focused, setFocused] = createSignal(document.hasFocus());
  const onFocus = () => setFocused(true);
  const onBlur = () => setFocused(false);
  window.addEventListener("focus", onFocus);
  window.addEventListener("blur", onBlur);
  onCleanup(() => {
    window.removeEventListener("focus", onFocus);
    window.removeEventListener("blur", onBlur);
  });
  return focused;
}
