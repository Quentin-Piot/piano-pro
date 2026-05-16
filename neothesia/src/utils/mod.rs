use std::{future::Future, pin::Pin};

pub use neothesia_core::utils::*;

#[cfg(desktop)]
pub type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send + 'static>>;
#[cfg(not(desktop))]
pub type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + 'static>>;

pub mod task;
pub mod window;
