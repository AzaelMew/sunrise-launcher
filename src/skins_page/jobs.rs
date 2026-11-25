//! The threads and bounded queues the page's requests run on, and how each answers.
use super::*;

/// How much work a [`Queue`] holds before it refuses more.
pub(super) const QUEUED: usize = 64;

/// A number for one request, never given twice in a run: an answer from before an account
/// switch can't pass for one asked after it.
pub(super) fn request_id() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

/// Runs `work` on a thread of its own: false when no thread could be started (the caller says
/// so), rather than a panic on the UI thread.
pub(super) fn spawn(name: &str, work: impl FnOnce() + Send + 'static) -> bool {
    std::thread::Builder::new()
        .name(name.into())
        .spawn(work)
        .is_ok()
}

/// Sends what `work` makes, or what `fallback` makes when `work` panics: every job and thread of
/// the page answers, so nothing it counts or waits on (a write under way, a reading, a picture
/// asked for) is left waiting for an answer that never comes.
pub(super) fn answer(tx: &Sender<Msg>, work: impl FnOnce() -> Msg, fallback: impl FnOnce() -> Msg) {
    let msg =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(work)).unwrap_or_else(|_| fallback());
    let _ = tx.send(msg);
}

/// The texture queue's job for the picture at `url`: as it starts it reads `offline`, the
/// page's own latest answer to whether the offline developer view is on
/// ([`Launcher::skins_offline`] stores it), and if the view came on since the job was queued,
/// nothing is fetched. That counts as a picture not had, asked for again once online.
pub(super) fn texture_job(
    tx: Sender<Msg>,
    url: String,
    offline: Arc<AtomicBool>,
    fetch: fn(&str) -> Option<Vec<u8>>,
) -> Job {
    Box::new(move || {
        answer(
            &tx,
            || {
                let png = (!offline.load(Ordering::Relaxed))
                    .then(|| fetch(&url))
                    .flatten();
                Msg::Picture(url.clone(), png)
            },
            || Msg::Picture(url.clone(), None),
        )
    })
}

/// What a job that stopped unexpectedly says.
pub(super) fn stopped() -> String {
    tr("The launcher stopped this unexpectedly. Try again.").to_string()
}

pub(super) type Job = Box<dyn FnOnce() + Send>;

/// Why a [`Queue`] didn't take some work.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Refused {
    /// It holds [`QUEUED`] jobs already.
    Full,
    /// Its thread couldn't be started.
    NoThread,
}

/// Work done in turn by one thread, started when first needed, that holds at most [`QUEUED`]
/// jobs: a profile with many capes, or a page of saved skins, costs one thread, not one each.
/// Work it refuses is reported by whoever asked, never waited for.
#[derive(Default)]
pub(crate) struct Queue {
    pub(super) jobs: Option<SyncSender<Job>>,
}

impl Queue {
    pub(super) fn push(&mut self, name: &str, job: Job) -> Result<(), Refused> {
        let job = match &self.jobs {
            Some(jobs) => match jobs.try_send(job) {
                Ok(()) => return Ok(()),
                Err(TrySendError::Full(_)) => return Err(Refused::Full),
                // Its thread is gone: another one takes over.
                Err(TrySendError::Disconnected(job)) => job,
            },
            None => job,
        };
        let (jobs, queued) = mpsc::sync_channel::<Job>(QUEUED);
        jobs.try_send(job).map_err(|_| Refused::Full)?;
        spawn(name, move || {
            for job in queued {
                // One job's panic doesn't take the rest of the queue with it.
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(job));
            }
        })
        .then_some(())
        .ok_or(Refused::NoThread)?;
        self.jobs = Some(jobs);
        Ok(())
    }
}
