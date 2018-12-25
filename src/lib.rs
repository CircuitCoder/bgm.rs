#![feature(custom_attribute)]
#![feature(never_type)]
#![feature(impl_trait_in_bindings)]
#![feature(slice_concat_ext)]

#[macro_use]
mod macros;
#[macro_use]
pub mod consts;
pub mod auth;
pub mod client;
pub mod settings;
