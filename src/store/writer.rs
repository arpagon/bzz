use tokio::sync::{mpsc, oneshot};

use crate::{
    error::{Error, Result},
    store::Store,
};

type Job = Box<dyn FnOnce(&mut Store) + Send + 'static>;
const STORE_QUEUE_CAPACITY: usize = 256;

#[derive(Clone)]
pub struct StoreHandle {
    sender: mpsc::Sender<Job>,
}

impl StoreHandle {
    pub fn spawn(store: Store) -> Result<Self> {
        let (sender, mut receiver) = mpsc::channel::<Job>(STORE_QUEUE_CAPACITY);
        std::thread::Builder::new()
            .name("bzz-sqlite".into())
            .spawn(move || {
                let mut store = store;
                while let Some(job) = receiver.blocking_recv() {
                    job(&mut store);
                }
            })
            .map_err(|error| Error::io("SQLite owner thread", error))?;
        Ok(Self { sender })
    }

    pub async fn call<T, F>(&self, operation: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(&mut Store) -> Result<T> + Send + 'static,
    {
        let (sender, receiver) = oneshot::channel();
        self.sender
            .send(Box::new(move |store| {
                let _ = sender.send(operation(store));
            }))
            .await
            .map_err(|_| Error::Database(rusqlite::Error::InvalidQuery))?;
        receiver
            .await
            .map_err(|_| Error::Database(rusqlite::Error::InvalidQuery))?
    }
}
