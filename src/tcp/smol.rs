//! Smol TCP connector implementation

use std::{future::Future, io, pin::Pin};

use smol::net::TcpStream;
use smol_hyper::rt::FuturesIo;

use crate::tcp::{TcpConnect, TcpConnector};

/// Create a Smol TCP connector
pub fn smol() -> TcpConnector<SmolTcp> {
    TcpConnector::new(SmolTcp)
}

/// Smol TCP connector
#[derive(Clone)]
pub struct SmolTcp;

impl TcpConnect for SmolTcp {
    type Stream = FuturesIo<TcpStream>;

    fn connect(
        &self,
        host: String,
        port: u16,
    ) -> Pin<Box<dyn Future<Output = io::Result<Self::Stream>> + Send>> {
        Box::pin(async move {
            let stream = TcpStream::connect((host, port)).await?;
            Ok(FuturesIo::new(stream))
        })
    }
}
