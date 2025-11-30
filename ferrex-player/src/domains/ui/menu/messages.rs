use uuid::Uuid;

use crate::domains::ui::menu::MenuButton;

#[derive(Clone, Debug)]
pub enum PosterMenuMessage {
    Close(Uuid),
    /// Initiate flip interaction (single flip or continuous)
    Start(Uuid),
    /// User input release
    End(Uuid),
    /// Button clicked on backface menu
    ButtonClicked(Uuid, MenuButton),
}
