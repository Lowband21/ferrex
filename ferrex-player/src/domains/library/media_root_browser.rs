use ferrex_core::player_prelude::{
    MediaRootBreadcrumb, MediaRootBrowseResponse, MediaRootEntry,
};

#[derive(Debug, Clone)]
pub struct State {
    pub visible: bool,
    pub is_loading: bool,
    pub media_root: Option<String>,
    pub current_path: String,
    pub parent_path: Option<String>,
    pub display_path: String,
    pub breadcrumbs: Vec<MediaRootBreadcrumb>,
    pub entries: Vec<MediaRootEntry>,
    pub error: Option<String>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            visible: false,
            is_loading: false,
            media_root: None,
            current_path: String::new(),
            parent_path: None,
            display_path: "/".into(),
            breadcrumbs: vec![MediaRootBreadcrumb {
                label: "/".into(),
                relative_path: String::new(),
            }],
            entries: Vec::new(),
            error: None,
        }
    }
}

#[derive(Debug, Clone)]
pub enum Message {
    Open,
    Close,
    Browse { path: Option<String> },
    ListingLoaded(Result<MediaRootBrowseResponse, String>),
    PathSelected(String),
}

impl Message {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Open => "Library::MediaRootBrowser::Open",
            Self::Close => "Library::MediaRootBrowser::Close",
            Self::Browse { .. } => "Library::MediaRootBrowser::Browse",
            Self::ListingLoaded(_) => {
                "Library::MediaRootBrowser::ListingLoaded"
            }
            Self::PathSelected(_) => "Library::MediaRootBrowser::PathSelected",
        }
    }
}
