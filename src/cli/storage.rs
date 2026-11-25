//! `storage`: what the launcher's managed files take up, and clearing a row of them.
use super::*;

pub(super) fn report(clear: Option<ClearStorage>) -> Result<(), Error> {
    let storage = Storage::open().map_err(native_mods::cache_error)?;
    let (row, all) = match clear {
        Some(ClearStorage::Row(row)) => (Some(row), false),
        Some(ClearStorage::All) => (None, true),
        None => (None, false),
    };
    let report = storage.report(row, all).map_err(native_mods::cache_error)?;
    println!("{}", serde_json::to_string_pretty(&report).unwrap());
    if row.is_some() || all {
        if let Some(kept) = report.rows.iter().flat_map(|row| &row.kept).next() {
            return Err(kept.reason.clone().into());
        }
    }
    Ok(())
}
