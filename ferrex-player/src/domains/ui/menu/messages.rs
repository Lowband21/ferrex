use crate::domains::ui::menu::MenuButton;
use crate::infra::shader_widgets::poster::PosterInstanceKey;

#[derive(Clone, Debug)]
pub enum PosterMenuMessage {
    Close(PosterInstanceKey),
    /// Initiate flip interaction (single flip or continuous)
    Start(PosterInstanceKey),
    /// User input release
    End(PosterInstanceKey),
    /// Button clicked on backface menu
    ButtonClicked(PosterInstanceKey, MenuButton),
}
