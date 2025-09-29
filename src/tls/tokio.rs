//! Tokio TLS implementation using tokio-rustls

use std::{
    error::Error,
    future::Future,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use http::Uri;
use hyper_util::rt::TokioIo;
use rustls::ClientConfig;
use tokio::net::TcpStream;
use tokio_rustls::{client::TlsStream, TlsConnector};
use tower_layer::Layer;
use tower_service::Service;

use crate::tls::{alpn::AlpnConfigured, DefaultServerNameResolver, ResolveServerName};

/// Create a Tokio TLS layer
pub fn tokio(config: ClientConfig) -> TokioTlsLayer {
    TokioTlsLayer {
        config: Arc::new(config),
        resolver: Arc::new(DefaultServerNameResolver),
    }
}

/// Create a Tokio TLS layer with a custom server name resolver
pub fn tokio_with_resolver<R>(config: ClientConfig, resolver: R) -> TokioTlsLayer
where
    R: ResolveServerName + 'static,
{
    TokioTlsLayer {
        config: Arc::new(config),
        resolver: Arc::new(resolver),
    }
}

pub trait TlsSource: Send + 'static {
    fn into_parts(self) -> (TokioIo<TcpStream>, Option<Vec<Vec<u8>>>);
}

impl TlsSource for TokioIo<TcpStream> {
    fn into_parts(self) -> (TokioIo<TcpStream>, Option<Vec<Vec<u8>>>) {
        (self, None)
    }
}

impl TlsSource for AlpnConfigured<TokioIo<TcpStream>> {
    fn into_parts(self) -> (TokioIo<TcpStream>, Option<Vec<Vec<u8>>>) {
        (self.inner, Some(self.protocols))
    }
}

/// Tokio TLS layer
#[derive(Clone)]
pub struct TokioTlsLayer {
    config: Arc<ClientConfig>,
    resolver: Arc<dyn ResolveServerName>,
}

/// Tokio TLS service
#[derive(Clone)]
pub struct TokioTlsService<S> {
    inner: S,
    config: Arc<ClientConfig>,
    resolver: Arc<dyn ResolveServerName>,
}

impl<S> Layer<S> for TokioTlsLayer {
    type Service = TokioTlsService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        TokioTlsService {
            inner,
            config: self.config.clone(),
            resolver: self.resolver.clone(),
        }
    }
}

impl<S, R> Service<Uri> for TokioTlsService<S>
where
    S: Service<Uri, Response = R> + Send + Clone + 'static,
    S::Error: Error + Send + Sync + 'static,
    S::Future: Send,
    R: TlsSource,
{
    type Response = TokioIo<TlsStream<TcpStream>>;
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

            Ok(TokioIo::new(tls))
        })
    }
}
