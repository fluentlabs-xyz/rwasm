use crate::TrapCode;
use std::{
    cell::RefCell,
    sync::mpsc::{self, Receiver, Sender},
    thread,
};
use wasmtime::Func;

thread_local! {
    static WORKER: RefCell<Option<Sender<WorkerMessage>>> = RefCell::new(None);
}

struct WorkerMessage {
    data: Func,
    resp: Sender<TrapCode>,
}

fn get_or_spawn_worker() -> Sender<WorkerMessage> {
    WORKER.with(|worker_opt| {
        let mut maybe_sender = worker_opt.borrow_mut();
        if let Some(sender) = &*maybe_sender {
            return sender.clone();
        }
        let (tx, rx): (Sender<WorkerMessage>, Receiver<WorkerMessage>) = mpsc::channel();
        thread::spawn(move || {
            while let Ok(msg) = rx.recv() {
                // let result = format!("Processed: {}", msg.data);
                // thread::sleep(Duration::from_millis(100));
                // let _ = msg.resp.send(result);
            }
        });
        *maybe_sender = Some(tx.clone());
        tx
    })
}
