pub mod controller;
pub mod focus;
pub mod subscriptions;

use iced::window;
use std::collections::HashMap;

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum WindowKind {
    Main,
    Search,
}

#[derive(Debug, Default)]
pub struct WindowManager {
    by_kind: HashMap<WindowKind, window::Id>,
    by_id: HashMap<window::Id, WindowKind>,
    pub focused: Option<window::Id>,
}

impl WindowManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set(&mut self, kind: WindowKind, id: window::Id) {
        self.by_kind.insert(kind, id);
        self.by_id.insert(id, kind);
    }

    pub fn get(&self, kind: WindowKind) -> Option<window::Id> {
        self.by_kind.get(&kind).copied()
    }

    pub fn get_kind(&self, id: window::Id) -> Option<WindowKind> {
        self.by_id.get(&id).copied()
    }

    pub fn remove_by_id(&mut self, id: window::Id) -> Option<WindowKind> {
        if let Some(kind) = self.by_id.remove(&id) {
            let _ = self.by_kind.remove(&kind);
            if self.focused == Some(id) {
                self.focused = None;
            }
            Some(kind)
        } else {
            None
        }
    }

    pub fn is_search_window(&self, id: window::Id) -> bool {
        matches!(self.get_kind(id), Some(WindowKind::Search))
    }

    pub fn is_main_window(&self, id: window::Id) -> bool {
        matches!(self.get_kind(id), Some(WindowKind::Main))
    }
}
