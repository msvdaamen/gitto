//! What Gitto's app works with, without drawing any of it: the data layer ([`data`]), the
//! services started at launch ([`services`]), and the state that outlives a view ([`state`]). The
//! app (`apps/app`) builds on it, and on its primitives (`gitto-app-kit`), which build alongside
//! it: neither knows of the other.

pub mod data;
pub mod services;
pub mod state;
