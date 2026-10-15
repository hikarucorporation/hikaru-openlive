mod client;
mod clipboard;
mod cursor;
mod display;
mod popup;
mod serial;
mod window;

/// Contains Types for configuring layer_shell surfaces.
pub mod layer_shell;

pub(crate) use client::*;

use wayland_protocols::wp::cursor_shape::v1::client::wp_cursor_shape_device_v1::Shape;

use gpui::CursorStyle;

pub(super) fn to_shape(style: CursorStyle) -> Shape {
    match style {
        CursorStyle::Arrow => Shape::Default,
        CursorStyle::IBeam => Shape::Text,
        CursorStyle::Crosshair => Shape::Crosshair,
        CursorStyle::ClosedHand => Shape::Grabbing,
        CursorStyle::OpenHand => Shape::Grab,
        CursorStyle::PointingHand => Shape::Pointer,
        CursorStyle::ResizeLeft => Shape::WResize,
        CursorStyle::ResizeRight => Shape::EResize,
        CursorStyle::ResizeLeftRight => Shape::EwResize,
        CursorStyle::ResizeUp => Shape::NResize,
        CursorStyle::ResizeDown => Shape::SResize,
        CursorStyle::ResizeUpDown => Shape::NsResize,
        CursorStyle::ResizeUpLeftDownRight => Shape::NwseResize,
        CursorStyle::ResizeUpRightDownLeft => Shape::NeswResize,
        CursorStyle::ResizeColumn => Shape::ColResize,
        CursorStyle::ResizeRow => Shape::RowResize,
        CursorStyle::IBeamCursorForVerticalLayout => Shape::VerticalText,
        CursorStyle::OperationNotAllowed => Shape::NotAllowed,
        CursorStyle::DragLink => Shape::Alias,
        CursorStyle::DragCopy => Shape::Copy,
        CursorStyle::ContextualMenu => Shape::ContextMenu,
        // PATCH LOCAL (hikaru): `wp_cursor_shape_v1` no tiene forma "hidden"
        // (el enum del protocolo es fijo), así que este brazo nunca se usa: el
        // cursor oculto se emite por `wl_pointer.set_cursor(serial, None, ..)`
        // en `client.rs`. El brazo existe sólo para que el match siga siendo
        // exhaustivo.
        CursorStyle::Hidden => Shape::Default,
    }
}
