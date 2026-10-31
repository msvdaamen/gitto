//! A keyboard shortcut. Ported from `components/ui/kbd.tsx`.

use gpui_kit::component::ActiveTheme as _;
use gpui_kit::prelude::*;
use gpui_kit::{Action, App, KeyContext, Modifiers, SharedString, Window, div};

use crate::theme::PaletteExt as _;

/// A keyboard shortcut, like `⌘ K`, as it's written. (GPUI Kit's `Kbd` shows a bound
/// `Keystroke`; use it for an action's binding.)
#[derive(IntoElement)]
pub struct Kbd {
    keys: SharedString,
}

impl Kbd {
    pub fn new(keys: impl Into<SharedString>) -> Self {
        Kbd { keys: keys.into() }
    }
}

impl RenderOnce for Kbd {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let palette = *cx.palette();
        div()
            .flex_none()
            .px_1()
            .py_0p5()
            .rounded_sm()
            .border_1()
            .border_color(palette.border)
            .bg(palette.panel_raised)
            .font_family(cx.theme().mono_font_family.clone())
            .text_sm()
            .text_color(palette.faint)
            .child(self.keys)
    }
}

/// The keys bound to `actions` in the key context `context`, as `aria-keyshortcuts` writes them
/// (`Alt+Shift+ArrowLeft Alt+Shift+ArrowRight`): read from the keymap, so they're the keys that
/// do it, however they're bound.
pub fn aria_keyshortcuts(window: &Window, actions: &[&dyn Action], context: &str) -> SharedString {
    let Ok(context) = KeyContext::parse(context) else {
        return SharedString::default();
    };
    actions
        .iter()
        .flat_map(|action| window.bindings_for_action_in_context(*action, context.clone()))
        .filter_map(|binding| {
            // A single keystroke: ARIA has no way to write a sequence.
            match binding.keystrokes() {
                [keystroke] => Some(aria_keystroke(keystroke.modifiers(), keystroke.key())),
                _ => None,
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
        .into()
}

/// A keystroke as `aria-keyshortcuts` writes it: its modifiers, then its key, joined by `+`.
fn aria_keystroke(modifiers: &Modifiers, key: &str) -> String {
    let mut parts = Vec::new();
    if modifiers.control {
        parts.push("Control".to_owned());
    }
    if modifiers.alt {
        parts.push("Alt".to_owned());
    }
    if modifiers.shift {
        parts.push("Shift".to_owned());
    }
    if modifiers.platform {
        parts.push("Meta".to_owned());
    }
    parts.push(match key {
        "left" => "ArrowLeft".to_owned(),
        "right" => "ArrowRight".to_owned(),
        "up" => "ArrowUp".to_owned(),
        "down" => "ArrowDown".to_owned(),
        "pageup" => "PageUp".to_owned(),
        "pagedown" => "PageDown".to_owned(),
        "space" => "Space".to_owned(),
        key => {
            // `home`, `enter`, `escape`…: capitalized, as the DOM names them; a letter's
            // uppercase.
            let mut chars = key.chars();
            chars
                .next()
                .map(|first| first.to_uppercase().chain(chars).collect())
                .unwrap_or_default()
        }
    });
    parts.join("+")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_a_keystroke_as_aria_does() {
        let alt_shift = Modifiers {
            alt: true,
            shift: true,
            ..Modifiers::default()
        };
        assert_eq!(aria_keystroke(&alt_shift, "left"), "Alt+Shift+ArrowLeft");
        assert_eq!(aria_keystroke(&Modifiers::default(), "home"), "Home");
        assert_eq!(
            aria_keystroke(&Modifiers::secondary_key(), "k"),
            if cfg!(target_os = "macos") {
                "Meta+K"
            } else {
                "Control+K"
            }
        );
    }
}
