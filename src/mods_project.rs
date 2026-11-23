//! Project icons and gallery images (`sunrise_launcher::browser::images`), cached on disk;
//! a placeholder stands in until one arrives.
use super::mods_page::Event;
use super::*;

use std::collections::HashMap;
use std::path::PathBuf;
use sunrise_launcher::browser::images;

/// The images' share of the Mods page's state.
#[derive(Default)]
pub(super) struct Browse {
    /// Images on disk, by URL and size; `None` while they load or when they failed.
    images: HashMap<(String, u32), Option<PathBuf>>,
    image_queue: Option<std::sync::mpsc::Sender<(String, u32)>>,
    /// Content-addressed v2 registry images on disk, by `(project_id, relative_path, size)`.
    v2_images: HashMap<(String, String, u32), Option<PathBuf>>,
    v2_image_queue: Option<std::sync::mpsc::Sender<(String, String, u32)>>,
}

/// What the background sends this module.
pub(super) enum Loaded {
    Image((String, u32), Option<PathBuf>),
    V2Image((String, String, u32), Option<PathBuf>),
}

/// Images fetched at once.
const IMAGE_WORKERS: usize = 4;

impl Launcher {
    pub(super) fn mods_loaded(&mut self, loaded: Loaded) {
        let b = &mut self.mods.browse;
        match loaded {
            Loaded::Image(key, path) => {
                // A refresh that failed (offline) keeps the cached image.
                if path.is_some() || !matches!(b.images.get(&key), Some(Some(_))) {
                    b.images.insert(key, path);
                }
            }
            Loaded::V2Image(key, path) => {
                if path.is_some() || !matches!(b.v2_images.get(&key), Some(Some(_))) {
                    b.v2_images.insert(key, path);
                }
            }
        }
    }

    /// The image of `url` at `size` when it is on disk; else asks for it once. One cached longer
    /// than `images::FRESH` is shown and asked for again, with its validators.
    pub(super) fn mods_image(&mut self, url: &str, size: u32) -> Option<PathBuf> {
        let key = (url.to_owned(), size);
        if let Some(state) = self.mods.browse.images.get(&key) {
            return state.clone();
        }
        // The launcher's data folder, shared by every instance.
        let home = backend::data_dir();
        if let Some(p) = images::cached(&home, url, size) {
            self.mods.browse.images.insert(key.clone(), Some(p.clone()));
            if images::stale(&home, url, size) {
                let _ = self.mods_image_queue().send(key);
            }
            return Some(p);
        }
        self.mods.browse.images.insert(key.clone(), None);
        if images::refuse(url).is_some() {
            return None;
        }
        let _ = self.mods_image_queue().send(key);
        None
    }

    /// A verified content-addressed v2 registry image (`pages/<project>/<sha256>.<png|jpg>`) at
    /// `size` when on disk; otherwise requests it once. Paths that are not content-addressed for
    /// `project_id` are never requested or shown.
    pub(super) fn v2_mod_image(
        &mut self,
        project_id: &str,
        relative_path: &str,
        size: u32,
    ) -> Option<PathBuf> {
        let key = (project_id.to_owned(), relative_path.to_owned(), size);
        if let Some(state) = self.mods.browse.v2_images.get(&key) {
            return state.clone();
        }
        if sunrise_launcher_smod::index_v2::content_addressed_image_sha256(
            project_id,
            relative_path,
        )
        .is_err()
        {
            self.mods.browse.v2_images.insert(key, None);
            return None;
        }
        let home = backend::data_dir();
        if let Some(p) = images::cached_v2(&home, project_id, relative_path, size) {
            self.mods.browse.v2_images.insert(key, Some(p.clone()));
            return Some(p);
        }
        self.mods.browse.v2_images.insert(key.clone(), None);
        let _ = self.v2_image_queue().send(key);
        None
    }

    /// The image workers' queue for content-addressed v2 registry images.
    fn v2_image_queue(&mut self) -> std::sync::mpsc::Sender<(String, String, u32)> {
        let tx = self.mods.tx.clone();
        self.mods
            .browse
            .v2_image_queue
            .get_or_insert_with(|| {
                let (send, receive) = std::sync::mpsc::channel::<(String, String, u32)>();
                let receive = std::sync::Arc::new(std::sync::Mutex::new(receive));
                let home = backend::data_dir();
                for _ in 0..IMAGE_WORKERS {
                    let (receive, tx, home) = (receive.clone(), tx.clone(), home.clone());
                    std::thread::spawn(move || loop {
                        let Ok(next) = receive.lock().map(|r| r.recv()) else {
                            break;
                        };
                        let Ok((project_id, relative_path, size)) = next else {
                            break;
                        };
                        let path = images::fetch_v2(&home, &project_id, &relative_path, size).ok();
                        if tx
                            .send(Event::Loaded(Loaded::V2Image(
                                (project_id, relative_path, size),
                                path,
                            )))
                            .is_err()
                        {
                            break;
                        }
                    });
                }
                send
            })
            .clone()
    }

    /// The image workers' queue, started the first time an image is asked for.
    fn mods_image_queue(&mut self) -> std::sync::mpsc::Sender<(String, u32)> {
        let tx = self.mods.tx.clone();
        self.mods
            .browse
            .image_queue
            .get_or_insert_with(|| {
                let (send, receive) = std::sync::mpsc::channel::<(String, u32)>();
                let receive = std::sync::Arc::new(std::sync::Mutex::new(receive));
                let home = backend::data_dir();
                for _ in 0..IMAGE_WORKERS {
                    let (receive, tx, home) = (receive.clone(), tx.clone(), home.clone());
                    std::thread::spawn(move || loop {
                        let Ok(next) = receive.lock().map(|r| r.recv()) else {
                            break;
                        };
                        let Ok((url, size)) = next else {
                            break;
                        };
                        let path = images::fetch(&home, &url, size).ok();
                        if tx
                            .send(Event::Loaded(Loaded::Image((url, size), path)))
                            .is_err()
                        {
                            break;
                        }
                    });
                }
                send
            })
            .clone()
    }
}
