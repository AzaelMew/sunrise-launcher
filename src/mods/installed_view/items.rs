//! The Installed page's items: the rows, their labels, and what a bulk removal names.

use super::*;

/// Keep removal reviews on the same labels Installed paints, including hidden dependents.
pub(crate) fn remove_names(rows: &[Row], labels: &[Option<String>]) -> BTreeMap<String, String> {
    rows.iter()
        .zip(labels)
        .filter(|(row, _)| row.selectable_archive())
        .map(|(row, label)| (row.id.clone(), label.as_ref().unwrap_or(&row.title).clone()))
        .collect()
}

impl ModsState {
    pub(super) fn installed_filters(&mut self, rows: &[Row]) -> model::FilterCounts {
        let before = self.row_filter.clone();
        let counts = model::counted_filters(rows, &mut self.row_filter);
        if rows.is_empty() || self.row_filter != before {
            self.selection.clear();
            self.anchor = None;
        }
        counts
    }

    pub(super) fn select_installed_row(
        &mut self,
        file: &str,
        shown: &[(String, String)],
        shift: bool,
    ) {
        let Some(pos) = shown.iter().position(|(path, _)| path == file) else {
            return;
        };
        let anchor = self
            .anchor
            .as_ref()
            .filter(|_| shift)
            .and_then(|anchor| shown.iter().position(|(path, _)| path == anchor));
        if let Some(anchor) = anchor {
            self.selection.extend(
                shown[anchor.min(pos)..=anchor.max(pos)]
                    .iter()
                    .map(|(_, id)| id.clone()),
            );
        } else {
            let id = &shown[pos].1;
            if !self.selection.remove(id) {
                self.selection.insert(id.clone());
            }
        }
        self.anchor = Some(file.to_owned());
    }
}

pub(super) enum InstalledItem<'a> {
    Head(&'a str, usize),
    Gap,
    /// Index into `rows`, and the row's place among the rows shown.
    Row(usize, usize),
}

pub(super) fn installed_items_key(items: &[InstalledItem<'_>], rows: &[Row]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    for item in items {
        match item {
            InstalledItem::Head(title, _) => (0u8, *title).hash(&mut hash),
            InstalledItem::Gap => 1u8.hash(&mut hash),
            InstalledItem::Row(i, _) => {
                (2u8, rows[*i].id.as_str(), rows[*i].archive_file()).hash(&mut hash)
            }
        }
    }
    hash.finish()
}
