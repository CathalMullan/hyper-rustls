//! ALPN configuration

use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

use http::Uri;
use pin_project_lite::pin_project;
use tower_layer::Layer;
use tower_service::Service;

/// Create an ALPN configuration layer
pub fn alpn<I>(protocols: I) -> AlpnLayer
where
    I: IntoIterator,
    I::Item: AsRef<[u8]>,
{
    AlpnLayer {
        protocols: protocols
            .into_iter()
            .map(|protocol| protocol.as_ref().to_vec())
            .collect(),
    }
}

/// ALPN configuration layer
#[derive(Clone)]
pub struct AlpnLayer {
    protocols: Vec<Vec<u8>>,
}

impl<S> Layer<S> for AlpnLayer {
    type Service = AlpnService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        AlpnService {
            inner,
            protocols: self.protocols.clone(),
        }
    }
}

/// ALPN configuration service
#[derive(Clone)]
pub struct AlpnService<S> {
    inner: S,
    protocols: Vec<Vec<u8>>,
}

/// Wrapper that carries ALPN configuration with the response
pub struct AlpnConfigured<T> {
    pub(crate) inner: T,
    pub(crate) protocols: Vec<Vec<u8>>,
}

impl<S> Service<Uri> for AlpnService<S>
where
    S: Service<Uri>,
{
    type Response = AlpnConfigured<S::Response>;
    type Error = S::Error;
    type Future = AlpnFuture<S::Future>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, uri: Uri) -> Self::Future {
        AlpnFuture {
            inner: self.inner.call(uri),
            protocols: self.protocols.clone(),
        }
    }
}

pin_project! {
    pub struct AlpnFuture<F> {
        #[pin]
        inner: F,
        protocols: Vec<Vec<u8>>,
    }
}

impl<F, T, E> Future for AlpnFuture<F>
where
    F: Future<Output = Result<T, E>>,
{
    type Output = Result<AlpnConfigured<T>, E>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        match this.inner.poll(cx) {
            Poll::Ready(Ok(inner)) => Poll::Ready(Ok(AlpnConfigured {
                inner,
                protocols: this.protocols.clone(),
            })),
            Poll::Ready(Err(err)) => Poll::Ready(Err(err)),
            Poll::Pending => Poll::Pending,
        }
    }
}
