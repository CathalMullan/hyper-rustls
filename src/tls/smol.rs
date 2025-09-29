//! Smol TLS implementation using futures-rustls

use std::{
    error::Error,
    future::Future,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use futures_rustls::{client::TlsStream, TlsConnector};
use http::Uri;
use rustls::ClientConfig;
use smol::net::TcpStream;
use smol_hyper::rt::FuturesIo;
use tower_layer::Layer;
use tower_service::Service;

use crate::tls::{alpn::AlpnConfigured, DefaultServerNameResolver, ResolveServerName};

/// Create a Smol TLS layer
pub fn smol(config: ClientConfig) -> SmolTlsLayer {
    SmolTlsLayer {
        config: Arc::new(config),
        resolver: Arc::new(DefaultServerNameResolver),
    }
}

/// Create a Smol TLS layer with a custom server name resolver
pub fn smol_with_resolver<R>(config: ClientConfig, resolver: R) -> SmolTlsLayer
where
    R: ResolveServerName + 'static,
{
    SmolTlsLayer {
        config: Arc::new(config),
        resolver: Arc::new(resolver),
    }
}

pub trait TlsSource: Send + 'static {
    fn into_parts(self) -> (FuturesIo<TcpStream>, Option<Vec<Vec<u8>>>);
}

impl TlsSource for FuturesIo<TcpStream> {
    fn into_parts(self) -> (FuturesIo<TcpStream>, Option<Vec<Vec<u8>>>) {
        (self, None)
    }
}

impl TlsSource for AlpnConfigured<FuturesIo<TcpStream>> {
    fn into_parts(self) -> (FuturesIo<TcpStream>, Option<Vec<Vec<u8>>>) {
        (self.inner, Some(self.protocols))
    }
}

/// Smol TLS layer
#[derive(Clone)]
pub struct SmolTlsLayer {
    config: Arc<ClientConfig>,
    resolver: Arc<dyn ResolveServerName>,
}

/// Smol TLS service
#[derive(Clone)]
pub struct SmolTlsService<S> {
    inner: S,
    config: Arc<ClientConfig>,
    resolver: Arc<dyn ResolveServerName>,
}

impl<S> Layer<S> for SmolTlsLayer {
    type Service = SmolTlsService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        SmolTlsService {
            inner,
            config: self.config.clone(),
            resolver: self.resolver.clone(),
        }
    }
}

impl<S, R> Service<Uri> for SmolTlsService<S>
where
    S: Service<Uri, Response = R> + Send + Clone + 'static,
    S::Error: Error + Send + Sync + 'static,
    S::Future: Send,
    R: TlsSource,
{
    type Response = FuturesIo<TlsStream<TcpStream>>;
    type Error = Box<dyn Error + Send>;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner
            .poll_ready(cx)
            .map_err(|err| Box::new(err) as Box<dyn Error + Send>)
    }

    fn call(&mut self, uri: Uri) -> Self::Future {
        let name = match self.resolver.resolve(&uri) {
            Ok(name) => name,
            Err(err) => {
                return Box::pin(async move { Err(Box::new(err) as _) });
            }
        };

        let config = self.config.clone();
        let mut inner = self.inner.clone();

        Box::pin(async move {
            let response = inner
                .call(uri)
                .await
                .map_err(|err| Box::new(err) as Box<dyn Error + Send>)?;

            let (stream, alpn) = response.into_parts();

            let config = if let Some(protocols) = alpn {
                let mut config = (*config).clone();
                config.alpn_protocols = protocols;
                Arc::new(config)
            } else {
                config
            };

            let connector = TlsConnector::from(config);
            let tcp = stream.into_inner();

            let tls = connector
                .connect(name, tcp)
                .await
                .map_err(|err| Box::new(err) as Box<dyn Error + Send>)?;

            Ok(FuturesIo::new(tls))
        })
    }
}
