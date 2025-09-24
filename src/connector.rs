use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::{fmt, io};

use http::Uri;
use hyper::rt;
use hyper_util::client::legacy::connect::Connection;
use pki_types::ServerName;
use tower_service::Service;

use crate::stream::MaybeHttpsStream;

#[cfg(feature = "tokio")]
use hyper_util::rt::TokioIo;
#[cfg(feature = "tokio")]
use tokio_rustls::TlsConnector;

#[cfg(feature = "smol")]
use futures_rustls::TlsConnector;
#[cfg(feature = "smol")]
use smol_hyper::rt::FuturesIo;

pub(crate) mod builder;

type BoxError = Box<dyn std::error::Error + Send + Sync>;

/// A Connector for the `https` scheme.
#[derive(Clone)]
pub struct HttpsConnector<T> {
    force_https: bool,
    http: T,
    tls_config: Arc<rustls::ClientConfig>,
    server_name_resolver: Arc<dyn ResolveServerName + Sync + Send>,
}

impl<T> HttpsConnector<T> {
    /// Creates a [`crate::HttpsConnectorBuilder`] to configure a `HttpsConnector`.
    ///
    /// This is the same as [`crate::HttpsConnectorBuilder::new()`].
    pub fn builder() -> builder::ConnectorBuilder<builder::WantsTlsConfig> {
        builder::ConnectorBuilder::new()
    }

    /// Creates a new `HttpsConnector`.
    ///
    /// The recommended way to create a `HttpsConnector` is to use a [`crate::HttpsConnectorBuilder`]. See [`HttpsConnector::builder()`].
    pub fn new(
        http: T,
        tls_config: impl Into<Arc<rustls::ClientConfig>>,
        force_https: bool,
        server_name_resolver: Arc<dyn ResolveServerName + Send + Sync>,
    ) -> Self {
        Self {
            http,
            tls_config: tls_config.into(),
            force_https,
            server_name_resolver,
        }
    }

    /// Force the use of HTTPS when connecting.
    ///
    /// If a URL is not `https` when connecting, an error is returned.
    pub fn enforce_https(&mut self) {
        self.force_https = true;
    }
}

#[cfg(feature = "tokio")]
impl<T> Service<Uri> for HttpsConnector<T>
where
    T: Service<Uri>,
    T::Response: Connection + rt::Read + rt::Write + Send + Unpin + 'static,
    T::Future: Send + 'static,
    T::Error: Into<BoxError>,
{
    type Response = MaybeHttpsStream<T::Response>;
    type Error = BoxError;

    #[allow(clippy::type_complexity)]
    type Future =
        Pin<Box<dyn Future<Output = Result<MaybeHttpsStream<T::Response>, BoxError>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        match self.http.poll_ready(cx) {
            Poll::Ready(Ok(())) => Poll::Ready(Ok(())),
            Poll::Ready(Err(e)) => Poll::Ready(Err(e.into())),
            Poll::Pending => Poll::Pending,
        }
    }

    fn call(&mut self, dst: Uri) -> Self::Future {
        // dst.scheme() would need to derive Eq to be matchable;
        // use an if cascade instead
        match dst.scheme() {
            Some(scheme) if scheme == &http::uri::Scheme::HTTP && !self.force_https => {
                let future = self.http.call(dst);
                return Box::pin(async move {
                    Ok(MaybeHttpsStream::Http(future.await.map_err(Into::into)?))
                });
            }
            Some(scheme) if scheme != &http::uri::Scheme::HTTPS => {
                let message = format!("unsupported scheme {scheme}");
                return Box::pin(async move {
                    Err(io::Error::new(io::ErrorKind::Other, message).into())
                });
            }
            Some(_) => {}
            None => {
                return Box::pin(async move {
                    Err(io::Error::new(io::ErrorKind::Other, "missing scheme").into())
                })
            }
        };

        let cfg = self.tls_config.clone();
        let hostname = match self.server_name_resolver.resolve(&dst) {
            Ok(hostname) => hostname,
            Err(e) => {
                return Box::pin(async move { Err(e) });
            }
        };

        let connecting_future = self.http.call(dst);
        Box::pin(async move {
            let tcp = connecting_future
                .await
                .map_err(Into::into)?;

            let stream = MaybeHttpsStream::Https(TokioIo::new(
                TlsConnector::from(cfg)
                    .connect(hostname, TokioIo::new(tcp))
                    .await
                    .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?,
            ));

            Ok(stream)
        })
    }
}

#[cfg(feature = "smol")]
impl<T> Service<Uri> for HttpsConnector<T>
where
    T: Service<Uri>,
    T::Response: Connection
        + rt::Read
        + rt::Write
        + smol::io::AsyncRead
        + smol::io::AsyncWrite
        + Send
        + Unpin
        + 'static,
    T::Future: Send + 'static,
    T::Error: Into<BoxError>,
{
    type Response = MaybeHttpsStream<T::Response>;
    type Error = BoxError;

    #[allow(clippy::type_complexity)]
    type Future =
        Pin<Box<dyn Future<Output = Result<MaybeHttpsStream<T::Response>, BoxError>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        match self.http.poll_ready(cx) {
            Poll::Ready(Ok(())) => Poll::Ready(Ok(())),
            Poll::Ready(Err(e)) => Poll::Ready(Err(e.into())),
            Poll::Pending => Poll::Pending,
        }
    }

    fn call(&mut self, dst: Uri) -> Self::Future {
        // dst.scheme() would need to derive Eq to be matchable;
        // use an if cascade instead
        match dst.scheme() {
            Some(scheme) if scheme == &http::uri::Scheme::HTTP && !self.force_https => {
                let future = self.http.call(dst);
                return Box::pin(async move {
                    Ok(MaybeHttpsStream::Http(future.await.map_err(Into::into)?))
                });
            }
            Some(scheme) if scheme != &http::uri::Scheme::HTTPS => {
                let message = format!("unsupported scheme {scheme}");
                return Box::pin(async move {
                    Err(io::Error::new(io::ErrorKind::Other, message).into())
                });
            }
            Some(_) => {}
            None => {
                return Box::pin(async move {
                    Err(io::Error::new(io::ErrorKind::Other, "missing scheme").into())
                })
            }
        };

        let cfg = self.tls_config.clone();
        let hostname = match self.server_name_resolver.resolve(&dst) {
            Ok(hostname) => hostname,
            Err(e) => {
                return Box::pin(async move { Err(e) });
            }
        };

        let connecting_future = self.http.call(dst);
        Box::pin(async move {
            let tcp = connecting_future
                .await
                .map_err(Into::into)?;

            let stream = MaybeHttpsStream::Https(FuturesIo::new(
                TlsConnector::from(cfg)
                    .connect(hostname, tcp)
                    .await
                    .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?,
            ));

            Ok(stream)
        })
    }
}

impl<H, C> From<(H, C)> for HttpsConnector<H>
where
    C: Into<Arc<rustls::ClientConfig>>,
{
    fn from((http, cfg): (H, C)) -> Self {
        Self {
            force_https: false,
            http,
            tls_config: cfg.into(),
            server_name_resolver: Arc::new(DefaultServerNameResolver::default()),
        }
    }
}

impl<T> fmt::Debug for HttpsConnector<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("HttpsConnector")
            .field("force_https", &self.force_https)
            .finish()
    }
}

/// The default server name resolver, which uses the hostname in the URI.
#[derive(Default)]
pub struct DefaultServerNameResolver(());

impl ResolveServerName for DefaultServerNameResolver {
    fn resolve(
        &self,
        uri: &Uri,
    ) -> Result<ServerName<'static>, Box<dyn std::error::Error + Sync + Send>> {
        let mut hostname = uri.host().unwrap_or_default();

        // Remove square brackets around IPv6 address.
        if let Some(trimmed) = hostname
            .strip_prefix('[')
            .and_then(|h| h.strip_suffix(']'))
        {
            hostname = trimmed;
        }

        ServerName::try_from(hostname.to_string()).map_err(|e| Box::new(e) as _)
    }
}

/// A server name resolver which always returns the same fixed name.
pub struct FixedServerNameResolver {
    name: ServerName<'static>,
}

impl FixedServerNameResolver {
    /// Creates a new resolver returning the specified name.
    pub fn new(name: ServerName<'static>) -> Self {
        Self { name }
    }
}

impl ResolveServerName for FixedServerNameResolver {
    fn resolve(
        &self,
        _: &Uri,
    ) -> Result<ServerName<'static>, Box<dyn std::error::Error + Sync + Send>> {
        Ok(self.name.clone())
    }
}

impl<F, E> ResolveServerName for F
where
    F: Fn(&Uri) -> Result<ServerName<'static>, E>,
    E: Into<Box<dyn std::error::Error + Sync + Send>>,
{
    fn resolve(
        &self,
        uri: &Uri,
    ) -> Result<ServerName<'static>, Box<dyn std::error::Error + Sync + Send>> {
        self(uri).map_err(Into::into)
    }
}

/// A trait implemented by types that can resolve a [`ServerName`] for a request.
pub trait ResolveServerName {
    /// Maps a [`Uri`] into a [`ServerName`].
    fn resolve(
        &self,
        uri: &Uri,
    ) -> Result<ServerName<'static>, Box<dyn std::error::Error + Sync + Send>>;
}

#[cfg(all(
    test,
    feature = "http1",
    any(feature = "tokio", feature = "smol"),
    any(feature = "ring", feature = "aws-lc-rs"),
    any(
        feature = "rustls-native-certs",
        feature = "webpki-roots",
        feature = "rustls-platform-verifier",
    )
))]
mod tests {
    use std::future::poll_fn;

    use http::Uri;

    use super::*;
    use crate::{ConfigBuilderExt, HttpsConnectorBuilder, MaybeHttpsStream};

    #[cfg(feature = "tokio")]
    use hyper_util::rt::TokioIo;
    #[cfg(feature = "tokio")]
    use tokio::net::TcpStream;

    #[cfg(feature = "smol")]
    use smol::net::TcpStream;

    #[cfg(feature = "tokio")]
    #[tokio::test]
    async fn connects_https() {
        connect_tokio(Allow::Any, Scheme::Https)
            .await
            .unwrap();
    }

    #[cfg(feature = "tokio")]
    #[tokio::test]
    async fn connects_http() {
        connect_tokio(Allow::Any, Scheme::Http)
            .await
            .unwrap();
    }

    #[cfg(feature = "tokio")]
    #[tokio::test]
    async fn connects_https_only() {
        connect_tokio(Allow::Https, Scheme::Https)
            .await
            .unwrap();
    }

    #[cfg(feature = "tokio")]
    #[tokio::test]
    async fn enforces_https_only() {
        let message = connect_tokio(Allow::Https, Scheme::Http)
            .await
            .unwrap_err()
            .to_string();

        assert_eq!(message, "unsupported scheme http");
    }

    #[cfg(feature = "smol")]
    #[macro_rules_attribute::apply(smol_macros::test!)]
    async fn smol_connects_https() {
        connect_smol(Allow::Any, Scheme::Https)
            .await
            .unwrap();
    }

    #[cfg(feature = "smol")]
    #[macro_rules_attribute::apply(smol_macros::test!)]
    async fn smol_connects_http() {
        connect_smol(Allow::Any, Scheme::Http)
            .await
            .unwrap();
    }

    #[cfg(feature = "smol")]
    #[macro_rules_attribute::apply(smol_macros::test!)]
    async fn smol_connects_https_only() {
        connect_smol(Allow::Https, Scheme::Https)
            .await
            .unwrap();
    }

    #[cfg(feature = "smol")]
    #[macro_rules_attribute::apply(smol_macros::test!)]
    async fn smol_enforces_https_only() {
        let message = connect_smol(Allow::Https, Scheme::Http)
            .await
            .unwrap_err()
            .to_string();

        assert_eq!(message, "unsupported scheme http");
    }

    #[cfg(feature = "tokio")]
    async fn connect_tokio(
        allow: Allow,
        scheme: Scheme,
    ) -> Result<MaybeHttpsStream<TokioIo<TcpStream>>, BoxError> {
        let config_builder = rustls::ClientConfig::builder();
        cfg_if::cfg_if! {
            if #[cfg(feature = "rustls-platform-verifier")] {
                let config_builder = config_builder.try_with_platform_verifier()?;
            } else if #[cfg(feature = "rustls-native-certs")] {
                let config_builder = config_builder.with_native_roots().unwrap();
            } else if #[cfg(feature = "webpki-roots")] {
                let config_builder = config_builder.with_webpki_roots();
            }
        }
        let config = config_builder.with_no_client_auth();

        let builder = HttpsConnectorBuilder::new().with_tls_config(config);
        let mut service = match allow {
            Allow::Https => builder.https_only(),
            Allow::Any => builder.https_or_http(),
        }
        .enable_http1()
        .build();

        poll_fn(|cx| service.poll_ready(cx)).await?;
        service
            .call(Uri::from_static(match scheme {
                Scheme::Https => "https://google.com",
                Scheme::Http => "http://google.com",
            }))
            .await
    }

    #[cfg(feature = "smol")]
    #[derive(Debug)]
    struct SmolTcpStream(smol_hyper::rt::FuturesIo<TcpStream>);

    #[cfg(feature = "smol")]
    impl Connection for SmolTcpStream {
        fn connected(&self) -> hyper_util::client::legacy::connect::Connected {
            hyper_util::client::legacy::connect::Connected::new()
        }
    }

    #[cfg(feature = "smol")]
    impl hyper::rt::Read for SmolTcpStream {
        fn poll_read(
            mut self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            buf: hyper::rt::ReadBufCursor<'_>,
        ) -> Poll<std::io::Result<()>> {
            Pin::new(&mut self.0).poll_read(cx, buf)
        }
    }

    #[cfg(feature = "smol")]
    impl hyper::rt::Write for SmolTcpStream {
        fn poll_write(
            mut self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            buf: &[u8],
        ) -> Poll<Result<usize, std::io::Error>> {
            Pin::new(&mut self.0).poll_write(cx, buf)
        }

        fn poll_flush(
            mut self: Pin<&mut Self>,
            cx: &mut Context<'_>,
        ) -> Poll<Result<(), std::io::Error>> {
            Pin::new(&mut self.0).poll_flush(cx)
        }

        fn poll_shutdown(
            mut self: Pin<&mut Self>,
            cx: &mut Context<'_>,
        ) -> Poll<Result<(), std::io::Error>> {
            Pin::new(&mut self.0).poll_shutdown(cx)
        }
    }

    #[cfg(feature = "smol")]
    impl smol::io::AsyncRead for SmolTcpStream {
        fn poll_read(
            mut self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            buf: &mut [u8],
        ) -> Poll<std::io::Result<usize>> {
            Pin::new(self.0.get_mut()).poll_read(cx, buf)
        }
    }

    #[cfg(feature = "smol")]
    impl smol::io::AsyncWrite for SmolTcpStream {
        fn poll_write(
            mut self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            buf: &[u8],
        ) -> Poll<std::io::Result<usize>> {
            Pin::new(self.0.get_mut()).poll_write(cx, buf)
        }

        fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
            Pin::new(self.0.get_mut()).poll_flush(cx)
        }

        fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
            Pin::new(self.0.get_mut()).poll_close(cx)
        }
    }

    #[cfg(feature = "smol")]
    async fn connect_smol(
        allow: Allow,
        scheme: Scheme,
    ) -> Result<MaybeHttpsStream<SmolTcpStream>, BoxError> {
        use smol::net::TcpStream;
        use smol_hyper::rt::FuturesIo;
        use std::future::Future;
        use std::pin::Pin;
        use std::task::{Context, Poll};
        use tower_service::Service;

        #[derive(Clone)]
        struct SmolHttpConnector;

        impl Service<Uri> for SmolHttpConnector {
            type Response = SmolTcpStream;
            type Error = std::io::Error;
            type Future =
                Pin<Box<dyn Future<Output = Result<Self::Response, std::io::Error>> + Send>>;

            fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
                Poll::Ready(Ok(()))
            }

            fn call(&mut self, uri: Uri) -> Self::Future {
                let host = uri
                    .host()
                    .unwrap_or("localhost")
                    .to_string();
                let port = uri
                    .port_u16()
                    .unwrap_or(if uri.scheme_str() == Some("https") {
                        443
                    } else {
                        80
                    });

                Box::pin(async move {
                    let stream = TcpStream::connect((host, port)).await?;
                    Ok(SmolTcpStream(FuturesIo::new(stream)))
                })
            }
        }

        let config_builder = rustls::ClientConfig::builder();
        cfg_if::cfg_if! {
            if #[cfg(feature = "rustls-platform-verifier")] {
                let config_builder = config_builder.try_with_platform_verifier()?;
            } else if #[cfg(feature = "rustls-native-certs")] {
                let config_builder = config_builder.with_native_roots().unwrap();
            } else if #[cfg(feature = "webpki-roots")] {
                let config_builder = config_builder.with_webpki_roots();
            }
        }
        let config = config_builder.with_no_client_auth();

        let builder = HttpsConnectorBuilder::new().with_tls_config(config);
        let mut service = match allow {
            Allow::Https => builder.https_only(),
            Allow::Any => builder.https_or_http(),
        }
        .enable_http1()
        .wrap_connector(SmolHttpConnector);

        poll_fn(|cx| service.poll_ready(cx)).await?;
        service
            .call(Uri::from_static(match scheme {
                Scheme::Https => "https://google.com",
                Scheme::Http => "http://google.com",
            }))
            .await
    }

    enum Allow {
        Https,
        Any,
    }

    enum Scheme {
        Https,
        Http,
    }
}
