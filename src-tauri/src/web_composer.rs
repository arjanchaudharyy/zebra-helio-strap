//! The "eyes" for fully automatic handoff to AI (rebuilt after user testing, Windows only).
//!
//! The first version pressed keys blind: once the title matched, it waited 1.5 seconds, pressed Ctrl+V, then Enter. In tests only DeepSeek worked fully.
//! Sites like ChatGPT send the title from the server before the page is live, so the first file paste missed. Some sites do not focus the input box,
//! so paste and Enter both missed. While a file was still uploading, the site blocked Enter and only the intro text was sent.
//!
//! Now it reads the UI tree the browser gives to screen readers (UI Automation, from Chrome, Edge and Firefox) and checks each step:
//! - paste only when the input box (Edit) in the web page is found and has focus;
//! - after the file paste, the file counts as attached only when an element with the file name (the attachment card) shows on the page;
//! - after Enter, the message counts as sent only when the intro text is gone from the input box.
//!
//! It only reads the page UI tree and calls SetFocus only on the input box. It does not click or change anything in the page.

use std::mem::ManuallyDrop;
use windows::core::BSTR;
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
};
use windows::Win32::System::Variant::{VariantClear, VARIANT, VT_BSTR, VT_I4};
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomationCondition, IUIAutomationElement,
    IUIAutomationTextPattern, IUIAutomationValuePattern, PropertyConditionFlags_MatchSubstring,
    TreeScope_Descendants, UIA_ControlTypePropertyId, UIA_DocumentControlTypeId,
    UIA_EditControlTypeId, UIA_NamePropertyId, UIA_TextPatternId, UIA_ValuePatternId,
    UIA_CONTROLTYPE_ID,
};

/// A VARIANT for a property condition. Call `VariantClear` when done (the BSTR must be freed).
struct Variant(VARIANT);

impl Variant {
    fn i4(value: i32) -> Self {
        let mut variant = VARIANT::default();
        // SAFETY: write the type first, then the matching member of the same union.
        unsafe {
            let inner = &mut *variant.Anonymous.Anonymous;
            inner.vt = VT_I4;
            inner.Anonymous.lVal = value;
        }
        Self(variant)
    }

    fn bstr(text: &str) -> Self {
        let mut variant = VARIANT::default();
        // SAFETY: same as above. The VARIANT owns the BSTR, and VariantClear in Drop frees it.
        unsafe {
            let inner = &mut *variant.Anonymous.Anonymous;
            inner.vt = VT_BSTR;
            inner.Anonymous.bstrVal = ManuallyDrop::new(BSTR::from(text));
        }
        Self(variant)
    }
}

impl Drop for Variant {
    fn drop(&mut self) {
        // SAFETY: this VARIANT was built correctly above.
        let _ = unsafe { VariantClear(&mut self.0) };
    }
}

pub struct Composer {
    uia: IUIAutomation,
    window: IUIAutomationElement,
}

impl Drop for Composer {
    fn drop(&mut self) {
        // SAFETY: pairs with CoInitializeEx in `attach` (same thread).
        unsafe { CoUninitialize() };
    }
}

fn center_in(rect: &RECT, outer: &RECT) -> bool {
    let x = (rect.left + rect.right) / 2;
    let y = (rect.top + rect.bottom) / 2;
    x >= outer.left && x <= outer.right && y >= outer.top && y <= outer.bottom
}

fn area(rect: &RECT) -> i64 {
    i64::from((rect.right - rect.left).max(0)) * i64::from((rect.bottom - rect.top).max(0))
}

impl Composer {
    /// Targets the browser window in the foreground. Use it all on one thread (COM objects do not cross threads).
    pub fn attach(hwnd: isize) -> Option<Self> {
        // SAFETY: initializes COM on this thread. If that fails (already initialized in another mode), the UIA object can still be created.
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            let uia: IUIAutomation =
                match CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER) {
                    Ok(uia) => uia,
                    Err(_) => {
                        CoUninitialize();
                        return None;
                    }
                };
            let window = uia.ElementFromHandle(HWND(hwnd as *mut _)).ok();
            match window {
                Some(window) => Some(Self { uia, window }),
                None => {
                    CoUninitialize();
                    None
                }
            }
        }
    }

    fn type_condition(&self, id: UIA_CONTROLTYPE_ID) -> Option<IUIAutomationCondition> {
        let value = Variant::i4(id.0);
        // SAFETY: read-only query.
        unsafe {
            self.uia
                .CreatePropertyCondition(UIA_ControlTypePropertyId, &value.0)
                .ok()
        }
    }

    /// The web page itself (Document). The browser address bar is also an Edit, so search for the input box only inside the page.
    /// Takes the largest one: some sites embed small frames in the page, and the first one is the wrong one.
    fn document(&self) -> Option<IUIAutomationElement> {
        let condition = self.type_condition(UIA_DocumentControlTypeId)?;
        // SAFETY: read-only query.
        let all = unsafe { self.window.FindAll(TreeScope_Descendants, &condition) }.ok()?;
        let count = unsafe { all.Length() }.unwrap_or(0);
        (0..count)
            .filter_map(|index| unsafe { all.GetElement(index) }.ok())
            .filter_map(|element| Self::rect(&element).map(|rect| (area(&rect), element)))
            .max_by_key(|(size, _)| *size)
            .map(|(_, element)| element)
    }

    fn rect(element: &IUIAutomationElement) -> Option<RECT> {
        // SAFETY: read-only property.
        unsafe { element.CurrentBoundingRectangle().ok() }
    }

    fn is_edit(element: &IUIAutomationElement) -> bool {
        // SAFETY: read-only property.
        unsafe { element.CurrentControlType() }.is_ok_and(|kind| kind == UIA_EditControlTypeId)
    }

    /// The input box in the page: the input box that has focus, if any; otherwise the largest visible one in the page.
    fn composer(&self) -> Option<(IUIAutomationElement, bool)> {
        let document = self.document()?;
        let page = Self::rect(&document)?;
        // SAFETY: read-only query.
        let focused = unsafe { self.uia.GetFocusedElement() }.ok();
        if let Some(focused) = focused {
            if Self::is_edit(&focused)
                && Self::rect(&focused).is_some_and(|r| area(&r) > 0 && center_in(&r, &page))
            {
                return Some((focused, true));
            }
        }
        let condition = self.type_condition(UIA_EditControlTypeId)?;
        // SAFETY: read-only query.
        let all = unsafe { document.FindAll(TreeScope_Descendants, &condition) }.ok()?;
        let count = unsafe { all.Length() }.unwrap_or(0);
        let mut best: Option<(IUIAutomationElement, i64)> = None;
        for index in 0..count {
            // SAFETY: the index is within Length. Read-only property.
            let Ok(element) = (unsafe { all.GetElement(index) }) else {
                continue;
            };
            let visible = unsafe { element.CurrentIsOffscreen() }.is_ok_and(|off| !off.as_bool())
                && unsafe { element.CurrentIsEnabled() }.is_ok_and(|on| on.as_bool());
            let Some(rect) = Self::rect(&element) else {
                continue;
            };
            let size = area(&rect);
            if visible && size > 0 && best.as_ref().is_none_or(|(_, s)| size > *s) {
                best = Some((element, size));
            }
        }
        best.map(|(element, _)| (element, false))
    }

    /// Gives focus to the input box in the page. Returns true when focus is on it now.
    pub fn focus(&self) -> bool {
        let Some((element, focused)) = self.composer() else {
            return self.focus_in_page();
        };
        if focused {
            return true;
        }
        // SAFETY: sets focus only on the input box.
        if unsafe { element.SetFocus() }.is_err() {
            return false;
        }
        std::thread::sleep(std::time::Duration::from_millis(150));
        self.composer().is_some_and(|(_, focused)| focused)
    }

    /// No readable input box in the page (some sites hide the editor from screen readers): if focus is in the page, treat it as
    /// on the input box, because these sites put the cursor there. Later checks use the attachment card and the message in the chat, not the input box.
    fn focus_in_page(&self) -> bool {
        let Some(document) = self.document() else {
            return false;
        };
        let Some(page) = Self::rect(&document) else {
            return false;
        };
        // SAFETY: read-only query.
        let Ok(focused) = (unsafe { self.uia.GetFocusedElement() }) else {
            return false;
        };
        Self::rect(&focused).is_some_and(|r| center_in(&r, &page)) && !Self::is_edit(&focused)
    }

    /// The text in the input box now (None if it cannot be read).
    pub fn text(&self) -> Option<String> {
        let (element, _) = self.composer()?;
        // SAFETY: read-only pattern interface.
        unsafe {
            if let Ok(value) =
                element.GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId)
            {
                if let Ok(text) = value.CurrentValue() {
                    return Some(text.to_string());
                }
            }
            let pattern = element
                .GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId)
                .ok()?;
            Some(pattern.DocumentRange().ok()?.GetText(-1).ok()?.to_string())
        }
    }

    /// Whether the page has an element whose name contains `needle` (the attachment card has the file name).
    pub fn shows(&self, needle: &str) -> bool {
        let Some(document) = self.document() else {
            return false;
        };
        let value = Variant::bstr(needle);
        // SAFETY: read-only query.
        unsafe {
            let Ok(condition) = self.uia.CreatePropertyConditionEx(
                UIA_NamePropertyId,
                &value.0,
                PropertyConditionFlags_MatchSubstring,
            ) else {
                return false;
            };
            document
                .FindFirst(TreeScope_Descendants, &condition)
                .is_ok()
        }
    }
}
