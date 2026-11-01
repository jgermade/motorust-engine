//! The circuits read again when their files change on the disk, in a
//! desktop development build: a circuit saved in an editor is seen in the
//! game a moment later, with no restart (and, to come, one saved by a map
//! editor).
//!
//! Twice a second [`watch`] asks the disk when the circuits' list and each
//! circuit's file were last changed. When one was, it reads them all again
//! and tells the world which circuits are new ([`CircuitsReloaded`]): the
//! world forgets what it had built of them and builds the one chosen again
//! (see [`crate::world`]). A file with a mistake is told in the log, and
//! the game goes on with what it had. Not in a race: what changed waits
//! till it is over.

use bevy::prelude::*;

#[cfg(all(debug_assertions, not(target_arch = "wasm32")))]
use super::{Circuits, CurrentCircuit, LIST};
#[cfg(all(debug_assertions, not(target_arch = "wasm32")))]
use crate::menu::GameState;

/// Which circuits were read again, by their indices in
/// [`super::Circuits`] (before
/// and after: one the list no longer has is among them).
#[derive(Message, Clone, Debug, PartialEq, Eq)]
pub struct CircuitsReloaded(pub Vec<usize>);

/// The circuits whose files differ between `before` and `after` (each its
/// path and its text, in the menu's order): those at the same index with
/// another file or another text, and those only one of them has.
#[cfg(any(
    test,
    feature = "testing",
    all(debug_assertions, not(target_arch = "wasm32"))
))]
pub fn changed(before: &[(String, &str)], after: &[(String, &str)]) -> Vec<usize> {
    (0..before.len().max(after.len()))
        .filter(|&index| before.get(index) != after.get(index))
        .collect()
}

/// What [`watch`] keeps: the files as last read, when each was changed on
/// the disk, and what changed that waits for a race to end.
#[cfg(all(debug_assertions, not(target_arch = "wasm32")))]
#[derive(Resource)]
pub struct Watch {
    files: Vec<(String, &'static str)>,
    modified: Vec<Option<std::time::SystemTime>>,
    every: Timer,
    waiting: Option<(Circuits, Vec<usize>)>,
}

#[cfg(all(debug_assertions, not(target_arch = "wasm32")))]
impl Default for Watch {
    fn default() -> Self {
        let files = super::files().unwrap_or_else(|error| panic!("{error}"));
        let modified = when_changed(&files);
        Self {
            files,
            modified,
            every: Timer::from_seconds(0.5, TimerMode::Repeating),
            waiting: None,
        }
    }
}

/// When the list and each of the `files` were last changed on the disk.
#[cfg(all(debug_assertions, not(target_arch = "wasm32")))]
fn when_changed(files: &[(String, &str)]) -> Vec<Option<std::time::SystemTime>> {
    std::iter::once(LIST)
        .chain(files.iter().map(|(path, _)| path.as_str()))
        .map(crate::content::modified)
        .collect()
}

/// The circuits' files as they are on the disk now, read again.
#[cfg(all(debug_assertions, not(target_arch = "wasm32")))]
fn reread() -> Result<Vec<(String, &'static str)>, String> {
    crate::content::reread(LIST)?;
    let names: Vec<String> = super::data::parse(LIST, crate::content::try_text(LIST)?)?;
    names
        .into_iter()
        .map(|name| {
            let path = format!("circuits/{name}");
            crate::content::reread(&path)?;
            let text = crate::content::try_text(&path)?;
            Ok((path, text))
        })
        .collect()
}

/// Reads the circuits again when their files change on the disk, and puts
/// the new ones in place when not racing (see the module's docs). Real
/// time, which goes on in the pause menu.
#[cfg(all(debug_assertions, not(target_arch = "wasm32")))]
pub fn watch(
    time: Res<Time<Real>>,
    state: Option<Res<State<GameState>>>,
    mut watch: ResMut<Watch>,
    mut circuits: ResMut<Circuits>,
    mut current: ResMut<CurrentCircuit>,
    mut reloaded: MessageWriter<CircuitsReloaded>,
) {
    if watch.every.tick(time.delta()).just_finished() {
        let modified = when_changed(&watch.files);
        if modified != watch.modified {
            watch.modified = modified;
            match reread().and_then(|files| Circuits::parse(&files).map(|new| (files, new))) {
                Ok((files, new)) => {
                    let mut changed = changed(&watch.files, &files);
                    // The list may have named other files.
                    watch.modified = when_changed(&files);
                    watch.files = files;
                    if !changed.is_empty() {
                        info!("circuits read again: {changed:?}");
                        if let Some((_, before)) = watch.waiting.take() {
                            changed.extend(before);
                            changed.sort_unstable();
                            changed.dedup();
                        }
                        watch.waiting = Some((new, changed));
                    }
                }
                Err(error) => error!("the circuits were not read again: {error}"),
            }
        }
    }
    let racing = state.is_some_and(|state| *state.get() == GameState::Race);
    if !racing && let Some((new, changed)) = watch.waiting.take() {
        *circuits = new;
        if current.0 >= circuits.len() {
            current.0 = 0;
        }
        reloaded.write(CircuitsReloaded(changed));
    }
}

#[cfg(any(test, feature = "testing"))]
mod tests {
    use super::*;

    fn files(texts: &[(&str, &'static str)]) -> Vec<(String, &'static str)> {
        texts
            .iter()
            .map(|&(path, text)| (path.to_string(), text))
            .collect()
    }

    #[test]
    fn only_the_circuits_whose_files_differ_are_new() {
        let before = files(&[("a.ron", "A"), ("b.ron", "B"), ("c.ron", "C")]);
        assert!(changed(&before, &before).is_empty(), "nothing changed");
        let edited = files(&[("a.ron", "A"), ("b.ron", "B2"), ("c.ron", "C")]);
        assert_eq!(changed(&before, &edited), [1], "one file edited");
        // Two swapped in the menu: both places are new.
        let swapped = files(&[("b.ron", "B"), ("a.ron", "A"), ("c.ron", "C")]);
        assert_eq!(changed(&before, &swapped), [0, 1]);
        // One taken off the end, and one added.
        assert_eq!(changed(&before, &before[..2]), [2]);
        let added = files(&[
            ("a.ron", "A"),
            ("b.ron", "B"),
            ("c.ron", "C"),
            ("d.ron", "D"),
        ]);
        assert_eq!(changed(&before, &added), [3]);
    }
}
