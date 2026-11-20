//! The mods that go when the player continues: the ones whose build failed, the ones taken
//! down and the ones with no version for the game they would play. One list, one sheet.
use sunrise_launcher::browser::installed;
use sunrise_launcher::browser::sunrise::auto;
use sunrise_launcher::i18n::trf;

/// A mod on the list, with why and, for one whose build failed, the compiler's output.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Item {
    pub(crate) id: String,
    pub(crate) title: String,
    pub(crate) reason: String,
    /// Set when the mod's own compile failed.
    pub(crate) log: Option<String>,
    /// The checksum of the installed file when it was listed: the mod goes only if it is still
    /// that file.
    pub(crate) checksum: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Removal {
    /// The instance whose profile the mods are in.
    pub(crate) instance: String,
    pub(crate) items: Vec<Item>,
    /// "Show why" is open.
    pub(crate) why: bool,
}

/// The removals waiting for an answer, one list per profile: one profile's list never holds
/// another's mods.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Pending(Vec<Removal>);

impl Pending {
    /// Adds `items` to the list of profile `instance`, whose installed `entries` they were read
    /// from.
    pub(crate) fn add(
        &mut self,
        instance: &str,
        entries: &[installed::Entry],
        items: impl IntoIterator<Item = Item>,
    ) {
        let at = match self.0.iter().position(|r| r.instance == instance) {
            Some(at) => at,
            None => {
                self.0.push(Removal {
                    instance: instance.to_owned(),
                    ..Removal::default()
                });
                self.0.len() - 1
            }
        };
        self.0[at].add(items.into_iter().map(|mut item| {
            if let Some(entry) = entries.iter().find(|e| e.project == item.id) {
                item.checksum = entry.checksum.clone();
            }
            item
        }));
    }

    pub(crate) fn first(&self) -> Option<&Removal> {
        self.0.first()
    }

    pub(crate) fn first_mut(&mut self) -> Option<&mut Removal> {
        self.0.first_mut()
    }

    /// The list that is on screen, taken for its answer; the next profile's follows.
    pub(crate) fn take_first(&mut self) -> Option<Removal> {
        (!self.0.is_empty()).then(|| self.0.remove(0))
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl Removal {
    /// Adds `items`, each mod once: a failed build's log wins over a plainer reason.
    pub(crate) fn add(&mut self, items: impl IntoIterator<Item = Item>) {
        for item in items {
            match self.items.iter_mut().find(|i| i.id == item.id) {
                Some(have) if item.log.is_some() => *have = item,
                Some(_) => {}
                None => self.items.push(item),
            }
        }
    }

    /// The list from automatic updates' removals.
    pub(crate) fn of_updates(removals: Vec<auto::Removal>) -> Vec<Item> {
        removals
            .into_iter()
            .map(|r| Item {
                id: r.id,
                title: r.title,
                reason: r.reason,
                log: None,
                checksum: String::new(),
            })
            .collect()
    }

    /// "2 mods couldn't be built and will be removed" when every one failed to build, else
    /// "2 mods will be removed": the reasons are under each name.
    pub(crate) fn heading(&self) -> String {
        let count = self.items.len().to_string();
        let built = self.items.iter().all(|i| i.log.is_some());
        match (self.items.len(), built) {
            (1, true) => sunrise_launcher::i18n::tr("1 mod couldn't be built and will be removed")
                .to_string(),
            (1, false) => sunrise_launcher::i18n::tr("1 mod will be removed").to_string(),
            (_, true) => trf(
                "{count} mods couldn't be built and will be removed",
                &[("count", &count)],
            ),
            (_, false) => trf("{count} mods will be removed", &[("count", &count)]),
        }
    }

    /// The compiler output of the mods whose build failed, one after another.
    pub(crate) fn log(&self) -> String {
        self.items
            .iter()
            .filter_map(|i| i.log.as_deref())
            .collect::<Vec<_>>()
            .join("\n\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: &str, log: Option<&str>) -> Item {
        Item {
            id: id.into(),
            title: id.to_uppercase(),
            reason: "r".into(),
            log: log.map(str::to_owned),
            checksum: String::new(),
        }
    }

    #[test]
    fn the_heading_says_could_not_be_built_only_when_every_mod_failed_to_build() {
        let mut removal = Removal::default();
        removal.add([item("a", Some("la")), item("b", Some("lb"))]);
        assert_eq!(
            removal.heading(),
            "2 mods couldn't be built and will be removed"
        );
        removal.add([item("c", None)]);
        assert_eq!(removal.heading(), "3 mods will be removed");
        let one = Removal {
            items: vec![item("a", Some("l"))],
            ..Removal::default()
        };
        assert_eq!(one.heading(), "1 mod couldn't be built and will be removed");
    }

    #[test]
    fn a_mod_is_listed_once_and_its_compiler_log_wins() {
        let mut removal = Removal::default();
        removal.add([item("a", None)]);
        removal.add([
            item("a", Some("error[E0425]")),
            item("b", Some("error[E0308]")),
        ]);
        removal.add([item("b", None)]);
        assert_eq!(removal.items.len(), 2);
        assert_eq!(removal.items[0].log.as_deref(), Some("error[E0425]"));
        assert_eq!(removal.log(), "error[E0425]\n\nerror[E0308]");
    }

    #[test]
    fn one_profiles_list_never_holds_another_profiles_mods() {
        let entry = |checksum: &str| installed::Entry {
            source: sunrise_launcher::browser::SourceId::Sunrise,
            project: "a".into(),
            slug: "a".into(),
            title: "A".into(),
            kind: sunrise_launcher::browser::Kind::Mod,
            version: "1.0.0".into(),
            number: "1.0.0".into(),
            file: "sunrise-mods/a.smod".into(),
            checksum: checksum.into(),
            page: String::new(),
            dependency: false,
            verdict: None,
        };
        let mut pending = Pending::default();
        pending.add("first", &[entry("SHA-256:1")], [item("a", None)]);
        pending.add(
            "second",
            &[entry("SHA-256:2")],
            [item("b", None), item("a", None)],
        );
        pending.add("first", &[], [item("c", None)]);
        let first = pending.take_first().unwrap();
        assert_eq!(first.instance, "first");
        let ids: Vec<_> = first.items.iter().map(|i| i.id.as_str()).collect();
        assert_eq!(ids, ["a", "c"], "nothing of the second profile's");
        assert_eq!(first.items[0].checksum, "SHA-256:1");
        let second = pending.take_first().unwrap();
        assert_eq!(
            (second.instance.as_str(), second.items.len()),
            ("second", 2)
        );
        assert_eq!(second.items[1].checksum, "SHA-256:2");
        assert!(pending.is_empty());
    }
}
