//! Publishing a sealed plan, then completing it: committed, or undone back to the preimage.
use super::*;

#[derive(PartialEq, Eq, Clone, Copy)]
pub(crate) enum Boundary {
    BeforeRename,
    Renamed,
    Durable,
    BeforeMarker,
    WrittenMarker,
    DurableMarker,
}

impl Record {
    pub(super) fn publish_at<G>(
        &self,
        home: &Path,
        cutoff: impl FnOnce() -> Result<G, String>,
        mut boundary: impl FnMut(usize, Boundary) -> Result<(), String>,
    ) -> Result<(), String> {
        self.verify_home(home)?;
        let op = self.operation()?;
        op.stage.verify(home)?;
        let stage = op.stage.path(home);
        let plan = op.ready.as_ref().ok_or_else(unsettled)?;
        if let Some(binding) = self.undo_binding()? {
            Self::preallocated_binding(&stage, &binding)?;
        }
        let pointer = home.join(POINTER);
        let next = plan.pointer_after;
        if Image::at(&pointer)? != op.before
            || Image::at(&stage.join("pointer-old"))? != op.before
            || Image::at(&stage.join("pointer-new"))?
                != plan.next_pointer.as_ref().map_or(Image::Missing, |_| next)
        {
            return Err(at(&pointer, unsettled()));
        }
        let mut progress = progress::Slots::open(&stage, self.digest()?, plan.steps())?;
        if progress.current().phase != progress::Phase::Ready {
            return Err(unsettled());
        }
        let directories = plan.directories.len();
        for i in 0..directories {
            let (src, dst, id) = plan.directory(home, &stage, i)?;
            if !absent(&dst)? || Identity::of(&src)? != id {
                return Err(at(&dst, unsettled()));
            }
        }
        let mut entries = BTreeSet::new();
        for (i, file) in plan.files.iter().enumerate() {
            let name = if plan
                .directories
                .contains_key(file.name.parent().ok_or_else(unsettled)?)
            {
                if file.before != Image::Missing {
                    return Err(unsettled());
                }
                home.join(&file.name)
            } else {
                target(home, file)?
            };
            unique_entry(&name, &mut entries)?;
            if Image::at(&name)? != file.before
                || Image::at(&stage.join(format!("old-{i}")))? != file.before
                || Image::at(&stage.join(format!("new-{i}")))? != file.after
            {
                return Err(at(&name, unsettled()));
            }
        }
        let mut permission = Some(cutoff()?);
        for i in 0..plan.steps() {
            progress.advance(progress::Phase::Publishing, i + 1)?;
            drop(permission.take());
            boundary(i, Boundary::BeforeRename)?;
            let mut fault = Ok(());
            let after_rename = || {
                fault = boundary(i, Boundary::Renamed);
            };
            if plan.next_pointer.is_some() && i == plan.steps() - 1 {
                durable::restore_move_at(&stage.join("pointer-new"), &pointer, after_rename)
                    .map_err(|e| at(&pointer, e))?;
            } else if i < directories {
                let (src, dst, id) = plan.directory(home, &stage, i)?;
                if Identity::of(&src)? != id {
                    return Err(at(&src, unsettled()));
                }
                durable::move_new_directory_at(&src, &dst, after_rename)
                    .map_err(|e| at(&dst, e))?;
            } else {
                let slot = i - directories;
                let file = &plan.files[slot];
                let dst = target(home, file)?;
                match (file.before, file.after) {
                    (Image::Missing, Image::Missing) => (),
                    (_, Image::Missing) => durable::restore_move_at(
                        &dst,
                        &stage.join(format!("removed-{slot}")),
                        after_rename,
                    )
                    .map_err(|e| at(&dst, e))?,
                    _ => durable::restore_move_at(
                        &stage.join(format!("new-{slot}")),
                        &dst,
                        after_rename,
                    )
                    .map_err(|e| at(&dst, e))?,
                }
            }
            fault?;
            boundary(i, Boundary::Durable)?;
        }
        boundary(plan.steps(), Boundary::BeforeMarker)?;
        progress.advance_at(progress::Phase::Committed, plan.steps(), || {
            boundary(plan.steps(), Boundary::WrittenMarker)
        })?;
        boundary(plan.steps(), Boundary::DurableMarker)
    }

    pub(super) fn complete(
        &self,
        home: &Path,
        mut restored: impl FnMut(usize),
        retired: impl FnOnce(),
    ) -> Result<bool, String> {
        use progress::{Phase, Slots};
        self.verify_home(home)?;
        let pointer = home.join(POINTER);
        let Some(op) = &self.operation else {
            if !absent(&pointer)? {
                return Err(unsettled());
            }
            self.clean_binding_stage(home)?;
            return Ok(false);
        };
        op.stage.verify(home)?;
        let stage = op.stage.path(home);
        let mut committed = false;
        if let Some(plan) = &op.ready {
            let next = plan.pointer_after;
            let mut slots = Slots::open(&stage, self.digest()?, plan.steps())?;
            match slots.current().phase {
                Phase::Ready => {
                    if Image::at(&pointer)? != op.before {
                        return Err(at(&pointer, unsettled()));
                    }
                }
                Phase::Committed => {
                    slots.sync()?; // The last marker can be visible after a reported sync failure.
                    committed = true;
                    if Image::at(&pointer)? != next {
                        return Err(at(&pointer, unsettled()));
                    }
                    for i in 0..plan.directories.len() {
                        let (_, dst, id) = plan.directory(home, &stage, i)?;
                        if Identity::of(&dst)? != id {
                            return Err(at(&dst, unsettled()));
                        }
                    }
                    for file in &plan.files {
                        let dst = target(home, file)?;
                        if Image::at(&dst)? != file.after {
                            return Err(at(&dst, unsettled()));
                        }
                    }
                }
                Phase::Publishing | Phase::Undoing => {
                    if slots.current().phase == Phase::Publishing {
                        slots.advance(Phase::Undoing, slots.current().frontier)?;
                    }
                    let directories = plan.directories.len();
                    while slots.current().frontier > 0 {
                        let step = slots.current().frontier - 1;
                        if plan.next_pointer.is_some() && step == plan.steps() - 1 {
                            restore(
                                &pointer,
                                &stage.join("pointer-old"),
                                &stage.join("pointer-discarded"),
                                op.before,
                                next,
                            )?;
                        } else if step < directories {
                            let (src, dst, id) = plan.directory(home, &stage, step)?;
                            restore_directory(&src, &dst, id)?;
                        } else {
                            let index = step - directories;
                            let spec = &plan.files[index];
                            let dst = target(home, spec)?;
                            restore(
                                &dst,
                                &stage.join(format!("old-{index}")),
                                &stage.join(format!("discarded-{index}")),
                                spec.before,
                                spec.after,
                            )?;
                        }
                        restored(step);
                        slots.advance(Phase::Undoing, step)?;
                    }
                    if Image::at(&pointer)? != op.before {
                        return Err(at(&pointer, unsettled()));
                    }
                }
            }
        } else if Image::at(&pointer)? != op.before {
            return Err(at(&pointer, unsettled()));
        }
        let file = home.join(RECORD);
        if !committed {
            if let Some(binding) = self.undo_binding()? {
                let saved = Self::preallocated_binding(&stage, &binding)?;
                durable::restore_move(&saved, &file).map_err(|e| at(&saved, e))?;
                retired();
                let _ = binding.clean_binding_stage(home);
                return Ok(false);
            }
        }
        let retired_file = home.join(RETIRED);
        if !absent(&retired_file)? {
            return Err(at(&retired_file, unsettled()));
        }
        durable::mark_retired(home, &file, &retired_file).map_err(|e| at(&file, e))?;
        retired();
        // This writer already established the retirement barrier. If cleanup fails, leave
        // proof for the next admission (which checks the barrier again before removing it).
        let _ = remove_retired(home, &retired_file, &op.stage);
        Ok(committed)
    }
}

/// Prove the directory is back on the private volume before advancing the undo frontier.
fn restore_directory(src: &Path, dst: &Path, id: Identity) -> Result<(), String> {
    if !absent(dst)? {
        if Identity::of(dst)? != id || fs::read_dir(dst).map_err(|e| at(dst, e))?.next().is_some() {
            return Err(at(dst, unsettled()));
        }
        return durable::move_new_directory(dst, src).map_err(|e| at(dst, e));
    }
    #[cfg(not(windows))]
    {
        if Identity::of(src)? != id {
            return Err(at(src, unsettled()));
        }
        durable::sync_restored(dst, src, true).map_err(|e| at(dst, e))
    }
    #[cfg(windows)]
    {
        // A visible name alone cannot stand in for the same-volume write-through move.
        // Retry in the private stage, never temporarily returning the directory to live.
        // If killed between moves, the same identity can be at exactly one of these names.
        let replay = src.with_extension("undo-directory");
        if !absent(src)? {
            if Identity::of(src)? != id {
                return Err(at(src, unsettled()));
            }
            durable::move_new_directory(src, &replay).map_err(|e| at(src, e))?;
        } else if Identity::of(&replay)? != id {
            return Err(at(&replay, unsettled()));
        }
        durable::move_new_directory(&replay, src).map_err(|e| at(src, e))
    }
}

fn restore(
    dst: &Path,
    backup: &Path,
    discarded: &Path,
    before: Image,
    after: Image,
) -> Result<(), String> {
    let live = Image::at(dst)?;
    if live != before {
        if live != after || Image::at(backup)? != before {
            return Err(at(dst, unsettled()));
        }
        if before == Image::Missing {
            durable::restore_move(dst, discarded)
        } else {
            durable::restore_move(backup, dst)
        }
        .map_err(|e| at(dst, e))?;
    }
    durable::sync_restored(dst, backup, before == Image::Missing).map_err(|e| at(dst, e))
}
