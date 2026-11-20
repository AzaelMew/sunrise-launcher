//! The mods that go when the player continues: the ones whose build failed, the ones taken
//! down and the ones with no version for the game they would play. One list, one sheet.
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
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Removal {
    /// The instance whose profile the mods are in.
    pub(crate) instance: String,
    pub(crate) items: Vec<Item>,
    /// "Show why" is open.
    pub(crate) why: bool,
}

impl Removal {
    /// Adds `items`, each mod once: a failed build's log wins over a plainer reason.
    pub(crate) fn add(&mut self, instance: &str, items: impl IntoIterator<Item = Item>) {
        self.instance = instance.to_owned();
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
        }
    }

    #[test]
    fn the_heading_says_could_not_be_built_only_when_every_mod_failed_to_build() {
        let mut removal = Removal::default();
        removal.add("main", [item("a", Some("la")), item("b", Some("lb"))]);
        assert_eq!(
            removal.heading(),
            "2 mods couldn't be built and will be removed"
        );
        removal.add("main", [item("c", None)]);
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
        removal.add("main", [item("a", None)]);
        removal.add(
            "main",
            [
                item("a", Some("error[E0425]")),
                item("b", Some("error[E0308]")),
            ],
        );
        removal.add("main", [item("b", None)]);
        assert_eq!(removal.items.len(), 2);
        assert_eq!(removal.items[0].log.as_deref(), Some("error[E0425]"));
        assert_eq!(removal.log(), "error[E0425]\n\nerror[E0308]");
    }
}
