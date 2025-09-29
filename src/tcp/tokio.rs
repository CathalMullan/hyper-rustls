//! Tokio TCP connector implementation

use std::{future::Future, io, pin::Pin};

use hyper_util::rt::TokioIo;
use tokio::net::TcpStream;

use crate::tcp::{TcpConnect, TcpConnector};

/// Create a Tokio TCP connector
pub fn tokio() -> TcpConnector<TokioTcp> {
    TcpConnector::new(TokioTcp)
}

/// Tokio TCP connector
#[derive(Clone)]
pub struct TokioTcp;

impl TcpConnect for TokioTcp {
    type Stream = TokioIo<TcpStream>;

    fn connect(
        &self,
        host: String,
        port: u16,
    ) -> Pin<Box<dyn Future<Output = io::Result<Self::Stream>> + Send>> {
        Box::pin(async move {
            let stream = TcpStream::connect((host, port)).await?;
            Ok(TokioIo::new(stream))
        })
    }
}
