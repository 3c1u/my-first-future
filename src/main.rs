use std::future::Future;
use std::pin::Pin;
use std::sync::mpsc::{sync_channel, Receiver, SyncSender};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

pub mod timer;

static WAKER_VTABLE: RawWakerVTable = RawWakerVTable::new(
    MyWaker::_clone,
    MyWaker::_wake,
    MyWaker::_wake_by_ref,
    MyWaker::_drop,
);

#[derive(Clone)]
struct MyWaker {
    task: Arc<Task>
}

impl Into<RawWaker> for MyWaker {
    fn into(self) -> RawWaker {
        let waker = Box::into_raw(Box::new(self));
        RawWaker::new(waker as *const (), &WAKER_VTABLE)
    }
}

impl Into<Waker> for MyWaker {
    fn into(self) -> Waker {
        unsafe { Waker::from_raw(<Self as Into<RawWaker>>::into(self)) }
    }
}

impl MyWaker {
    pub fn new(task: Arc<Task>) -> Self {
        MyWaker { task }
    }

    unsafe fn _clone(_self: *const ()) -> RawWaker {
        let _self = (_self as *const Self).as_ref().unwrap();
        _self.clone().into()
    }

    unsafe fn _wake(_self: *const ()) {
        let _self = Box::from_raw(_self as *mut Self);
        _self.wake();
    }

    unsafe fn _wake_by_ref(_self: *const ()) {
        let _self = (_self as *const Self).as_ref().unwrap();
        _self.wake();
    }

    unsafe fn _drop(_self: *const ()) {
        Box::from_raw(_self as *mut Self);
    }

    fn wake(&self) {
        self.task.sender.send(self.task.clone()).unwrap();
    }
}

pub struct Executor {
    receiver: Receiver<Arc<Task>>,
}

pub struct Spawner {
    sender: SyncSender<Arc<Task>>,
}

struct Task {
    future: Mutex<Option<Pin<Box<dyn Future<Output = ()> + Send>>>>,
    sender: SyncSender<Arc<Task>>,
}

impl Executor {
    pub fn run(&self) {
        const TOTAL_THREADS: usize = 4;
        let (poller_s, poller_r) = sync_channel(TOTAL_THREADS);

        for _ in 0..TOTAL_THREADS {
            let (s, r) = sync_channel(1);
            let p_s = poller_s.clone();
            let s_ = s.clone();

            std::thread::spawn(move || {
                while let Ok(task) = r.recv() {
                    let task: Arc<Task> = task;
                    let mut future = task.future.lock().unwrap();

                    if let Some(mut f) = future.take() {
                        let waker = MyWaker::new(task.clone()).into();
                        let mut ctx = Context::from_waker(&waker);
    
                        if let Poll::Pending = f.as_mut().poll(&mut ctx) {
                            *future = Some(f);
                        }
                    }
                    
                    p_s.send(s_.clone()).unwrap();
                }
            });

            poller_s.send(s).unwrap();
        }

        while let Ok(task) = self.receiver.recv() {
            if let Ok(poller) = poller_r.recv() {
                poller.send(task).unwrap();
            }
        }
    }
}

impl Spawner {
    pub fn spawn<F>(&self, future: F)
    where
        F: Future<Output = ()> + 'static + Send,
    {
        let task = Task {
            future: Mutex::new(Some(Pin::from(Box::new(future)))),
            sender: self.sender.clone(),
        };

        self.sender.send(Arc::new(task)).unwrap();
    }
}

pub fn new_runtime() -> (Executor, Spawner) {
    let (sender, receiver) = sync_channel(1_000);
    (Executor { receiver }, Spawner { sender })
}

async fn async_something() {
    /* なんかすごいことをする */
    async_something_2().await;
}

async fn async_something_2() {
    use timer::TimerFuture;
    use std::time::Duration;

    for _ in 0..5usize {
        println!("[{:?}] 仙狐さんもふもふ！", std::thread::current().id());
        TimerFuture::new(Duration::from_millis(100)).await;
    }
}

fn main() {
    let (e, s) = new_runtime();

    s.spawn(async_something());
    s.spawn(async_something());
    s.spawn(async_something());
    s.spawn(async_something());

    drop(s);

    e.run();
}
