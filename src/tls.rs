//! TLS connection

use std::io;

use http::Uri;
use pki_types::ServerName;

mod alpn;
pub use self::alpn::alpn;

#[cfg(feature = "tokio")]
mod tokio;
#[cfg(feature = "tokio")]
pub use self::tokio::{tokio, tokio_with_resolver};

#[cfg(feature = "smol")]
mod smol;
#[cfg(feature = "smol")]
pub use self::smol::{smol, smol_with_resolver};

/// Trait for resolving server names from URIs
pub trait ResolveServerName: Send {
    /// Extract a server name from the URI
    fn resolve(&self, uri: &Uri) -> Result<ServerName<'static>, io::Error>;
}

/// Default server name resolver that extracts from the URI
#[derive(Debug, Clone)]
pub struct DefaultServerNameResolver;

impl ResolveServerName for DefaultServerNameResolver {
    fn resolve(&self, uri: &Uri) -> Result<ServerName<'static>, io::Error> {
        let host = uri
            .host()
            .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "missing host in URI"))?;

        // Remove brackets from IPv6 addresses
        let hostname = if host.starts_with('[') && host.ends_with(']') {
            &host[1..host.len() - 1]
        } else {
            host
        };

        let server_name = ServerName::try_from(hostname.to_string());
        server_name.map_err(|err| io::Error::new(io::ErrorKind::Other, err))
    }
}
