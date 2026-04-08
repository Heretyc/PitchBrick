//! Push-to-Talk key simulation via Win32 keybd_event / mouse_event.
//!
//! Provides key press/release functions and a mapping from human-readable
//! key names to Windows virtual key codes for the PTT-on-Green feature.
//! Extended mouse buttons (Mouse4/Mouse5) use mouse_event with XBUTTON flags.

const VK_XBUTTON1: u16 = 0x05;
const VK_XBUTTON2: u16 = 0x06;

/// Presses a key or mouse button down (does not release).
#[cfg(windows)]
pub fn press_key(vk: u16) {
    if vk == VK_XBUTTON1 || vk == VK_XBUTTON2 {
        use windows::Win32::UI::Input::KeyboardAndMouse::{mouse_event, MOUSEEVENTF_XDOWN};
        let xbutton = if vk == VK_XBUTTON1 { 1 } else { 2 };
        unsafe {
            mouse_event(MOUSEEVENTF_XDOWN, 0, 0, xbutton, 0);
        }
    } else {
        use windows::Win32::UI::Input::KeyboardAndMouse::{keybd_event, KEYBD_EVENT_FLAGS};
        unsafe {
            keybd_event(vk as u8, 0, KEYBD_EVENT_FLAGS(0), 0);
        }
    }
}

/// Releases a key or mouse button that was previously pressed.
#[cfg(windows)]
pub fn release_key(vk: u16) {
    if vk == VK_XBUTTON1 || vk == VK_XBUTTON2 {
        use windows::Win32::UI::Input::KeyboardAndMouse::{mouse_event, MOUSEEVENTF_XUP};
        let xbutton = if vk == VK_XBUTTON1 { 1 } else { 2 };
        unsafe {
            mouse_event(MOUSEEVENTF_XUP, 0, 0, xbutton, 0);
        }
    } else {
        use windows::Win32::UI::Input::KeyboardAndMouse::{keybd_event, KEYEVENTF_KEYUP};
        unsafe {
            keybd_event(vk as u8, 0, KEYEVENTF_KEYUP, 0);
        }
    }
}

#[cfg(not(windows))]
pub fn press_key(_vk: u16) {}
#[cfg(not(windows))]
pub fn release_key(_vk: u16) {}

/// Maps a human-readable key name to a Windows virtual key code.
pub fn key_name_to_vk(name: &str) -> Option<u16> {
    Some(match name {
        "`" => 0xC0,        // VK_OEM_3 (backtick/tilde)
        "Tab" => 0x09,      // VK_TAB
        "CapsLock" => 0x14, // VK_CAPITAL
        "F1" => 0x70,
        "F2" => 0x71,
        "F3" => 0x72,
        "F4" => 0x73,
        "F5" => 0x74,
        "F6" => 0x75,
        "F7" => 0x76,
        "F8" => 0x77,
        "F9" => 0x78,
        "F10" => 0x79,
        "F11" => 0x7A,
        "F12" => 0x7B,
        "F13" => 0x7C,
        "F14" => 0x7D,
        "F15" => 0x7E,
        "F16" => 0x7F,
        "F17" => 0x80,
        "F18" => 0x81,
        "F19" => 0x82,
        "F20" => 0x83,
        "F21" => 0x84,
        "F22" => 0x85,
        "F23" => 0x86,
        "F24" => 0x87,
        "Mouse4" => VK_XBUTTON1,
        "Mouse5" => VK_XBUTTON2,
        "-" => 0xBD,        // VK_OEM_MINUS
        "=" => 0xBB,        // VK_OEM_PLUS
        "[" => 0xDB,        // VK_OEM_4
        "]" => 0xDD,        // VK_OEM_6
        "\\" => 0xDC,       // VK_OEM_5
        ";" => 0xBA,        // VK_OEM_1
        "'" => 0xDE,        // VK_OEM_7
        "," => 0xBC,        // VK_OEM_COMMA
        "." => 0xBE,        // VK_OEM_PERIOD
        "/" => 0xBF,        // VK_OEM_2
        "Insert" => 0x2D,   // VK_INSERT
        "Delete" => 0x2E,   // VK_DELETE
        "Home" => 0x24,     // VK_HOME
        "End" => 0x23,      // VK_END
        "PageUp" => 0x21,   // VK_PRIOR
        "PageDown" => 0x22, // VK_NEXT
        "ScrollLock" => 0x91,
        "Pause" => 0x13,
        _ => return None,
    })
}

/// Available PTT key names for the settings picker (static, zero allocation).
pub const AVAILABLE_KEYS: &[&str] = &[
    "`", "Tab", "CapsLock",
    "Mouse4", "Mouse5",
    "F1", "F2", "F3", "F4", "F5", "F6",
    "F7", "F8", "F9", "F10", "F11", "F12",
    "F13", "F14", "F15", "F16", "F17", "F18",
    "F19", "F20", "F21", "F22", "F23", "F24",
    "-", "=", "[", "]", "\\", ";", "'", ",", ".", "/",
    "Insert", "Delete", "Home", "End",
    "PageUp", "PageDown", "ScrollLock", "Pause",
];
