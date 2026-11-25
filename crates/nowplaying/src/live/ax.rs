//! Read the text shown in rekordbox's window through the macOS Accessibility API.
//!
//! rekordbox 7 (JUCE) exposes the same deck panels as on Windows: a deck-number label, the
//! title, artist, BPM and the remaining/elapsed time, as `AXStaticText` elements with screen
//! positions in points. A full read of the window is ~450 elements; asking for all the
//! attributes of an element in one call keeps it to about one round trip per element.
//!
//! The user must allow djcast under System Settings → Privacy & Security → Accessibility.
//! AX references are not `Send`, so a [`Scanner`] must be created and used on one thread.

use super::PermissionNeeded;
use super::decks::TextItem;
use anyhow::{Result, bail};
use core_foundation::array::{CFArray, CFArrayRef};
use core_foundation::base::{CFGetTypeID, CFType, CFTypeID, CFTypeRef, TCFType};
use core_foundation::boolean::CFBoolean;
use core_foundation::dictionary::CFDictionary;
use core_foundation::string::CFString;
use std::ffi::c_void;

type AXUIElementRef = CFTypeRef;
type AXError = i32;
const AX_SUCCESS: AXError = 0;
const AX_VALUE_CGPOINT: u32 = 1;
const AX_VALUE_CGSIZE: u32 = 2;
/// Give up on an element after this long instead of waiting for a busy rekordbox (default 6 s).
const MESSAGING_TIMEOUT_SECS: f32 = 1.0;
/// Deeper than the deck panels ever go; guards against cycles.
const MAX_DEPTH: usize = 40;

#[repr(C)]
#[derive(Default)]
struct CGPoint {
    x: f64,
    y: f64,
}

#[repr(C)]
#[derive(Default)]
struct CGSize {
    width: f64,
    height: f64,
}

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXIsProcessTrusted() -> bool;
    fn AXIsProcessTrustedWithOptions(options: CFTypeRef) -> bool;
    fn AXUIElementCreateApplication(pid: i32) -> AXUIElementRef;
    fn AXUIElementSetMessagingTimeout(element: AXUIElementRef, seconds: f32) -> AXError;
    fn AXUIElementCopyMultipleAttributeValues(
        element: AXUIElementRef,
        attributes: CFArrayRef,
        options: u32,
        values: *mut CFArrayRef,
    ) -> AXError;
    fn AXValueGetTypeID() -> CFTypeID;
    fn AXValueGetValue(value: CFTypeRef, value_type: u32, out: *mut c_void) -> bool;
}

const PERMISSION: &str = "djcast needs permission to read the rekordbox window: System Settings → \
     Privacy & Security → Accessibility → turn on djcast (then restart djcast)";

pub struct Scanner {
    /// rekordbox's process id and its application element.
    app: Option<(i32, CFType)>,
    /// Attributes read for every element, in this order.
    attrs: CFArray<CFString>,
}

impl Scanner {
    pub fn new() -> Result<Self> {
        // Shows the system prompt (once) that leads the user to the Accessibility setting.
        let prompt = CFString::from_static_string("AXTrustedCheckOptionPrompt");
        let options = CFDictionary::from_CFType_pairs(&[(prompt.as_CFType(), CFBoolean::true_value().as_CFType())]);
        if !unsafe { AXIsProcessTrustedWithOptions(options.as_CFTypeRef()) } {
            log::warn!("Accessibility permission missing; asked the user");
        }
        let attrs = ["AXRole", "AXValue", "AXChildren", "AXPosition", "AXSize"].map(CFString::from_static_string);
        Ok(Self { app: None, attrs: CFArray::from_CFTypes(&attrs) })
    }

    /// The text elements of rekordbox's window.
    pub fn scan(&mut self) -> Result<Vec<TextItem>> {
        if !unsafe { AXIsProcessTrusted() } {
            return Err(PermissionNeeded(PERMISSION).into());
        }
        if self.app.as_ref().is_none_or(|(pid, _)| !alive(*pid)) {
            let Some(pid) = find_rekordbox() else {
                self.app = None;
                bail!("rekordbox window not found (is rekordbox running?)");
            };
            let app = unsafe { CFType::wrap_under_create_rule(AXUIElementCreateApplication(pid)) };
            unsafe { AXUIElementSetMessagingTimeout(app.as_CFTypeRef(), MESSAGING_TIMEOUT_SECS) };
            self.app = Some((pid, app));
        }
        let app = self.app.as_ref().unwrap().1.as_CFTypeRef();
        let mut items = Vec::new();
        self.walk(app, 0, &mut items);
        if items.is_empty() {
            self.app = None; // still starting, or a different process; look again next time
            bail!("rekordbox window exposes no text (still starting?)");
        }
        Ok(items)
    }

    fn walk(&self, element: AXUIElementRef, depth: usize, out: &mut Vec<TextItem>) {
        let mut raw: CFArrayRef = std::ptr::null();
        let err = unsafe {
            AXUIElementCopyMultipleAttributeValues(element, self.attrs.as_concrete_TypeRef(), 0, &mut raw)
        };
        if err != AX_SUCCESS || raw.is_null() {
            return;
        }
        // Missing attributes come back as AXValue errors, which the type checks below skip.
        let values: CFArray<CFType> = unsafe { CFArray::wrap_under_create_rule(raw) };
        let get = |i: isize| values.get(i).map(|v| v.clone());
        let role = get(0).and_then(|v| v.downcast::<CFString>()).map(|s| s.to_string()).unwrap_or_default();
        match role.as_str() {
            "AXMenuBar" => return,
            "AXStaticText" => {
                let text = get(1).and_then(|v| v.downcast::<CFString>()).map(|s| s.to_string()).unwrap_or_default();
                if !text.is_empty() {
                    let mut p = CGPoint::default();
                    let mut s = CGSize::default();
                    if get(3).is_some_and(|v| ax_value(&v, AX_VALUE_CGPOINT, &mut p))
                        && get(4).is_some_and(|v| ax_value(&v, AX_VALUE_CGSIZE, &mut s))
                    {
                        out.push(TextItem {
                            name: text,
                            x: p.x.round() as i32,
                            y: p.y.round() as i32,
                            w: s.width.round() as i32,
                            h: s.height.round() as i32,
                        });
                    }
                }
            }
            _ => {}
        }
        if depth >= MAX_DEPTH {
            return;
        }
        if let Some(children) = get(2).and_then(|v| v.downcast::<CFArray>()) {
            for child in children.iter() {
                self.walk(*child as AXUIElementRef, depth + 1, out);
            }
        }
    }
}

/// Read a point or size out of an `AXValue`.
fn ax_value<T>(value: &CFType, value_type: u32, out: &mut T) -> bool {
    let r = value.as_CFTypeRef();
    unsafe { CFGetTypeID(r) == AXValueGetTypeID() && AXValueGetValue(r, value_type, out as *mut T as *mut c_void) }
}

fn alive(pid: i32) -> bool {
    unsafe { libc::kill(pid, 0) == 0 }
}

/// The process id of rekordbox (the app itself, not its agent or helpers).
fn find_rekordbox() -> Option<i32> {
    let mut pids = vec![0i32; 4096];
    let bytes = (pids.len() * size_of::<i32>()) as i32;
    let n = unsafe { libc::proc_listallpids(pids.as_mut_ptr() as *mut c_void, bytes) };
    if n <= 0 {
        return None;
    }
    let mut path = vec![0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
    pids.iter().take(n as usize).copied().find(|&pid| {
        let len = unsafe { libc::proc_pidpath(pid, path.as_mut_ptr() as *mut c_void, path.len() as u32) };
        len > 0 && is_rekordbox_path(&String::from_utf8_lossy(&path[..len as usize]))
    })
}

fn is_rekordbox_path(path: &str) -> bool {
    path.ends_with(".app/Contents/MacOS/rekordbox") && path.contains("/rekordbox.app/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_rekordbox_app_matches() {
        assert!(is_rekordbox_path("/Applications/rekordbox 7/rekordbox.app/Contents/MacOS/rekordbox"));
        assert!(!is_rekordbox_path(
            "/Applications/rekordbox 7/rekordbox.app/Contents/MacOS/rekordboxAgent.app/Contents/MacOS/rekordboxAgent"
        ));
        assert!(!is_rekordbox_path("/usr/local/bin/rekordbox"));
    }
}
