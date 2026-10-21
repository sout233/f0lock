use crate::MouseCursor;
use std::ffi::c_void;
use std::ptr::{null_mut, NonNull};
use windows_core::{Error, Result};
use windows_sys::Win32::UI::WindowsAndMessaging::{LoadCursorW, SetCursor};
use windows_sys::{
    core::PCWSTR,
    Win32::UI::WindowsAndMessaging::{
        IDC_APPSTARTING, IDC_ARROW, IDC_CROSS, IDC_HAND, IDC_HELP, IDC_IBEAM, IDC_NO, IDC_SIZEALL,
        IDC_SIZENESW, IDC_SIZENS, IDC_SIZENWSE, IDC_SIZEWE, IDC_WAIT,
    },
};

#[derive(Copy, Clone)]
pub struct SystemCursor(Option<NonNull<c_void>>);

impl SystemCursor {
    pub fn load(cursor: MouseCursor) -> Result<Self> {
        if matches!(cursor, MouseCursor::Hidden) {
            return Ok(Self(None));
        }
        let cursor_ptr = cursor_to_lpcwstr(cursor);

        // SAFETY: the PCWSTR returned by cursor_to_lpcwstr is always a valid shared cursor ID
        let result = unsafe { LoadCursorW(null_mut(), cursor_ptr) };

        match NonNull::new(result) {
            Some(res) => Ok(Self(Some(res))),
            None => Err(Error::from_thread()),
        }
    }

    pub fn set(&self) {
        // SAFETY: The handle is a loaded shared cursor, or NULL to hide the cursor.
        unsafe { SetCursor(self.0.map_or(null_mut(), NonNull::as_ptr)) };
    }
}

fn cursor_to_lpcwstr(cursor: MouseCursor) -> PCWSTR {
    match cursor {
        MouseCursor::Default => IDC_ARROW,
        MouseCursor::Hand => IDC_HAND,
        MouseCursor::HandGrabbing => IDC_SIZEALL,
        MouseCursor::Help => IDC_HELP,
        MouseCursor::Hidden => unreachable!("Hidden cursors do not load a resource"),

        MouseCursor::Text => IDC_IBEAM,
        MouseCursor::VerticalText => IDC_IBEAM,

        MouseCursor::Working => IDC_WAIT,
        MouseCursor::PtrWorking => IDC_APPSTARTING,

        MouseCursor::NotAllowed => IDC_NO,
        MouseCursor::PtrNotAllowed => IDC_NO,

        MouseCursor::ZoomIn => IDC_ARROW,
        MouseCursor::ZoomOut => IDC_ARROW,

        MouseCursor::Alias => IDC_ARROW,
        MouseCursor::Copy => IDC_ARROW,
        MouseCursor::Move => IDC_SIZEALL,
        MouseCursor::AllScroll => IDC_SIZEALL,
        MouseCursor::Cell => IDC_CROSS,
        MouseCursor::Crosshair => IDC_CROSS,

        MouseCursor::EResize => IDC_SIZEWE,
        MouseCursor::NResize => IDC_SIZENS,
        MouseCursor::NeResize => IDC_SIZENESW,
        MouseCursor::NwResize => IDC_SIZENWSE,
        MouseCursor::SResize => IDC_SIZENS,
        MouseCursor::SeResize => IDC_SIZENWSE,
        MouseCursor::SwResize => IDC_SIZENESW,
        MouseCursor::WResize => IDC_SIZEWE,
        MouseCursor::EwResize => IDC_SIZEWE,
        MouseCursor::NsResize => IDC_SIZENS,
        MouseCursor::NwseResize => IDC_SIZENWSE,
        MouseCursor::NeswResize => IDC_SIZENESW,

        MouseCursor::ColResize => IDC_SIZEWE,
        MouseCursor::RowResize => IDC_SIZENS,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::UI::WindowsAndMessaging::GetCursor;

    #[test]
    fn hidden_cursor_sets_null_and_a_visible_cursor_can_be_restored() {
        struct RestoreCursor(*mut c_void);

        impl Drop for RestoreCursor {
            fn drop(&mut self) {
                unsafe { SetCursor(self.0) };
            }
        }

        let _restore = RestoreCursor(unsafe { GetCursor() });
        let visible = SystemCursor::load(MouseCursor::Default).unwrap();
        visible.set();
        let handle = unsafe { GetCursor() };
        assert!(!handle.is_null());

        SystemCursor::load(MouseCursor::Hidden).unwrap().set();
        assert!(unsafe { GetCursor() }.is_null());

        visible.set();
        assert_eq!(unsafe { GetCursor() }, handle);
    }
}
