use once_cell::sync::Lazy;
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, RwLock,
    },
    thread,
};

struct ThreadPoolWorker {
    is_busy: Arc<AtomicBool>,
    sender: mpsc::Sender<Box<dyn FnOnce() + Send>>,
}

struct ThreadPool {
    workers: RwLock<Vec<ThreadPoolWorker>>,
}

impl ThreadPool {
    pub fn new() -> Self {
        let pool = ThreadPool {
            workers: RwLock::new(Vec::new()),
        };
        pool.ensure_worker();
        pool
    }

    fn ensure_worker(&self) {
        let mut workers = self.workers.write().unwrap();
        if workers.is_empty() {
            workers.push(Self::spawn_worker());
        }
    }

    fn spawn_worker() -> ThreadPoolWorker {
        let (tx, rx) = mpsc::channel::<Box<dyn FnOnce() + Send>>();
        let is_busy = Arc::new(AtomicBool::new(false));
        let busy_flag = Arc::clone(&is_busy);
        thread::spawn(move || {
            for job in rx {
                busy_flag.store(true, Ordering::SeqCst);
                job();
                busy_flag.store(false, Ordering::SeqCst);
            }
        });
        ThreadPoolWorker {
            is_busy,
            sender: tx,
        }
    }

    pub fn execute<F>(&self, job: F)
    where
        F: FnOnce() + Send + 'static,
    {
        let workers = self.workers.read().unwrap();
        if let Some(worker) = workers.iter().find(|w| !w.is_busy.load(Ordering::SeqCst)) {
            // try to mark as busy (CAS avoids rare races)
            if worker
                .is_busy
                .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
            {
                worker.sender.send(Box::new(job)).unwrap();
                return;
            }
        }
        drop(workers);
        // all busy or race lost, spawn a new thread
        let new_worker = Self::spawn_worker();
        new_worker.is_busy.store(true, Ordering::SeqCst);
        new_worker.sender.send(Box::new(job)).unwrap();
        let mut workers = self.workers.write().unwrap();
        workers.push(new_worker);
    }
}

static GLOBAL_THREAD_POOL: Lazy<Arc<ThreadPool>> = Lazy::new(|| Arc::new(ThreadPool::new()));

pub(crate) fn spawn_on_global_pool<F>(job: F)
where
    F: FnOnce() + Send + 'static,
{
    GLOBAL_THREAD_POOL.execute(job);
}
