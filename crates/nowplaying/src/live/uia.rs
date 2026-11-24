//! Read the text shown in rekordbox's window through Windows UI Automation.
//!
//! COM objects are not `Send`, so a [`Scanner`] must be created and used on one thread.

use super::decks::TextItem;
use anyhow::{Context, Result, bail};
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx};
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomationCacheRequest, IUIAutomationCondition, IUIAutomationElement,
    TreeScope_Children, TreeScope_Descendants, UIA_BoundingRectanglePropertyId, UIA_ClassNamePropertyId,
    UIA_ControlTypePropertyId, UIA_NamePropertyId, UIA_TextControlTypeId,
};

pub struct Scanner {
    uia: IUIAutomation,
    cache: IUIAutomationCacheRequest,
    all: IUIAutomationCondition,
    window: Option<IUIAutomationElement>,
}

impl Scanner {
    pub fn new() -> Result<Self> {
        unsafe {
            // S_FALSE (already initialized) is fine; ignore the result.
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            let uia: IUIAutomation =
                CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER).context("creating UI Automation")?;
            let cache = uia.CreateCacheRequest()?;
            for prop in [
                UIA_NamePropertyId,
                UIA_BoundingRectanglePropertyId,
                UIA_ControlTypePropertyId,
                UIA_ClassNamePropertyId,
            ] {
                cache.AddProperty(prop)?;
            }
            let all = uia.CreateTrueCondition()?;
            Ok(Self { uia, cache, all, window: None })
        }
    }

    fn find_window(&self) -> Result<IUIAutomationElement> {
        unsafe {
            let root = self.uia.GetRootElement()?;
            let children = root.FindAllBuildCache(TreeScope_Children, &self.all, &self.cache)?;
            for i in 0..children.Length()? {
                let e = children.GetElement(i)?;
                let name = e.CachedName().map(|s| s.to_string()).unwrap_or_default();
                let class = e.CachedClassName().map(|s| s.to_string()).unwrap_or_default();
                if name == "rekordbox" && class.starts_with("JUCE") {
                    return Ok(e);
                }
            }
        }
        bail!("rekordbox window not found (is rekordbox running?)")
    }

    /// All text elements in the rekordbox window, in one cross-process round trip.
    pub fn scan(&mut self) -> Result<Vec<TextItem>> {
        if self.window.is_none() {
            self.window = Some(self.find_window()?);
        }
        let window = self.window.as_ref().unwrap();
        let elements = match unsafe { window.FindAllBuildCache(TreeScope_Descendants, &self.all, &self.cache) } {
            Ok(e) => e,
            Err(e) => {
                self.window = None; // window closed or recreated
                return Err(e).context("reading rekordbox window");
            }
        };
        let mut out = Vec::new();
        unsafe {
            for i in 0..elements.Length()? {
                let e = elements.GetElement(i)?;
                if e.CachedControlType()? != UIA_TextControlTypeId {
                    continue;
                }
                let name = e.CachedName().map(|s| s.to_string()).unwrap_or_default();
                if name.is_empty() {
                    continue;
                }
                let r = e.CachedBoundingRectangle()?;
                out.push(TextItem { name, x: r.left, y: r.top, w: r.right - r.left, h: r.bottom - r.top });
            }
        }
        if out.is_empty() {
            self.window = None;
            bail!("rekordbox window exposes no text (minimized?)");
        }
        Ok(out)
    }
}
