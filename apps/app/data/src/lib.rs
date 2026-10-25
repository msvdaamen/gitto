//! What Gitto's app works with, without drawing any of it: the data layer ([`data`]), the
//! services started at launch ([`services`]), and the state that outlives a view ([`state`]). The
//! app (`apps/app`) and its primitives (`gitto-app-kit`) build on it; it knows nothing of either.

pub mod data;
pub mod services;
pub mod state;
