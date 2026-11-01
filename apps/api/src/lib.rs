//! Invoxa HTTP API. All tenant-scoped operations receive an authenticated
//! organization context from the request extractor; client-supplied tenant ids
//! are never used for authorization.

pub mod auth;
pub mod config;
pub mod error;
pub mod openapi;
pub mod presenters;
pub mod rate_limit;
pub mod router;
pub mod routes;
pub mod state;
pub mod util;
