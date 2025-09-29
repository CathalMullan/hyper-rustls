//! TCP connection

use std::{
    future::Future,
    io,
    pin::Pin,
    task::{Context, Poll},
};

use http::Uri;
use hyper::rt;
use tower_service::Service;

#[cfg(feature = "tokio")]
mod tokio;
#[cfg(feature = "tokio")]
pub use self::tokio::tokio;

#[cfg(feature = "smol")]
mod smol;
#[cfg(feature = "smol")]
pub use self::smol::smol;

/// Trait for TCP connection establishment
pub trait TcpConnect: Clone + Send {
    /// The stream type returned by this connector
    type Stream: rt::Read + rt::Write + Unpin + Send + 'static;

    /// Connect to the given host and port
    fn connect(
        &self,
        host: String,
        port: u16,
    ) -> Pin<Box<dyn Future<Output = io::Result<Self::Stream>> + Send>>;
}

/// A TCP connector service
#[derive(Clone)]
pub struct TcpConnector<C> {
    inner: C,
}

impl<C> TcpConnector<C>
where
    C: TcpConnect,
{
    /// Create a new TCP connector with the given connection implementation
    #[allow(unused)]
    pub(crate) fn new(inner: C) -> Self {
        TcpConnector { inner }
    }
}

impl<C> Service<Uri> for TcpConnector<C>
where
    C: TcpConnect + Send + 'static,
    C::Stream: Send,
{
    type Response = C::Stream;
    type Error = io::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, uri: Uri) -> Self::Future {
        let host = match uri.host() {
            Some(host) => host.to_string(),
            None => {
                return Box::pin(async {
                    Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "URI missing host",
                    ))
                })
            }
        };

        let port = uri
            .port_u16()
            .unwrap_or(match uri.scheme_str() {
                Some("https") => 443,
                Some("http") => 80,
                _ => 80,
            });

        self.inner.connect(host, port)
    }
}
