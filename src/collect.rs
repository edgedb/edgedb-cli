use std::sync::mpsc::{channel, Sender, Receiver};

pub struct Collector<T> {
    sender: Sender<T>,
    receiver: Receiver<T>,
}

impl<T> Collector<T> {
    pub fn new() -> Collector<T> {
        let (sender, receiver) = channel();
        Collector { sender, receiver }
    }
    pub fn add(&self, value: T) {
        self.sender.send(value)
            .expect("can always send in an unbounded channel")
    }
    pub fn list(self) -> Vec<T> {
        let Collector { sender, receiver } = self;
        drop(sender);
        return receiver.into_iter().collect();
    }
}
