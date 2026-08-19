//! Generated, effective-keymap help for the interaction foundation.
//!
//! Help is derived from the validated typed map rather than a handwritten
//! duplicate of defaults, so scoped user overrides and disabled bindings stay
//! truthful without exposing the contents of `keymap.toml`.

use crate::ui::{
    actions::ContextAction,
    keymap::{KeyChord, KeyMap, KeyScope},
};

pub fn effective_keymap(keymap: &KeyMap, scope: KeyScope) -> String {
    effective_keymap_with_actions(keymap, scope, &[])
}

pub fn effective_keymap_with_actions(
    keymap: &KeyMap,
    scope: KeyScope,
    contextual_actions: &[ContextAction],
) -> String {
    let mut bindings = keymap
        .effective_bindings(scope)
        .into_iter()
        .map(|binding| (binding.sequence.label(), binding.action.label()))
        .collect::<Vec<_>>();
    bindings.sort_unstable_by(|left, right| left.0.cmp(&right.0));

    let mut lines = vec![
        "bzz effective keymap".to_owned(),
        String::new(),
        "The entries below include your active scoped overrides. Esc or q closes help.".to_owned(),
        String::new(),
    ];
    lines.extend(
        bindings
            .into_iter()
            .map(|(keys, action)| format!("  {keys:<20} {action}")),
    );
    let mut disabled = keymap
        .disabled_bindings(scope)
        .into_iter()
        .map(|sequence| sequence.label())
        .collect::<Vec<_>>();
    disabled.sort_unstable();
    if !disabled.is_empty() {
        lines.push(String::new());
        lines.push("Disabled in this scope".into());
        lines.extend(
            disabled
                .into_iter()
                .map(|keys| format!("  {keys:<20} disabled")),
        );
    }
    if !contextual_actions.is_empty() {
        lines.push(String::new());
        lines.push("Actions for focused item".into());
        lines.extend(contextual_actions.iter().map(|entry| match entry.reason {
            Some(reason) => format!("  {:<20} unavailable — {reason}", entry.label),
            None => format!("  {:<20} {}", entry.label, "available"),
        }));
    }
    lines.join("\n")
}

pub fn which_key(keymap: &KeyMap, scope: KeyScope, prefix: &[KeyChord]) -> String {
    let mut hints = keymap
        .next_chords(scope, prefix)
        .into_iter()
        .map(|chord| {
            let mut sequence = prefix.to_vec();
            sequence.push(chord);
            let label = match keymap.lookup(scope, &sequence) {
                crate::ui::keymap::KeyLookup::Action(action) => action.label(),
                crate::ui::keymap::KeyLookup::Pending => "continue sequence",
                crate::ui::keymap::KeyLookup::NoMatch => "",
            };
            (chord.label(), label)
        })
        .collect::<Vec<_>>();
    hints.sort_unstable_by(|left, right| left.0.cmp(&right.0));

    let prefix = prefix
        .iter()
        .map(|chord| chord.label())
        .collect::<Vec<_>>()
        .join(" ");
    let mut lines = vec![format!(" {prefix} · next key "), String::new()];
    lines.extend(
        hints
            .into_iter()
            .map(|(key, action)| format!("  {key:<12} {action}")),
    );
    lines.push(String::new());
    lines.push("Esc cancels".into());
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use crate::ui::{
        actions::ContextAction,
        keymap::{KeyChord, KeyMap, KeyScope, UiAction},
    };

    use super::{effective_keymap, effective_keymap_with_actions, which_key};

    #[test]
    fn help_is_derived_from_the_effective_scoped_map() {
        let temporary = TempDir::new().unwrap();
        let path = temporary.path().join("keymap.toml");
        fs::write(
            &path,
            "[[binding]]\nscope = 'workspace'\nkeys = ['?']\ndisabled = true\n\
             [[binding]]\nscope = 'workspace'\nkeys = ['x']\naction = 'open-inbox'\n",
        )
        .unwrap();
        let keymap = KeyMap::load_from(&path).unwrap();
        let help = effective_keymap(&keymap, KeyScope::Workspace);
        assert!(help.contains("x                    open Inbox"));
        assert!(!help.contains("show keymap help"));
        assert!(help.contains("Disabled in this scope"));
        assert!(help.contains("?                    disabled"));
    }

    #[test]
    fn help_lists_enabled_and_unavailable_context_actions() {
        let output = effective_keymap_with_actions(
            &KeyMap::builtin(),
            KeyScope::Workspace,
            &[
                ContextAction {
                    action: UiAction::Compose,
                    label: "reply",
                    enabled: true,
                    reason: None,
                },
                ContextAction {
                    action: UiAction::Delete,
                    label: "delete own message",
                    enabled: false,
                    reason: Some("only your own message can be deleted"),
                },
            ],
        );
        assert!(output.contains("Actions for focused item"));
        assert!(output.contains("reply                available"));
        assert!(output.contains("unavailable — only your own message can be deleted"));
    }

    #[test]
    fn which_key_labels_only_valid_continuations() {
        let keymap = KeyMap::builtin();
        let output = which_key(
            &keymap,
            KeyScope::Workspace,
            &[KeyChord::new(
                crossterm::event::KeyCode::Char(' '),
                crossterm::event::KeyModifiers::NONE,
            )],
        );
        assert!(output.contains("n            open Inbox"));
        assert!(output.contains("Space        channel / DM switcher"));
        assert!(!output.contains("refresh\n  z"));
    }
}
