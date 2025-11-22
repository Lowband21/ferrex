pub mod update;

use crate::domains::ui::messages::UiMessage;

pub use update::update_view_model_ui;

#[derive(Clone)]
pub enum ViewModelMessage {
    RefreshViewModels,
    UpdateViewModelFilters,
}

impl From<ViewModelMessage> for UiMessage {
    fn from(msg: ViewModelMessage) -> Self {
        UiMessage::ViewModels(msg)
    }
}

impl ViewModelMessage {
    pub fn name(&self) -> &'static str {
        match self {
            Self::RefreshViewModels => "UI::RefreshViewModels",
            Self::UpdateViewModelFilters => "UI::UpdateViewModelFilters",
        }
    }
}

impl std::fmt::Debug for ViewModelMessage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RefreshViewModels => write!(f, "UI::RefreshViewModels"),
            Self::UpdateViewModelFilters => {
                write!(f, "UI::UpdateViewModelFilters")
            }
        }
    }
}
