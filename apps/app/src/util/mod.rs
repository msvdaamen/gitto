//! Pure helpers (`lib/*.ts`): formatting for display.
//!
//! The rest of `lib/` has no counterpart: `rpc.ts` (the UI calls the crates), `query-client.ts`
//! (`crate::data`'s entities), `scroll.ts` (each view owns its scroll handles, and resets them when it shows
//! another repository), `trace.ts` (GPUI's own profiling), `typing.ts` (key bindings are scoped by
//! key context, so typing in an input never reaches a list's shortcuts), and `errors.ts`
//! (`DataError::message` and `DataError::has_code`).

pub mod format;
