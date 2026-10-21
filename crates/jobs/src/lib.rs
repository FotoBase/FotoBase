//! Bounded priority job queue: visible items first, background last,
//! throttling hook, cancellation (PLAN §3 rule 4).
//!
//! All heavy work (thumbnailing, metadata extraction, face detection) goes
//! through this queue so a 50k-photo library never exhausts memory: the queue
//! holds at most `capacity` entries (pending + one running job) and `enqueue`
//! waits for space instead of growing without bound.
//!
//! Contracts:
//! - Higher priority dequeues first; ties dequeue FIFO.
//! - Jobs with the same [`JobKey`] coalesce: a duplicate enqueue attaches to
//!   the queued/in-flight job and shares its outcome.
//! - A cancelled job's outcome is published as [`Outcome::Cancelled`] and never
//!   delivered as a success — jobs observe [`JobContext::cancelled`] and clean
//!   up partial output (write to a temp path, finalize by rename) so the
//!   on-disk cache stays consistent.
//! - The [`ThrottleHook`] stub gates dequeueing (battery throttling arrives in
//!   a later epic).

use std::cmp::Ordering as CmpOrdering;
use std::collections::{BinaryHeap, HashMap};
use std::future::{ready, Future};
use std::pin::Pin;
use std::sync::Arc;

use tokio::sync::{watch, Mutex, Notify};

use std::sync::Mutex as StdMutex;

/// Crate identity, used by the workspace smoke test.
pub const CRATE_NAME: &str = "fotobase-jobs";

/// Job priority: visible items first, background last (PLAN §3 rule 4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Priority {
    /// On screen right now — decode before anything else.
    Visible,
    /// Prefetch / reindex work — always runs after visible items.
    Background,
}

impl Priority {
    /// Higher rank dequeues first from the max-heap.
    fn rank(self) -> u8 {
        match self {
            Priority::Visible => 1,
            Priority::Background => 0,
        }
    }
}

/// Stable identity for a unit of work (e.g. content hash of a photo).
/// Jobs sharing a key coalesce.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct JobKey(Arc<str>);

impl From<&str> for JobKey {
    fn from(s: &str) -> Self {
        Self(Arc::from(s))
    }
}

impl std::fmt::Display for JobKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Outcome of a job.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome<T> {
    Done(T),
    Cancelled,
    Failed(String),
}

/// Cancellation signal handed to every job. Cooperative: the job observes it
/// via [`JobContext`] and must abort before its output becomes visible.
#[derive(Clone)]
pub struct CancelToken {
    rx: watch::Receiver<bool>,
    tx: Arc<watch::Sender<bool>>,
}

impl Default for CancelToken {
    fn default() -> Self {
        let (tx, rx) = watch::channel(false);
        Self { tx: Arc::new(tx), rx }
    }
}

impl CancelToken {
    pub fn cancel(&self) {
        let _ = self.tx.send(true);
    }

    pub fn is_cancelled(&self) -> bool {
        *self.rx.borrow()
    }

    /// Resolves as soon as the token is cancelled.
    pub async fn cancelled(&self) {
        let _ = self.rx.clone().wait_for(|c| *c).await;
    }
}

/// Context passed to a running job.
#[derive(Clone)]
pub struct JobContext {
    token: CancelToken,
}

impl JobContext {
    pub fn is_cancelled(&self) -> bool {
        self.token.is_cancelled()
    }

    /// Resolves as soon as the job is cancelled.
    pub async fn cancelled(&self) {
        self.token.cancelled().await;
    }
}

/// A unit of work: an async closure receiving the job context.
pub type Job<T> =
    Arc<dyn Fn(JobContext) -> Pin<Box<dyn Future<Output = Outcome<T>> + Send>> + Send + Sync>;

/// Build a [`Job`] from an async closure.
pub fn job<T, F, Fut>(f: F) -> Job<T>
where
    T: Send + 'static,
    F: Fn(JobContext) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Outcome<T>> + Send + 'static,
{
    Arc::new(move |ctx| Box::pin(f(ctx)))
}

/// Throttling hook. Called before the worker dequeues the next job; the
/// returned future resolves when the queue is allowed to proceed. The P1 stub
/// always admits immediately; battery detection plugs in here later.
pub trait ThrottleHook: Send + Sync + 'static {
    fn wait_admission(&self) -> Pin<Box<dyn Future<Output = ()> + Send + '_>>;
}

/// Default hook: never throttles.
pub struct NoThrottle;

impl ThrottleHook for NoThrottle {
    fn wait_admission(&self) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        Box::pin(ready(()))
    }
}

/// Errors from enqueueing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum EnqueueError {
    /// The queue is at capacity; `enqueue` waits, `try_enqueue` returns this.
    #[error("job queue is full")]
    Full,
    /// The queue was closed.
    #[error("job queue is closed")]
    Closed,
}

struct Shared<T> {
    result: Mutex<Option<Arc<Outcome<T>>>>,
    done: Notify,
}

impl<T> Shared<T> {
    fn new() -> Self {
        Self { result: Mutex::new(None), done: Notify::new() }
    }

    async fn publish(&self, outcome: Arc<Outcome<T>>) {
        let mut slot = self.result.lock().await;
        if slot.is_none() {
            *slot = Some(outcome);
        }
        drop(slot);
        self.done.notify_waiters();
    }
}

struct Entry<T> {
    key: JobKey,
    priority: Priority,
    seq: u64,
    job: Job<T>,
    token: CancelToken,
    shared: Arc<Shared<T>>,
}

// Max-heap: higher priority first, FIFO within a priority.
impl<T> PartialEq for Entry<T> {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == CmpOrdering::Equal
    }
}
impl<T> Eq for Entry<T> {}
impl<T> PartialOrd for Entry<T> {
    fn partial_cmp(&self, other: &Self) -> Option<CmpOrdering> {
        Some(self.cmp(other))
    }
}
impl<T> Ord for Entry<T> {
    fn cmp(&self, other: &Self) -> CmpOrdering {
        self.priority.rank().cmp(&other.priority.rank()).then_with(|| other.seq.cmp(&self.seq))
    }
}

struct State<T> {
    pending: BinaryHeap<Arc<Entry<T>>>,
    /// The job the worker is currently running, if any.
    running: Option<Arc<Entry<T>>>,
    /// Queued or running entries by key (coalescing index).
    map: HashMap<JobKey, Arc<Shared<T>>>,
    closed: bool,
    /// Monotonic sequence for FIFO ordering within a priority.
    next_seq: u64,
    /// Wakes the worker when a job is enqueued. Arc'd so waiters can create
    /// registration futures that outlive the state lock guard.
    work: Arc<Notify>,
    /// Wakes `enqueue` waiters when space frees up.
    space: Arc<Notify>,
}

struct Inner<T> {
    capacity: usize,
    throttle: Arc<dyn ThrottleHook>,
    /// Lock is never held across an await, so a std Mutex is safe to take
    /// from both sync (`try_enqueue`) and async contexts.
    state: StdMutex<State<T>>,
}

/// Bounded priority job queue with a single worker.
#[derive(Clone)]
pub struct Queue<T> {
    inner: Arc<Inner<T>>,
}

impl<T: Send + Sync + 'static> Queue<T> {
    /// Create a queue holding at most `capacity` entries (pending + running).
    /// Spawns the worker; must be called inside a tokio runtime.
    pub fn new(capacity: usize) -> Self {
        Self::with_throttle(capacity, Arc::new(NoThrottle))
    }

    pub fn with_throttle(capacity: usize, throttle: Arc<dyn ThrottleHook>) -> Self {
        assert!(capacity > 0, "queue capacity must be > 0");
        let inner = Arc::new(Inner {
            capacity,
            throttle,
            state: StdMutex::new(State {
                pending: BinaryHeap::new(),
                running: None,
                map: HashMap::new(),
                closed: false,
                next_seq: 0,
                work: Arc::new(Notify::new()),
                space: Arc::new(Notify::new()),
            }),
        });
        tokio::spawn(worker_loop(inner.clone()));
        Self { inner }
    }

    /// Enqueue a job, waiting for capacity if the queue is full. A job with
    /// the same key that is queued or in flight is coalesced: the returned
    /// handle resolves with that job's outcome instead of running again.
    pub async fn enqueue(
        &self,
        key: impl Into<JobKey>,
        priority: Priority,
        job: Job<T>,
    ) -> Result<JobHandle<T>, EnqueueError> {
        let key = key.into();
        loop {
            match self.try_enqueue_inner(key.clone(), priority, job.clone()) {
                Ok(handle) => return Ok(handle),
                Err(EnqueueError::Full) => {
                    // Register for the space-freed wakeup *before* re-checking
                    // the condition, so no wakeup can be missed.
                    let space = self.inner.state.lock().unwrap().space.clone();
                    let notified = space.notified();
                    tokio::pin!(notified);
                    notified.as_mut().enable();
                    let full = {
                        let state = self.inner.state.lock().unwrap();
                        state.pending.len() + usize::from(state.running.is_some())
                            >= self.inner.capacity
                    };
                    if full {
                        notified.await;
                    }
                }
                Err(err) => return Err(err),
            }
        }
    }

    /// Enqueue without waiting; fails with [`EnqueueError::Full`] at capacity.
    pub fn try_enqueue(
        &self,
        key: impl Into<JobKey>,
        priority: Priority,
        job: Job<T>,
    ) -> Result<JobHandle<T>, EnqueueError> {
        self.try_enqueue_inner(key.into(), priority, job)
    }

    fn try_enqueue_inner(
        &self,
        key: JobKey,
        priority: Priority,
        job: Job<T>,
    ) -> Result<JobHandle<T>, EnqueueError> {
        let mut state = self.inner.state.lock().expect("job queue state poisoned");
        if state.closed {
            return Err(EnqueueError::Closed);
        }
        if let Some(shared) = state.map.get(&key).cloned() {
            // Coalesce onto the queued/in-flight job.
            return Ok(JobHandle { shared, queue: Arc::downgrade(&self.inner), key });
        }
        if state.pending.len() + usize::from(state.running.is_some()) >= self.inner.capacity {
            return Err(EnqueueError::Full);
        }
        let entry = Arc::new(Entry {
            seq: state.next_seq,
            key: key.clone(),
            priority,
            job,
            token: CancelToken::default(),
            shared: Arc::new(Shared::<T>::new()),
        });
        state.next_seq += 1;
        state.map.insert(key.clone(), entry.shared.clone());
        state.pending.push(entry.clone());
        state.work.notify_one();
        Ok(JobHandle { shared: entry.shared.clone(), queue: Arc::downgrade(&self.inner), key })
    }

    /// Close the queue: pending entries are dropped (their handles get
    /// `Cancelled`), no new jobs are accepted. The running job finishes.
    pub async fn close(&self) {
        let drained: Vec<Arc<Entry<T>>> = {
            let mut state = self.inner.state.lock().expect("job queue state poisoned");
            state.closed = true;
            state.map.clear();
            // notify_one (not notify_waiters): it stores a permit when the
            // worker has not registered its waiter yet, so the wakeup cannot
            // be missed between the worker's condition check and its await.
            state.work.notify_one();
            state.pending.drain().collect()
        };
        for entry in drained {
            entry.shared.publish(Arc::new(Outcome::Cancelled)).await;
        }
    }
}

impl<T> Drop for Queue<T> {
    fn drop(&mut self) {
        // Best effort: stop the worker from waiting forever on an owned queue.
        if let Ok(mut state) = self.inner.state.try_lock() {
            state.closed = true;
            // Same permit-storing rationale as `close`.
            state.work.notify_one();
        }
    }
}

/// Handle to a queued or running job.
pub struct JobHandle<T> {
    shared: Arc<Shared<T>>,
    queue: std::sync::Weak<Inner<T>>,
    key: JobKey,
}

impl<T> std::fmt::Debug for JobHandle<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JobHandle").field("key", &self.key).finish_non_exhaustive()
    }
}

impl<T: Send + Sync + 'static> JobHandle<T> {
    /// Wait for the job's outcome.
    pub async fn result(&self) -> Arc<Outcome<T>> {
        loop {
            let notified = self.shared.done.notified();
            {
                let slot = self.shared.result.lock().await;
                if let Some(outcome) = slot.as_ref() {
                    return outcome.clone();
                }
            }
            notified.await;
        }
    }

    /// Cancel the job. If it is queued it is removed and never runs; if it is
    /// running its cancellation token fires — the job body keeps running until
    /// it observes the token (so its own cleanup can complete without being
    /// aborted mid-write) and the outcome is published as
    /// [`Outcome::Cancelled`], never as a success.
    pub fn cancel(&self) {
        let Some(queue) = self.queue.upgrade() else { return };
        let Ok(mut state) = queue.state.try_lock() else { return };
        if let Some(running) = state.running.as_ref() {
            if running.key == self.key {
                running.token.cancel();
                return;
            }
        }
        // Still queued: remove it and publish Cancelled.
        if let Some(shared) = state.map.get(&self.key).cloned() {
            if Arc::ptr_eq(&shared, &self.shared) {
                let remaining: Vec<Arc<Entry<T>>> =
                    state.pending.drain().filter(|e| e.key != self.key).collect();
                for e in remaining {
                    state.pending.push(e);
                }
                state.map.remove(&self.key);
                state.space.notify_waiters();
                drop(state);
                tokio::spawn(
                    async move { shared.publish(Arc::new(Outcome::<T>::Cancelled)).await },
                );
            }
        }
    }
}

async fn worker_loop<T: Send + Sync + 'static>(inner: Arc<Inner<T>>) {
    enum Action<T> {
        Run(Arc<Entry<T>>),
        Wait(Arc<Notify>),
        Exit,
    }
    loop {
        inner.throttle.wait_admission().await;
        // The state lock must never be alive across an await point, so the
        // decision is made under the lock and acted on after it is released.
        let action = {
            let mut state = inner.state.lock().expect("job queue state poisoned");
            match state.pending.pop() {
                Some(entry) => {
                    state.running = Some(entry.clone());
                    Action::Run(entry)
                }
                None if state.closed => Action::Exit,
                // notify_one stores a permit when no waiter is registered, so
                // awaiting after the lock is dropped cannot miss a wakeup.
                None => Action::Wait(state.work.clone()),
            }
        };
        let entry = match action {
            Action::Exit => return,
            Action::Wait(work) => {
                work.notified().await;
                continue;
            }
            Action::Run(entry) => entry,
        };

        let ctx = JobContext { token: entry.token.clone() };
        let outcome = (entry.job)(ctx).await;
        // A cancelled job never publishes a success: partial output stays
        // invisible to consumers regardless of what the job body returned.
        let outcome = if entry.token.is_cancelled() { Outcome::Cancelled } else { outcome };
        entry.shared.publish(Arc::new(outcome)).await;

        let mut state = inner.state.lock().expect("job queue state poisoned");
        if state.running.as_ref().is_some_and(|r| Arc::ptr_eq(r, &entry)) {
            state.running = None;
        }
        state.map.remove(&entry.key);
        state.space.notify_waiters();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crate_identity() {
        assert_eq!(CRATE_NAME, "fotobase-jobs");
    }

    #[test]
    fn visible_sorts_before_background() {
        assert!(Priority::Visible.rank() > Priority::Background.rank());
    }

    #[tokio::test]
    async fn cancel_token_signals_waiters() {
        let token = CancelToken::default();
        let rx = token.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            rx.cancel();
        });
        token.cancelled().await;
        assert!(token.is_cancelled());
    }
}
