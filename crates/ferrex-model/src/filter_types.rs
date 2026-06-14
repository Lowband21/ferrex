use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UiGenre {
    Action,
    Adventure,
    Animation,
    Comedy,
    Crime,
    Documentary,
    Drama,
    Family,
    Fantasy,
    History,
    Horror,
    Music,
    Mystery,
    Romance,
    ScienceFiction,
    Thriller,
    War,
    Western,
}

impl UiGenre {
    pub fn all() -> &'static [UiGenre] {
        use UiGenre::*;
        &[
            Action,
            Adventure,
            Animation,
            Comedy,
            Crime,
            Documentary,
            Drama,
            Family,
            Fantasy,
            History,
            Horror,
            Music,
            Mystery,
            Romance,
            ScienceFiction,
            Thriller,
            War,
            Western,
        ]
    }

    pub fn api_name(&self) -> &'static str {
        match self {
            UiGenre::Action => "Action",
            UiGenre::Adventure => "Adventure",
            UiGenre::Animation => "Animation",
            UiGenre::Comedy => "Comedy",
            UiGenre::Crime => "Crime",
            UiGenre::Documentary => "Documentary",
            UiGenre::Drama => "Drama",
            UiGenre::Family => "Family",
            UiGenre::Fantasy => "Fantasy",
            UiGenre::History => "History",
            UiGenre::Horror => "Horror",
            UiGenre::Music => "Music",
            UiGenre::Mystery => "Mystery",
            UiGenre::Romance => "Romance",
            UiGenre::ScienceFiction => "Science Fiction",
            UiGenre::Thriller => "Thriller",
            UiGenre::War => "War",
            UiGenre::Western => "Western",
        }
    }
}

impl fmt::Display for UiGenre {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.api_name())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiDecade {
    Y2020s,
    Y2010s,
    Y2000s,
    Y1990s,
    Y1980s,
    Y1970s,
    Y1960s,
    Y1950s,
}

impl UiDecade {
    pub fn all() -> &'static [UiDecade] {
        use UiDecade::*;
        &[
            Y2020s, Y2010s, Y2000s, Y1990s, Y1980s, Y1970s, Y1960s, Y1950s,
        ]
    }
    pub fn label(&self) -> &'static str {
        match self {
            UiDecade::Y2020s => "2020s",
            UiDecade::Y2010s => "2010s",
            UiDecade::Y2000s => "2000s",
            UiDecade::Y1990s => "1990s",
            UiDecade::Y1980s => "1980s",
            UiDecade::Y1970s => "1970s",
            UiDecade::Y1960s => "1960s",
            UiDecade::Y1950s => "1950s",
        }
    }
    pub fn start_year(&self) -> u16 {
        match self {
            UiDecade::Y2020s => 2020,
            UiDecade::Y2010s => 2010,
            UiDecade::Y2000s => 2000,
            UiDecade::Y1990s => 1990,
            UiDecade::Y1980s => 1980,
            UiDecade::Y1970s => 1970,
            UiDecade::Y1960s => 1960,
            UiDecade::Y1950s => 1950,
        }
    }
}

impl fmt::Display for UiDecade {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.label())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiResolution {
    Any,
    SD,      // <= 576
    HD720,   // 720p
    FHD1080, // 1080p
    QHD1440, // 1440p
    UHD4K,   // 2160p
    UHD8K,   // 4320p
}

impl UiResolution {
    pub fn all() -> &'static [UiResolution] {
        use UiResolution::*;
        &[Any, SD, HD720, FHD1080, QHD1440, UHD4K, UHD8K]
    }
    pub fn label(&self) -> &'static str {
        match self {
            UiResolution::Any => "Any",
            UiResolution::SD => "SD (<=576p)",
            UiResolution::HD720 => "720p",
            UiResolution::FHD1080 => "1080p",
            UiResolution::QHD1440 => "1440p",
            UiResolution::UHD4K => "4K (2160p)",
            UiResolution::UHD8K => "8K (4320p)",
        }
    }
}

impl fmt::Display for UiResolution {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.label())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiWatchStatus {
    Any,
    Unwatched,
    InProgress,
    Completed,
}

impl UiWatchStatus {
    pub fn all() -> &'static [UiWatchStatus] {
        use UiWatchStatus::*;
        &[Any, Unwatched, InProgress, Completed]
    }
    pub fn label(&self) -> &'static str {
        match self {
            UiWatchStatus::Any => "Any",
            UiWatchStatus::Unwatched => "Unwatched",
            UiWatchStatus::InProgress => "In Progress",
            UiWatchStatus::Completed => "Completed",
        }
    }
}

impl fmt::Display for UiWatchStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.label())
    }
}
