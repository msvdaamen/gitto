//! The primitives screens are built from (`components/ui/*`), on GPUI Kit: one module per TSX
//! file, named after it. Where Kit has the component, the primitive is Kit's with Gitto's look
//! from the theme (buttons, switches, inputs, dialogs, menus, popovers); the rest are Gitto's own,
//! drawn from `cx.palette()`.

pub mod avatar;
pub mod badge;
pub mod button;
pub mod card;
pub mod confirm_dialog;
pub mod context_menu;
pub mod dialog;
pub mod divider;
pub mod empty_state;
pub mod failure_popover;
pub mod form_field;
pub mod icons;
pub mod kbd;
pub mod line_stats;
pub mod mascot;
pub mod notice;
pub mod resize_handle;
pub mod section_header;
pub mod segmented_control;
pub mod status_dot;
pub mod switch;
pub mod text_input;
pub mod tone;
pub mod updating_indicator;
pub mod virtual_list;

pub use self::avatar::{Avatar, AvatarSize};
pub use self::badge::Badge;
pub use self::button::{IconButton, LinkButton, action_button};
pub use self::card::Card;
pub use self::confirm_dialog::ConfirmDialog;
pub use self::context_menu::{ContextMenuExt, PopupMenu, context_menu_item, menu_frame};
pub use self::dialog::DialogHeading;
pub use self::divider::Divider;
pub use self::empty_state::{EmptyState, LoadingState};
pub use self::failure_popover::FailurePopover;
pub use self::form_field::FormField;
pub use self::icons::{GittoIcon, Icon, IconName};
pub use self::kbd::Kbd;
pub use self::line_stats::LineStats;
pub use self::mascot::Mascot;
pub use self::notice::Notice;
pub use self::resize_handle::{HandleSide, PanelDrag, ResizeHandle, dragged_width};
pub use self::section_header::SectionHeader;
pub use self::segmented_control::SegmentedControl;
pub use self::status_dot::StatusDot;
pub use self::switch::SettingSwitch;
pub use self::text_input::TextInput;
pub use self::tone::Tone;
pub use self::updating_indicator::UpdatingIndicator;
pub use self::virtual_list::{VirtualList, is_near_end};
