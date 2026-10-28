//! The window widths the layout changes at: `index.css`'s breakpoints, which replace Tailwind's
//! own, so TS's `sm`, `md` and `lg` (`max-sm:`, `max-md:`, `max-lg:`) are these on every page.
//! Each is a window width, in pixels.

/// `sm`: below it, the home page stacks its search and leaves out when a repository was last
/// committed to, and the repository toolbar tightens.
pub const SMALL: f32 = 700.;
/// `md`: below it, the home page's activity goes under the repositories and its banner leaves out
/// the sync status, and the repository page's open sidebar is a rail of icons.
pub const MEDIUM: f32 = 900.;
/// `lg`: below it, the home page's activity column narrows, and the repository page's details
/// slide over the history instead of taking a column.
pub const LARGE: f32 = 1100.;
