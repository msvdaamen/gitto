//! What a repository loads by a key, like a file's patch or a commit, a [`Loaded`] for each:
//! [`Keyed`]. A view holds the ones it shows ([`Hold`]); the repository reloads those as it
//! changes and drops the rest, and keeps a few of those nothing holds, the most recently used, to
//! show again at once.

use std::collections::HashMap;
use std::hash::Hash;
use std::rc::{Rc, Weak};

use super::loaded::Loaded;

/// A view's hold on what it shows of a [`Keyed`]: while it's kept, that's kept, and reloaded as
/// the repository changes.
#[derive(Clone)]
pub struct Hold {
    _alive: Rc<()>,
}

struct Entry<T, A> {
    loaded: Loaded<T>,
    /// What it was last asked for with (see [`Keyed::ask`]).
    asked: Option<A>,
    holds: Vec<Weak<()>>,
    /// When it was last asked for, to drop the least recently used first.
    used: u64,
}

impl<T, A> Entry<T, A> {
    fn is_held(&self) -> bool {
        self.holds.iter().any(|hold| hold.strong_count() > 0)
    }
}

/// Loaded data by key (see the module's doc), and what each was asked for with, `A`, if it's
/// loaded from more than its key: a file's patch from the file as its list has it, say.
pub struct Keyed<K, T, A = ()> {
    entries: HashMap<K, Entry<T, A>>,
    /// How many that nothing holds are kept.
    unheld: usize,
    clock: u64,
}

impl<K: Clone + Eq + Hash, T, A> Keyed<K, T, A> {
    /// Keeping `unheld` entries that nothing holds.
    pub fn new(unheld: usize) -> Self {
        Keyed {
            entries: HashMap::new(),
            unheld,
            clock: 0,
        }
    }

    pub fn get(&self, key: &K) -> Option<&Loaded<T>> {
        self.entries.get(key).map(|entry| &entry.loaded)
    }

    pub fn get_mut(&mut self, key: &K) -> Option<&mut Loaded<T>> {
        self.entries.get_mut(key).map(|entry| &mut entry.loaded)
    }

    /// The entry for `key`, made if there's none, as used just now; drops the least recently used
    /// of those nothing holds, past what's kept.
    pub fn entry(&mut self, key: K) -> &mut Loaded<T> {
        self.clock += 1;
        let clock = self.clock;
        if !self.entries.contains_key(&key) {
            self.evict();
        }
        let entry = self.entries.entry(key).or_insert_with(|| Entry {
            loaded: Loaded::default(),
            asked: None,
            holds: Vec::new(),
            used: clock,
        });
        entry.used = clock;
        &mut entry.loaded
    }

    /// What `key`'s entry was last asked for with, which it's loaded from.
    pub fn asked(&self, key: &K) -> Option<&A> {
        self.entries.get(key)?.asked.as_ref()
    }

    /// Has `key`'s entry, made if there's none, loaded from `asked` from now on; what it was asked
    /// for with before.
    pub fn ask(&mut self, key: K, asked: A) -> Option<A> {
        self.entry(key.clone());
        let entry = self.entries.get_mut(&key).expect("just made");
        entry.asked.replace(asked)
    }

    /// Holds `key`'s entry, made if there's none (see [`Hold`]).
    pub fn hold(&mut self, key: K) -> Hold {
        self.entry(key.clone());
        let hold = Rc::new(());
        let entry = self.entries.get_mut(&key).expect("just made");
        entry.holds.retain(|hold| hold.strong_count() > 0);
        entry.holds.push(Rc::downgrade(&hold));
        Hold { _alive: hold }
    }

    /// The keys of the entries `pick` picks that something holds, to reload; those nothing holds
    /// are dropped, as they're out of date, or marked so if they're loading (a patch loaded ahead,
    /// say), so what they load is loaded again once they're asked for.
    pub fn held_and_drop_rest(&mut self, pick: impl Fn(&K) -> bool) -> Vec<K> {
        self.entries.retain(|key, entry| {
            if !pick(key) || entry.is_held() {
                return true;
            }
            if entry.loaded.is_fetching() {
                entry.loaded.mark_stale();
                return true;
            }
            false
        });
        self.entries
            .iter()
            .filter(|(key, entry)| pick(key) && entry.is_held())
            .map(|(key, _)| key.clone())
            .collect()
    }

    /// The keys of the entries something holds whose last load failed, and that aren't loading.
    pub fn held_failed(&self) -> Vec<K> {
        self.entries
            .iter()
            .filter(|(_, entry)| {
                entry.is_held() && entry.loaded.is_error() && !entry.loaded.is_fetching()
            })
            .map(|(key, _)| key.clone())
            .collect()
    }

    /// Drops the entries nothing holds or loads: for a repository that's gone off show, which
    /// needn't keep them till it's shown again.
    pub fn drop_unheld(&mut self) {
        self.entries
            .retain(|_, entry| entry.is_held() || entry.loaded.is_fetching());
    }

    /// Sets `key`'s data, as if it had loaded it.
    pub fn set(&mut self, key: K, data: T)
    where
        T: PartialEq,
    {
        self.entry(key).set(data);
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Drops the least recently used entries nothing holds or loads, until there's room for one
    /// more of them.
    fn evict(&mut self) {
        loop {
            let unheld: Vec<(&K, u64)> = self
                .entries
                .iter()
                .filter(|(_, entry)| !entry.is_held() && !entry.loaded.is_fetching())
                .map(|(key, entry)| (key, entry.used))
                .collect();
            if unheld.len() < self.unheld {
                return;
            }
            let Some(oldest) = unheld
                .into_iter()
                .min_by_key(|(_, used)| *used)
                .map(|(key, _)| key.clone())
            else {
                return;
            };
            self.entries.remove(&oldest);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_what_its_held_and_a_few_more() {
        let mut keyed = Keyed::<&str, i32>::new(2);
        let held = keyed.hold("a");
        keyed.set("b", 2);
        keyed.set("c", 3);
        keyed.set("d", 4);
        // `b` was the least recently used of those nothing holds.
        assert!(keyed.get(&"a").is_some());
        assert!(keyed.get(&"b").is_none());
        assert_eq!(keyed.len(), 3);
        drop(held);
        keyed.set("e", 5);
        assert!(keyed.get(&"a").is_none());
    }

    #[test]
    fn keeps_what_each_was_asked_for_with() {
        let mut keyed = Keyed::<&str, i32, &str>::new(8);
        assert_eq!(keyed.ask("a", "first"), None);
        assert_eq!(keyed.ask("a", "then"), Some("first"));
        assert_eq!(keyed.asked(&"a"), Some(&"then"));
        assert_eq!(keyed.asked(&"b"), None);
    }

    #[test]
    fn drops_what_nothing_holds_or_loads() {
        let mut keyed = Keyed::<&str, i32>::new(8);
        let _held = keyed.hold("a");
        keyed.set("b", 2);
        keyed.drop_unheld();
        assert!(keyed.get(&"a").is_some());
        assert!(keyed.get(&"b").is_none());
    }

    #[test]
    fn reloads_what_its_held_and_drops_the_rest() {
        let mut keyed = Keyed::<&str, i32>::new(8);
        let _held = keyed.hold("a");
        keyed.set("b", 2);
        keyed.set("other", 3);
        assert_eq!(keyed.held_and_drop_rest(|key| *key != "other"), ["a"]);
        assert!(keyed.get(&"b").is_none());
        assert!(keyed.get(&"other").is_some());
    }
}
