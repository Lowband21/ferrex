//! Semantic typography roles and text metrics for adaptive detail surfaces.
//!
//! This layer translates global [`FontTokens`] and [`SpacingTokens`] into
//! detail-specific roles. Renderers can ask for a semantic role instead of
//! duplicating composition checks for hero copy, facts, metadata, rails, cast,
//! notices, and 10-foot affordances.

use crate::infra::design_tokens::{FontTokens, SizeProvider, SpacingTokens};

use super::layout::DetailComposition;

/// Semantic color intent for detail text roles.
///
/// The intent stays theme-agnostic so renderers can map it to the active theme
/// palette without the typography layer depending on Iced colors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DetailColorIntent {
    Primary,
    Secondary,
    Subdued,
    Dimmed,
    Accent,
    Success,
    Warning,
    Error,
}

/// Horizontal alignment strategy for a detail text role or group.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DetailTextAlignment {
    Start,
    Center,
    End,
}

/// Overflow guidance for renderers that apply text clipping or budgeting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DetailTextOverflow {
    /// Normal wrapping without a fixed line budget.
    Wrap,
    /// One visual line with trailing truncation when supported by the renderer.
    SingleLineEllipsis,
    /// A wrapped block with a preferred maximum number of lines.
    MultiLine { max_lines: u8 },
    /// Keep content on one row and place it in a horizontal scroller.
    HorizontalScroll,
}

impl DetailTextOverflow {
    pub fn max_lines(self) -> Option<u8> {
        match self {
            Self::MultiLine { max_lines } => Some(max_lines),
            Self::SingleLineEllipsis => Some(1),
            Self::Wrap | Self::HorizontalScroll => None,
        }
    }
}

/// Named detail text roles exposed to renderers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DetailTextRole {
    HeroEyebrow,
    HeroTitle,
    HeroSubtitle,
    HeroOverview,
    Metadata,
    SectionTitle,
    OverviewBody,
    FactLabel,
    FactValue,
    ActionLabel,
    ActionSubtitle,
    Caption,
    RailTitle,
    RailSubtitle,
    CastName,
    CastRole,
    NoticeTitle,
    NoticeBody,
    TenFootFocusLabel,
    TenFootHelper,
}

/// Fully resolved style for a semantic detail text role.
///
/// `line_height` is a relative multiplier matching Iced's `LineHeight::Relative`
/// conversion. Use [`DetailTextStyle::line_height_px`] when an absolute budget is
/// needed for clipping or scroll-height calculations.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DetailTextStyle {
    pub size: f32,
    pub line_height: f32,
    pub color_intent: DetailColorIntent,
    pub spacing_after: f32,
    pub measure: f32,
    pub overflow: DetailTextOverflow,
    pub alignment: DetailTextAlignment,
}

impl DetailTextStyle {
    pub fn line_height_px(self) -> f32 {
        self.size * self.line_height
    }

    pub fn max_lines(self) -> Option<u8> {
        self.overflow.max_lines()
    }
}

/// Preferred structure for fact rows in the active detail composition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DetailFactLayoutMode {
    /// Label and value stack vertically for narrow portrait cards.
    Stacked,
    /// Label uses a fixed measure and value fills the remaining row.
    Inline,
    /// Facts are expected to live in a multi-column panel/card grid.
    TwoColumn,
}

/// Line budgets for caption-like detail text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DetailCaptionBudgets {
    pub hero_title_lines: u8,
    pub hero_overview_lines: u8,
    pub metadata_lines: u8,
    pub overview_lines: u8,
    pub fact_value_lines: u8,
    pub rail_title_lines: u8,
    pub rail_subtitle_lines: u8,
    pub cast_name_lines: u8,
    pub cast_role_lines: u8,
    pub notice_body_lines: u8,
}

/// Renderer-facing measures and layout strategies for detail text groups.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DetailTextMetrics {
    pub hero_copy_width: f32,
    pub hero_alignment: DetailTextAlignment,
    pub overview_measure: f32,
    pub overview_alignment: DetailTextAlignment,
    pub fact_layout_mode: DetailFactLayoutMode,
    pub fact_label_width: f32,
    pub caption_budgets: DetailCaptionBudgets,
    pub metadata_spacing: f32,
    pub metadata_pill_gap: f32,
}

/// Inputs from the solved detail layout that typography needs to resolve
/// readable text measures for the selected composition.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DetailTypographyInput {
    pub composition: DetailComposition,
    pub scale: f32,
    pub content_width: f32,
    pub hero_art_width: f32,
    pub hero_gap: f32,
    pub rail_card_width: f32,
}

/// Semantic typography roles for an adaptive detail layout.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DetailTypography {
    pub metrics: DetailTextMetrics,
    pub hero_eyebrow: DetailTextStyle,
    pub hero_title: DetailTextStyle,
    pub hero_subtitle: DetailTextStyle,
    pub hero_overview: DetailTextStyle,
    pub metadata: DetailTextStyle,
    pub section_title: DetailTextStyle,
    pub overview_body: DetailTextStyle,
    pub fact_label: DetailTextStyle,
    pub fact_value: DetailTextStyle,
    pub action_label: DetailTextStyle,
    pub action_subtitle: DetailTextStyle,
    pub caption: DetailTextStyle,
    pub rail_title: DetailTextStyle,
    pub rail_subtitle: DetailTextStyle,
    pub cast_name: DetailTextStyle,
    pub cast_role: DetailTextStyle,
    pub notice_title: DetailTextStyle,
    pub notice_body: DetailTextStyle,
    pub tenfoot_focus_label: DetailTextStyle,
    pub tenfoot_helper: DetailTextStyle,
}

impl DetailTypography {
    /// Resolve detail typography from the central size provider and layout
    /// constraints selected by [`DetailComposition`].
    pub fn from_size_provider(
        sizes: &SizeProvider,
        input: DetailTypographyInput,
    ) -> Self {
        Self::from_tokens(sizes.font, sizes.spacing, input)
    }

    /// Resolve detail typography from already scaled font and spacing tokens.
    pub fn from_tokens(
        font: FontTokens,
        spacing: SpacingTokens,
        input: DetailTypographyInput,
    ) -> Self {
        let scale = input.scale.clamp(0.5, 3.0);
        let composition = input.composition;
        let metrics = detail_text_metrics(spacing, input, scale);
        let hero_alignment = metrics.hero_alignment;
        let overview_alignment = metrics.overview_alignment;
        let caption_budgets = metrics.caption_budgets;

        let hero_title_size = match composition {
            DetailComposition::CompactPortrait => font.title,
            DetailComposition::CompactLandscape => font.title_lg,
            DetailComposition::BalancedDesktop => font.display,
            DetailComposition::CinematicWide => font.display * 1.25,
            DetailComposition::TenFoot => font.display * 1.45,
        };
        let hero_title_line_height = match composition {
            DetailComposition::CompactPortrait => 1.12,
            DetailComposition::CompactLandscape => 1.08,
            DetailComposition::BalancedDesktop => 1.08,
            DetailComposition::CinematicWide => 1.06,
            DetailComposition::TenFoot => 1.04,
        };
        let hero_overview_size = match composition {
            DetailComposition::TenFoot => font.body_lg * 1.12,
            DetailComposition::CinematicWide => font.body * 1.05,
            _ => font.body,
        };
        let metadata_size = match composition {
            DetailComposition::TenFoot => font.caption * 1.15,
            DetailComposition::CinematicWide => font.caption,
            _ => font.small,
        };
        let action_label_size = match composition {
            DetailComposition::TenFoot => font.subtitle,
            _ => font.body,
        };
        let action_subtitle_size = match composition {
            DetailComposition::TenFoot => font.caption * 1.10,
            _ => font.small,
        };
        let caption_size = match composition {
            DetailComposition::TenFoot => font.body_lg,
            _ => font.caption,
        };
        let cast_name_size = match composition {
            DetailComposition::TenFoot => font.caption,
            _ => font.small,
        };
        let cast_role_size = match composition {
            DetailComposition::TenFoot => font.small,
            _ => font.micro,
        };
        let notice_title_size = match composition {
            DetailComposition::TenFoot => font.title,
            _ => font.subtitle,
        };
        let focus_label_size = match composition {
            DetailComposition::TenFoot => font.body_lg * 1.15,
            DetailComposition::CinematicWide => font.body_lg,
            _ => font.body,
        };
        let helper_size = match composition {
            DetailComposition::TenFoot => font.caption * 1.15,
            _ => font.caption,
        };

        Self {
            metrics,
            hero_eyebrow: style(
                font.caption,
                1.18,
                DetailColorIntent::Secondary,
                spacing.xs,
                metrics.hero_copy_width,
                DetailTextOverflow::SingleLineEllipsis,
                hero_alignment,
            ),
            hero_title: style(
                hero_title_size,
                hero_title_line_height,
                DetailColorIntent::Primary,
                spacing.sm,
                metrics.hero_copy_width,
                DetailTextOverflow::MultiLine {
                    max_lines: caption_budgets.hero_title_lines,
                },
                hero_alignment,
            ),
            hero_subtitle: style(
                font.subtitle,
                1.20,
                DetailColorIntent::Secondary,
                spacing.sm,
                metrics.hero_copy_width,
                DetailTextOverflow::MultiLine { max_lines: 2 },
                hero_alignment,
            ),
            hero_overview: style(
                hero_overview_size,
                match composition {
                    DetailComposition::TenFoot => 1.38,
                    _ => 1.42,
                },
                DetailColorIntent::Primary,
                spacing.md,
                metrics.overview_measure.min(metrics.hero_copy_width),
                DetailTextOverflow::MultiLine {
                    max_lines: caption_budgets.hero_overview_lines,
                },
                hero_alignment,
            ),
            metadata: style(
                metadata_size,
                1.18,
                DetailColorIntent::Secondary,
                metrics.metadata_spacing,
                metrics.hero_copy_width,
                DetailTextOverflow::HorizontalScroll,
                hero_alignment,
            ),
            section_title: style(
                match composition {
                    DetailComposition::TenFoot => font.title,
                    _ => font.subtitle,
                },
                1.15,
                DetailColorIntent::Primary,
                spacing.sm,
                metrics.overview_measure,
                DetailTextOverflow::SingleLineEllipsis,
                DetailTextAlignment::Start,
            ),
            overview_body: style(
                hero_overview_size,
                1.48,
                DetailColorIntent::Primary,
                spacing.md,
                metrics.overview_measure,
                DetailTextOverflow::MultiLine {
                    max_lines: caption_budgets.overview_lines,
                },
                overview_alignment,
            ),
            fact_label: style(
                font.small,
                1.20,
                DetailColorIntent::Secondary,
                spacing.xs,
                metrics.fact_label_width,
                DetailTextOverflow::SingleLineEllipsis,
                DetailTextAlignment::Start,
            ),
            fact_value: style(
                font.caption,
                1.24,
                DetailColorIntent::Primary,
                spacing.sm,
                (metrics.overview_measure - metrics.fact_label_width)
                    .max(metrics.fact_label_width),
                DetailTextOverflow::MultiLine {
                    max_lines: caption_budgets.fact_value_lines,
                },
                DetailTextAlignment::Start,
            ),
            action_label: style(
                action_label_size,
                1.10,
                DetailColorIntent::Primary,
                spacing.xs,
                metrics.hero_copy_width,
                DetailTextOverflow::SingleLineEllipsis,
                DetailTextAlignment::Center,
            ),
            action_subtitle: style(
                action_subtitle_size,
                1.12,
                DetailColorIntent::Secondary,
                spacing.xs,
                metrics.hero_copy_width,
                DetailTextOverflow::SingleLineEllipsis,
                DetailTextAlignment::Center,
            ),
            caption: style(
                caption_size,
                1.22,
                DetailColorIntent::Secondary,
                spacing.xs,
                input.rail_card_width.max(1.0),
                DetailTextOverflow::MultiLine {
                    max_lines: caption_budgets.rail_title_lines,
                },
                DetailTextAlignment::Start,
            ),
            rail_title: style(
                caption_size,
                1.22,
                DetailColorIntent::Primary,
                spacing.xs,
                input.rail_card_width.max(1.0),
                DetailTextOverflow::MultiLine {
                    max_lines: caption_budgets.rail_title_lines,
                },
                DetailTextAlignment::Start,
            ),
            rail_subtitle: style(
                font.small,
                1.18,
                DetailColorIntent::Secondary,
                spacing.xs,
                input.rail_card_width.max(1.0),
                DetailTextOverflow::MultiLine {
                    max_lines: caption_budgets.rail_subtitle_lines,
                },
                DetailTextAlignment::Start,
            ),
            cast_name: style(
                cast_name_size,
                1.18,
                DetailColorIntent::Primary,
                spacing.xs,
                input.rail_card_width.max(1.0),
                DetailTextOverflow::MultiLine {
                    max_lines: caption_budgets.cast_name_lines,
                },
                DetailTextAlignment::Center,
            ),
            cast_role: style(
                cast_role_size,
                1.16,
                DetailColorIntent::Secondary,
                spacing.xs,
                input.rail_card_width.max(1.0),
                DetailTextOverflow::MultiLine {
                    max_lines: caption_budgets.cast_role_lines,
                },
                DetailTextAlignment::Center,
            ),
            notice_title: style(
                notice_title_size,
                1.16,
                DetailColorIntent::Accent,
                spacing.xs,
                metrics.overview_measure,
                DetailTextOverflow::MultiLine { max_lines: 2 },
                DetailTextAlignment::Start,
            ),
            notice_body: style(
                caption_size,
                1.34,
                DetailColorIntent::Secondary,
                spacing.sm,
                metrics.overview_measure,
                DetailTextOverflow::MultiLine {
                    max_lines: caption_budgets.notice_body_lines,
                },
                DetailTextAlignment::Start,
            ),
            tenfoot_focus_label: style(
                focus_label_size,
                1.10,
                DetailColorIntent::Primary,
                spacing.sm,
                metrics.hero_copy_width,
                DetailTextOverflow::SingleLineEllipsis,
                DetailTextAlignment::Center,
            ),
            tenfoot_helper: style(
                helper_size,
                1.18,
                DetailColorIntent::Secondary,
                spacing.md,
                metrics.hero_copy_width,
                DetailTextOverflow::MultiLine { max_lines: 2 },
                DetailTextAlignment::Center,
            ),
        }
    }

    /// Return the resolved style for a semantic text role.
    pub fn role(&self, role: DetailTextRole) -> DetailTextStyle {
        match role {
            DetailTextRole::HeroEyebrow => self.hero_eyebrow,
            DetailTextRole::HeroTitle => self.hero_title,
            DetailTextRole::HeroSubtitle => self.hero_subtitle,
            DetailTextRole::HeroOverview => self.hero_overview,
            DetailTextRole::Metadata => self.metadata,
            DetailTextRole::SectionTitle => self.section_title,
            DetailTextRole::OverviewBody => self.overview_body,
            DetailTextRole::FactLabel => self.fact_label,
            DetailTextRole::FactValue => self.fact_value,
            DetailTextRole::ActionLabel => self.action_label,
            DetailTextRole::ActionSubtitle => self.action_subtitle,
            DetailTextRole::Caption => self.caption,
            DetailTextRole::RailTitle => self.rail_title,
            DetailTextRole::RailSubtitle => self.rail_subtitle,
            DetailTextRole::CastName => self.cast_name,
            DetailTextRole::CastRole => self.cast_role,
            DetailTextRole::NoticeTitle => self.notice_title,
            DetailTextRole::NoticeBody => self.notice_body,
            DetailTextRole::TenFootFocusLabel => self.tenfoot_focus_label,
            DetailTextRole::TenFootHelper => self.tenfoot_helper,
        }
    }
}

fn detail_text_metrics(
    spacing: SpacingTokens,
    input: DetailTypographyInput,
    scale: f32,
) -> DetailTextMetrics {
    let composition = input.composition;
    let content_width = input.content_width.max(1.0);
    let horizontal_copy_width =
        (content_width - input.hero_art_width - input.hero_gap)
            .max(content_width * 0.42)
            .max(1.0);
    let hero_copy_width = match composition {
        DetailComposition::CompactPortrait => content_width.min(620.0 * scale),
        DetailComposition::CompactLandscape => {
            horizontal_copy_width.min(520.0 * scale)
        }
        DetailComposition::BalancedDesktop => {
            horizontal_copy_width.min(680.0 * scale)
        }
        DetailComposition::CinematicWide => {
            horizontal_copy_width.min(820.0 * scale)
        }
        DetailComposition::TenFoot => horizontal_copy_width.min(980.0 * scale),
    }
    .min(content_width)
    .max(1.0);

    let overview_measure = match composition {
        DetailComposition::CompactPortrait => content_width.min(560.0 * scale),
        DetailComposition::CompactLandscape => content_width.min(600.0 * scale),
        DetailComposition::BalancedDesktop => content_width.min(720.0 * scale),
        DetailComposition::CinematicWide => content_width.min(860.0 * scale),
        DetailComposition::TenFoot => content_width.min(980.0 * scale),
    }
    .max(1.0);

    let fact_layout_mode = match composition {
        DetailComposition::CompactPortrait => DetailFactLayoutMode::Stacked,
        DetailComposition::CompactLandscape => DetailFactLayoutMode::Inline,
        DetailComposition::BalancedDesktop
        | DetailComposition::CinematicWide
        | DetailComposition::TenFoot => DetailFactLayoutMode::TwoColumn,
    };
    let fact_label_width = match composition {
        DetailComposition::CompactPortrait => {
            (content_width * 0.36).min(120.0 * scale).max(84.0 * scale)
        }
        DetailComposition::CompactLandscape => 128.0 * scale,
        DetailComposition::BalancedDesktop => 140.0 * scale,
        DetailComposition::CinematicWide => 156.0 * scale,
        DetailComposition::TenFoot => 190.0 * scale,
    }
    .min(overview_measure * 0.48)
    .max(1.0);

    let caption_budgets = match composition {
        DetailComposition::CompactPortrait => DetailCaptionBudgets {
            hero_title_lines: 3,
            hero_overview_lines: 5,
            metadata_lines: 1,
            overview_lines: 7,
            fact_value_lines: 3,
            rail_title_lines: 2,
            rail_subtitle_lines: 2,
            cast_name_lines: 2,
            cast_role_lines: 2,
            notice_body_lines: 4,
        },
        DetailComposition::CompactLandscape => DetailCaptionBudgets {
            hero_title_lines: 2,
            hero_overview_lines: 3,
            metadata_lines: 1,
            overview_lines: 5,
            fact_value_lines: 2,
            rail_title_lines: 2,
            rail_subtitle_lines: 1,
            cast_name_lines: 2,
            cast_role_lines: 1,
            notice_body_lines: 3,
        },
        DetailComposition::BalancedDesktop => DetailCaptionBudgets {
            hero_title_lines: 3,
            hero_overview_lines: 4,
            metadata_lines: 1,
            overview_lines: 6,
            fact_value_lines: 2,
            rail_title_lines: 2,
            rail_subtitle_lines: 1,
            cast_name_lines: 2,
            cast_role_lines: 1,
            notice_body_lines: 4,
        },
        DetailComposition::CinematicWide => DetailCaptionBudgets {
            hero_title_lines: 2,
            hero_overview_lines: 4,
            metadata_lines: 1,
            overview_lines: 6,
            fact_value_lines: 2,
            rail_title_lines: 2,
            rail_subtitle_lines: 1,
            cast_name_lines: 2,
            cast_role_lines: 1,
            notice_body_lines: 4,
        },
        DetailComposition::TenFoot => DetailCaptionBudgets {
            hero_title_lines: 2,
            hero_overview_lines: 3,
            metadata_lines: 1,
            overview_lines: 5,
            fact_value_lines: 2,
            rail_title_lines: 1,
            rail_subtitle_lines: 1,
            cast_name_lines: 1,
            cast_role_lines: 1,
            notice_body_lines: 3,
        },
    };

    let (metadata_spacing, metadata_pill_gap) = match composition {
        DetailComposition::TenFoot => (spacing.lg, spacing.md),
        DetailComposition::CinematicWide
        | DetailComposition::BalancedDesktop => (spacing.md, spacing.sm),
        DetailComposition::CompactLandscape
        | DetailComposition::CompactPortrait => (spacing.sm, spacing.xs),
    };
    let hero_alignment = match composition {
        DetailComposition::CompactPortrait => DetailTextAlignment::Center,
        _ => DetailTextAlignment::Start,
    };

    DetailTextMetrics {
        hero_copy_width,
        hero_alignment,
        overview_measure,
        overview_alignment: DetailTextAlignment::Start,
        fact_layout_mode,
        fact_label_width,
        caption_budgets,
        metadata_spacing,
        metadata_pill_gap,
    }
}

fn style(
    size: f32,
    line_height: f32,
    color_intent: DetailColorIntent,
    spacing_after: f32,
    measure: f32,
    overflow: DetailTextOverflow,
    alignment: DetailTextAlignment,
) -> DetailTextStyle {
    DetailTextStyle {
        size,
        line_height,
        color_intent,
        spacing_after,
        measure: measure.max(1.0),
        overflow,
        alignment,
    }
}
