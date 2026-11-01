//! Core Invoxa domain: roles, invoice data, deterministic validation, duplicate
//! detection and automation rules. This crate has no I/O dependencies so the
//! financial and authorization rules can be tested in isolation.

pub mod automation;
pub mod duplicate;
pub mod invoice;
pub mod money;
pub mod normalization;
pub mod role;
pub mod status;
pub mod validation;
