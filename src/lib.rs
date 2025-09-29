//! # hyper-rustls
//!
//! A pure-Rust HTTPS connector for [hyper](https://hyper.rs), based on
//! [Rustls](https://github.com/rustls/rustls).

#![warn(missing_docs, unreachable_pub)]
#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]

mod config;
pub use config::ConfigBuilderExt;

pub mod tcp;
pub mod tls;

#[allow(unused_macros, unused_imports)]
#[cfg(feature = "logging")]
mod log {
    #[cfg(any(feature = "rustls-native-certs", feature = "webpki-roots"))]
    pub(crate) use log::debug;
    #[cfg(feature = "rustls-native-certs")]
    pub(crate) use log::warn;
}

#[allow(unused_macros, unused_imports)]
#[cfg(not(feature = "logging"))]
mod log {
    #[cfg(any(feature = "rustls-native-certs", feature = "webpki-roots"))]
    macro_rules! debug    ( ($($tt:tt)*) => {{}} );
    #[cfg(any(feature = "rustls-native-certs", feature = "webpki-roots"))]
    pub(crate) use debug;
    #[cfg(feature = "rustls-native-certs")]
    macro_rules! warn_    ( ($($tt:tt)*) => {{}} );
    #[cfg(feature = "rustls-native-certs")]
    pub(crate) use warn_ as warn;
}
