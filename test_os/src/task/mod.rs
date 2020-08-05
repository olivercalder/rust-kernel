use core::{future::Future, pin::Pin, task::{Context, Poll}};
use alloc::boxed::Box;

pub mod simple_executor;
pub mod keyboard;

pub struct Task {   // newtype wrapper around a pinned, heap allocated, dynamically dispatched future
    future: Pin<Box<dyn Future<Output = ()>>>,
}

impl Task {
    pub fn new(future: impl Future<Output = ()> + 'static) -> Task {
        Task {
            future: Box::pin(future),
            // takes arbitrary future with output type ()
            // pins to memory using Box::pin
            // wraps the boxed future in the Task struct and returns it
        }
    }

    fn poll(&mut self, context: &mut Context) -> Poll<()> {
        self.future.as_mut().poll(context)
        // use Pin::as_mut to convert self.future from type Pin<Box<T>> to type Pin<&mut T>
        // then call poll on the converted self.future field and return the result
    }
}
