//! Simple HTTPS GET client based on hyper-rustls
//!
//! First parameter is the mandatory URL to GET.
//! Second parameter is an optional path to CA store.

use std::{env, fs, io, str::FromStr};

use http::{Request, Uri};
use http_body_util::{BodyExt, Empty};
use hyper::{body::Bytes, client::conn};
use hyper_rustls::{tcp, tls, ConfigBuilderExt};
use rustls::RootCertStore;
use tower::ServiceBuilder;
use tower_service::Service;

fn main() {
    // Send GET request and inspect result, with proper error handling.
    if let Err(e) = run_client() {
        eprintln!("FAILED: {e}");
        std::process::exit(1);
    }
}

fn error(err: String) -> io::Error {
    io::Error::new(io::ErrorKind::Other, err)
}

#[tokio::main]
async fn run_client() -> io::Result<()> {
    // Set a process wide default crypto provider.
    #[cfg(feature = "ring")]
    let _ = rustls::crypto::ring::default_provider().install_default();
    #[cfg(feature = "aws-lc-rs")]
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

    // First parameter is target URL (mandatory).
    let url = match env::args().nth(1) {
        Some(ref url) => Uri::from_str(url).map_err(|e| error(format!("{e}")))?,
        None => {
            println!("Usage: client <url> <ca_store>");
            return Ok(());
        }
    };

    // Second parameter is custom Root-CA store (optional, defaults to native cert store).
    let mut ca = match env::args().nth(2) {
        Some(ref path) => {
            let f =
                fs::File::open(path).map_err(|e| error(format!("failed to open {path}: {e}")))?;
            let rd = io::BufReader::new(f);
            Some(rd)
        }
        None => None,
    };

    // Prepare the TLS client config
    let tls = match ca {
        Some(ref mut rd) => {
            // Read trust roots
            let certs = rustls_pemfile::certs(rd).collect::<Result<Vec<_>, _>>()?;
            let mut roots = RootCertStore::empty();
            roots.add_parsable_certificates(certs);
            // TLS client config using the custom CA store for lookups
            rustls::ClientConfig::builder()
                .with_root_certificates(roots)
                .with_no_client_auth()
        }
        // Default TLS client config with native roots
        None => rustls::ClientConfig::builder()
            .with_native_roots()?
            .with_no_client_auth(),
    };

    let mut service = ServiceBuilder::new()
        .layer(tls::tokio(tls))
        .layer(tls::alpn(["http/1.1"]))
        .service(tcp::tokio());

    let stream = service
        .call(url.clone())
        .await
        .map_err(|e| error(format!("Could not connect: {e:?}")))?;

    let (mut tx, conn) = conn::http1::handshake(stream)
        .await
        .map_err(|e| error(format!("Could not handshake: {e:?}")))?;

    tokio::spawn(async move {
        if let Err(e) = conn.await {
            eprintln!("Connection error: {:?}", e);
        }
    });

    let mut request = Request::builder().uri(&url);
    if let Some(authority) = url.authority() {
        request = request.header("Host", authority.host());
    }

    let request = request
        .body(Empty::<Bytes>::new())
        .map_err(|e| error(format!("Could not build request: {e:?}")))?;

    let response = tx
        .send_request(request)
        .await
        .map_err(|e| error(format!("Could not get: {e:?}")))?;

    println!("Status:\n{}", response.status());
    println!("Headers:\n{:#?}", response.headers());

    let body = response
        .into_body()
        .collect()
        .await
        .map_err(|e| error(format!("Could not get body: {e:?}")))?
        .to_bytes();
    println!("Body:\n{}", String::from_utf8_lossy(&body));

    Ok(())
}
