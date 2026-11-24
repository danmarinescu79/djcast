//! Read the text shown in rekordbox's window through Windows UI Automation.
//!
//! rekordbox answers these reads on its own UI thread, so they are slow when it is busy:
//! a read of the whole window (~770 elements) took 0.8–6 s with two decks playing. We
//! therefore read only text elements, and only inside the panel that holds the decks
//! (~95 elements, ~0.1 s), finding that panel again now and then in case the layout changed.
//! (`CUIAutomation8` with a short transaction timeout was tried: its reads were slower still
//! and never finished while rekordbox was busy, so the classic object is used.)
//!
//! COM objects are not `Send`, so a [`Scanner`] must be created and used on one thread.

use super::decks::{TextItem, is_deck_label};
use anyhow::{Context, Result, bail};
use std::time::{Duration, Instant};
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx};
use windows::Win32::System::Variant::VARIANT;
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomationCacheRequest, IUIAutomationCondition, IUIAutomationElement,
    IUIAutomationElementArray, IUIAutomationTreeWalker, TreeScope_Children, TreeScope_Descendants,
    UIA_BoundingRectanglePropertyId, UIA_ClassNamePropertyId, UIA_ControlTypePropertyId, UIA_NamePropertyId,
    UIA_TextControlTypeId,
};

/// Look for the deck panel across the whole window again after this long.
const AREA_REFRESH: Duration = Duration::from_secs(30);

pub struct Scanner {
    uia: IUIAutomation,
    cache: IUIAutomationCacheRequest,
    all: IUIAutomationCondition,
    text: IUIAutomationCondition,
    walker: IUIAutomationTreeWalker,
    window: Option<IUIAutomationElement>,
    /// The element holding the deck panels, and when it was found.
    area: Option<(IUIAutomationElement, Instant)>,
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
            let text = uia.CreatePropertyCondition(UIA_ControlTypePropertyId, &VARIANT::from(UIA_TextControlTypeId.0))?;
            let walker = uia.RawViewWalker()?;
            Ok(Self { uia, cache, all, text, walker, window: None, area: None })
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

    /// The text elements of the deck panels (or of the whole window while the panel is unknown).
    pub fn scan(&mut self) -> Result<Vec<TextItem>> {
        if self.window.is_none() {
            self.window = Some(self.find_window()?);
            self.area = None;
        }
        let window = self.window.clone().unwrap();
        let area = self.area.as_ref().filter(|(_, found)| found.elapsed() < AREA_REFRESH).map(|(a, _)| a.clone());

        if let Some(area) = area {
            match self.read(&area) {
                // Still showing decks: done. Otherwise the layout changed; look again below.
                Ok((items, _)) if items.iter().any(is_deck_label) => return Ok(items),
                Ok(_) => {}
                Err(e) => log::debug!("reading the deck panel: {e}"),
            }
        }

        let (items, labels) = match self.read(&window) {
            Ok(r) => r,
            Err(e) => {
                self.window = None; // window closed or recreated
                self.area = None;
                return Err(e).context("reading rekordbox window");
            }
        };
        if items.is_empty() {
            self.window = None;
            self.area = None;
            bail!("rekordbox window exposes no text (minimized?)");
        }
        // The panel = the common parent of the deck labels; without one, keep reading the window.
        self.area = Some((self.common_parent(&labels).unwrap_or(window), Instant::now()));
        Ok(items)
    }

    /// Non-empty text elements under `root`, plus the elements that are deck labels.
    fn read(&self, root: &IUIAutomationElement) -> windows::core::Result<(Vec<TextItem>, Vec<IUIAutomationElement>)> {
        let elements: IUIAutomationElementArray =
            unsafe { root.FindAllBuildCache(TreeScope_Descendants, &self.text, &self.cache)? };
        let mut items = Vec::new();
        let mut labels = Vec::new();
        unsafe {
            for i in 0..elements.Length()? {
                let e = elements.GetElement(i)?;
                let name = e.CachedName().map(|s| s.to_string()).unwrap_or_default();
                if name.is_empty() {
                    continue;
                }
                let r = e.CachedBoundingRectangle()?;
                let item = TextItem { name, x: r.left, y: r.top, w: r.right - r.left, h: r.bottom - r.top };
                if is_deck_label(&item) {
                    labels.push(e);
                }
                items.push(item);
            }
        }
        Ok((items, labels))
    }

    fn common_parent(&self, labels: &[IUIAutomationElement]) -> Option<IUIAutomationElement> {
        let mut parents = labels.iter().map(|l| unsafe { self.walker.GetParentElement(l) });
        let first = parents.next()?.ok()?;
        for p in parents {
            let same = unsafe { self.uia.CompareElements(&first, &p.ok()?) }.ok()?;
            if !same.as_bool() {
                return None;
            }
        }
        Some(first)
    }
}
