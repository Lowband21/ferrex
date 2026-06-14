use ferrex_core::player_prelude::MovieID;
use ferrex_player_ui::{
    domains::ui::{
        types::ViewState,
        views::tenfoot::{
            detail::{is_tenfoot_detail_route, view_tenfoot_detail},
            home::{is_tenfoot_home_route, view_tenfoot_home},
        },
    },
    state::{InterfaceMode, State},
};

fn state_with_mode(mode: InterfaceMode) -> State {
    State::new_with_interface_mode("http://localhost:3000".to_string(), mode)
}

#[tokio::test]
async fn tenfoot_home_route_requires_tenfoot_mode_and_home_surface() {
    let mut state = state_with_mode(InterfaceMode::TenFoot);
    state.is_authenticated = true;
    state.domains.ui.state.view = ViewState::Library;

    assert!(is_tenfoot_home_route(&state));
    let _ = view_tenfoot_home(&state);

    state.interface_mode = InterfaceMode::Desktop;
    assert!(!is_tenfoot_home_route(&state));
}

#[tokio::test]
async fn tenfoot_detail_route_requires_tenfoot_mode_and_detail_surface() {
    let mut state = state_with_mode(InterfaceMode::TenFoot);
    state.is_authenticated = true;
    state.domains.ui.state.view = ViewState::MovieDetail {
        movie_id: MovieID::new(),
        backdrop_handle: None,
    };

    assert!(is_tenfoot_detail_route(&state));
    let _ = view_tenfoot_detail(&state);

    state.domains.ui.state.view = ViewState::Library;
    assert!(!is_tenfoot_detail_route(&state));
}
