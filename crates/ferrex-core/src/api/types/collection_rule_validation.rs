use super::collections::*;
use chrono::{DateTime, NaiveDate, Utc};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, fmt::Write as _};
use uuid::Uuid;

#[derive(Serialize)]
struct CollectionRuleHashInput<'a> {
    schema_version: u16,
    predicate: &'a CollectionRulePredicate,
    sort: &'a CollectionSortPolicy,
    limit: &'a CollectionLimitPolicy,
}

pub(super) fn normalized_rule(
    rule: &DynamicCollectionRule,
) -> DynamicCollectionRule {
    DynamicCollectionRule {
        schema_version: rule.schema_version,
        predicate: normalize_predicate(&rule.predicate),
        sort: normalize_sort_policy(&rule.sort),
        limit: normalize_limit_policy(&rule.limit),
    }
}

pub(super) fn validate_rule(
    rule: &DynamicCollectionRule,
) -> CollectionRuleValidationReport {
    let normalized = normalized_rule(rule);
    let mut validator = RuleValidator::default();
    validator.validate_rule(&normalized);

    let rule_hash_input = rule_hash_input_json(&normalized).unwrap_or_default();
    let valid = validator.errors.is_empty();
    let rule_hash = valid
        .then(|| rule_hash(&normalized))
        .transpose()
        .ok()
        .flatten();
    let watch_user_ids =
        validator.watch_user_ids.iter().copied().collect::<Vec<_>>();

    CollectionRuleValidationReport {
        valid,
        errors: validator.errors,
        rule_hash_input,
        rule_hash,
        summary: rule_summary(&normalized),
        uses_user_scoped_watch_data: !watch_user_ids.is_empty(),
        watch_user_ids,
    }
}

pub(super) fn rule_hash_input_json(
    rule: &DynamicCollectionRule,
) -> Result<String, serde_json::Error> {
    let normalized = normalized_rule(rule);
    let input = CollectionRuleHashInput {
        schema_version: normalized.schema_version,
        predicate: &normalized.predicate,
        sort: &normalized.sort,
        limit: &normalized.limit,
    };
    serde_json::to_string(&input)
}

pub(super) fn rule_hash(
    rule: &DynamicCollectionRule,
) -> Result<String, serde_json::Error> {
    let input = rule_hash_input_json(rule)?;
    let digest = Sha256::digest(input.as_bytes());
    Ok(format!("{}:{:x}", COLLECTION_RULE_HASH_ALGORITHM, digest))
}

pub(super) fn rule_summary(rule: &DynamicCollectionRule) -> String {
    format!(
        "{}; {}; {}",
        predicate_summary(&rule.predicate),
        sort_summary(&rule.sort),
        limit_summary(&rule.limit)
    )
}

pub(super) fn watch_user_ids(rule: &DynamicCollectionRule) -> Vec<Uuid> {
    let mut ids = BTreeSet::new();
    collect_predicate_watch_user_ids(&rule.predicate, &mut ids);
    for key in &rule.sort.keys {
        if is_user_scoped_sort_field(key.field) {
            if let Some(user_id) = key.user_id {
                ids.insert(user_id);
            }
        }
    }
    ids.into_iter().collect()
}

fn normalize_predicate(
    predicate: &CollectionRulePredicate,
) -> CollectionRulePredicate {
    match predicate {
        CollectionRulePredicate::All { clauses } => {
            let mut normalized =
                clauses.iter().map(normalize_predicate).collect::<Vec<_>>();
            normalized.sort_by_key(predicate_sort_key);
            CollectionRulePredicate::All {
                clauses: normalized,
            }
        }
        CollectionRulePredicate::Any { clauses } => {
            let mut normalized =
                clauses.iter().map(normalize_predicate).collect::<Vec<_>>();
            normalized.sort_by_key(predicate_sort_key);
            CollectionRulePredicate::Any {
                clauses: normalized,
            }
        }
        CollectionRulePredicate::Not { clause } => {
            CollectionRulePredicate::Not {
                clause: Box::new(normalize_predicate(clause)),
            }
        }
        CollectionRulePredicate::Field {
            field,
            operator,
            value,
        } => CollectionRulePredicate::Field {
            field: *field,
            operator: *operator,
            value: normalize_value(*field, *operator, value),
        },
    }
}

fn predicate_sort_key(predicate: &CollectionRulePredicate) -> String {
    serde_json::to_string(predicate)
        .unwrap_or_else(|_| predicate_summary(predicate))
}

fn normalize_sort_policy(sort: &CollectionSortPolicy) -> CollectionSortPolicy {
    CollectionSortPolicy {
        schema_version: sort.schema_version,
        keys: sort
            .keys
            .iter()
            .map(|key| CollectionSortKey {
                field: key.field,
                direction: key.direction,
                nulls: key.nulls,
                user_id: key.user_id,
            })
            .collect(),
        tie_breaker: sort.tie_breaker,
    }
}

fn normalize_limit_policy(
    limit: &CollectionLimitPolicy,
) -> CollectionLimitPolicy {
    CollectionLimitPolicy {
        schema_version: limit.schema_version,
        max_items: limit.max_items,
        per_media_type: limit.per_media_type,
        window: limit.window,
    }
}

fn normalize_value(
    field: CollectionRuleField,
    operator: CollectionRuleOperator,
    value: &CollectionRuleValue,
) -> CollectionRuleValue {
    let case_insensitive = field_uses_case_insensitive_text(field);
    match value {
        CollectionRuleValue::String(value) => {
            CollectionRuleValue::String(normalize_text(value, case_insensitive))
        }
        CollectionRuleValue::Strings(values) => CollectionRuleValue::Strings(
            normalize_string_set(values, case_insensitive, operator),
        ),
        CollectionRuleValue::Integer(value) => {
            CollectionRuleValue::Integer(*value)
        }
        CollectionRuleValue::Integers(values) => {
            let mut values = values.clone();
            if should_sort_values(operator) {
                values.sort_unstable();
            }
            if should_dedup_values(operator) {
                values.dedup();
            }
            CollectionRuleValue::Integers(values)
        }
        CollectionRuleValue::Decimal(value) => {
            CollectionRuleValue::Decimal(normalize_decimal(value))
        }
        CollectionRuleValue::Decimals(values) => {
            let mut values = values
                .iter()
                .map(|value| normalize_decimal(value))
                .collect::<Vec<_>>();
            if should_sort_values(operator) {
                values.sort_by(|left, right| {
                    decimal_sort_key(left).total_cmp(&decimal_sort_key(right))
                });
            }
            if should_dedup_values(operator) {
                values.dedup();
            }
            CollectionRuleValue::Decimals(values)
        }
        CollectionRuleValue::Boolean(value) => {
            CollectionRuleValue::Boolean(*value)
        }
        CollectionRuleValue::Date(value) => {
            CollectionRuleValue::Date(normalize_date(value))
        }
        CollectionRuleValue::Dates(values) => {
            let mut values = values
                .iter()
                .map(|value| normalize_date(value))
                .collect::<Vec<_>>();
            if should_sort_values(operator) {
                values.sort();
            }
            if should_dedup_values(operator) {
                values.dedup();
            }
            CollectionRuleValue::Dates(values)
        }
        CollectionRuleValue::Uuid(value) => CollectionRuleValue::Uuid(*value),
        CollectionRuleValue::Uuids(values) => {
            let mut values = values.clone();
            values.sort_unstable();
            values.dedup();
            CollectionRuleValue::Uuids(values)
        }
        CollectionRuleValue::MediaType(value) => {
            CollectionRuleValue::MediaType(*value)
        }
        CollectionRuleValue::MediaTypes(values) => {
            let mut values = values.clone();
            values.sort_by_key(|value| value.as_slug());
            values.dedup();
            CollectionRuleValue::MediaTypes(values)
        }
        CollectionRuleValue::Availability(value) => {
            CollectionRuleValue::Availability(*value)
        }
        CollectionRuleValue::Person(value) => {
            CollectionRuleValue::Person(normalize_person(value))
        }
        CollectionRuleValue::WatchStatus(value) => {
            CollectionRuleValue::WatchStatus(normalize_watch_status(value))
        }
        CollectionRuleValue::WatchProgress(value) => {
            CollectionRuleValue::WatchProgress(value.clone())
        }
    }
}

fn normalize_person(
    value: &CollectionPersonRuleValue,
) -> CollectionPersonRuleValue {
    CollectionPersonRuleValue {
        role: value.role,
        name: value.name.as_ref().map(|name| normalize_text(name, true)),
        tmdb_id: value.tmdb_id,
    }
}

fn normalize_watch_status(
    value: &CollectionWatchStatusRuleValue,
) -> CollectionWatchStatusRuleValue {
    let mut statuses = value.statuses.clone();
    statuses.sort_unstable();
    statuses.dedup();
    CollectionWatchStatusRuleValue {
        user_id: value.user_id,
        statuses,
    }
}

fn normalize_string_set(
    values: &[String],
    case_insensitive: bool,
    operator: CollectionRuleOperator,
) -> Vec<String> {
    let mut values = values
        .iter()
        .map(|value| normalize_text(value, case_insensitive))
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    if should_sort_values(operator) {
        values.sort();
    }
    if should_dedup_values(operator) {
        values.dedup();
    }
    values
}

fn normalize_text(value: &str, case_insensitive: bool) -> String {
    let collapsed = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if case_insensitive {
        collapsed.to_lowercase()
    } else {
        collapsed
    }
}

fn normalize_decimal(value: &str) -> String {
    let trimmed = value.trim();
    let Ok(parsed) = trimmed.parse::<f64>() else {
        return trimmed.to_string();
    };
    if !parsed.is_finite() {
        return trimmed.to_string();
    }
    let mut normalized = format!("{parsed:.6}");
    while normalized.contains('.') && normalized.ends_with('0') {
        normalized.pop();
    }
    if normalized.ends_with('.') {
        normalized.pop();
    }
    if normalized == "-0" {
        "0".to_string()
    } else {
        normalized
    }
}

fn normalize_date(value: &str) -> String {
    let trimmed = value.trim();
    if let Ok(date) = NaiveDate::parse_from_str(trimmed, "%Y-%m-%d") {
        return date.format("%Y-%m-%d").to_string();
    }
    if let Ok(date_time) = DateTime::parse_from_rfc3339(trimmed) {
        return date_time
            .with_timezone(&Utc)
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    }
    trimmed.to_string()
}

fn should_sort_values(operator: CollectionRuleOperator) -> bool {
    matches!(
        operator,
        CollectionRuleOperator::In
            | CollectionRuleOperator::NotIn
            | CollectionRuleOperator::ContainsAny
            | CollectionRuleOperator::ContainsAll
            | CollectionRuleOperator::Between
    )
}

fn should_dedup_values(operator: CollectionRuleOperator) -> bool {
    matches!(
        operator,
        CollectionRuleOperator::In
            | CollectionRuleOperator::NotIn
            | CollectionRuleOperator::ContainsAny
            | CollectionRuleOperator::ContainsAll
    )
}

fn decimal_sort_key(value: &str) -> f64 {
    value.trim().parse::<f64>().unwrap_or(f64::NAN)
}

#[derive(Default)]
struct RuleValidator {
    errors: Vec<CollectionRuleValidationError>,
    watch_user_ids: BTreeSet<Uuid>,
}

impl RuleValidator {
    fn validate_rule(&mut self, rule: &DynamicCollectionRule) {
        if rule.schema_version != COLLECTION_RULE_SCHEMA_VERSION {
            self.error(
                "schema_version",
                CollectionRuleValidationCode::UnsupportedSchemaVersion,
                format!(
                    "unsupported collection rule schema version {}; expected {}",
                    rule.schema_version, COLLECTION_RULE_SCHEMA_VERSION
                ),
            );
        }

        self.validate_predicate("predicate", &rule.predicate, 0);
        self.validate_sort_policy(&rule.sort);
        self.validate_limit_policy(&rule.limit);

        if self.watch_user_ids.len() > 1 {
            self.error(
                "rule.user_scope",
                CollectionRuleValidationCode::ConflictingUserScopes,
                "watch predicates and watch sort keys must use one explicit user id per rule",
            );
        }
    }

    fn validate_predicate(
        &mut self,
        path: &str,
        predicate: &CollectionRulePredicate,
        depth: usize,
    ) {
        if depth > MAX_COLLECTION_RULE_DEPTH {
            self.error(
                path,
                CollectionRuleValidationCode::TooComplex,
                format!(
                    "predicate nesting exceeds max depth of {}",
                    MAX_COLLECTION_RULE_DEPTH
                ),
            );
            return;
        }

        match predicate {
            CollectionRulePredicate::All { clauses } => {
                self.validate_clauses(path, clauses, depth, true);
            }
            CollectionRulePredicate::Any { clauses } => {
                self.validate_clauses(path, clauses, depth, false);
            }
            CollectionRulePredicate::Not { clause } => {
                self.validate_predicate(
                    &format!("{path}.clause"),
                    clause,
                    depth + 1,
                );
            }
            CollectionRulePredicate::Field {
                field,
                operator,
                value,
            } => self.validate_field(path, *field, *operator, value),
        }
    }

    fn validate_clauses(
        &mut self,
        path: &str,
        clauses: &[CollectionRulePredicate],
        depth: usize,
        allow_empty_all: bool,
    ) {
        if clauses.is_empty() && !allow_empty_all {
            self.error(
                format!("{path}.clauses"),
                CollectionRuleValidationCode::EmptyPredicate,
                "any predicate requires at least one clause",
            );
        }
        if clauses.len() > MAX_COLLECTION_RULE_CLAUSES {
            self.error(
                format!("{path}.clauses"),
                CollectionRuleValidationCode::TooComplex,
                format!(
                    "predicate contains {} clauses; max is {}",
                    clauses.len(),
                    MAX_COLLECTION_RULE_CLAUSES
                ),
            );
        }
        for (index, clause) in clauses.iter().enumerate() {
            self.validate_predicate(
                &format!("{path}.clauses[{index}]"),
                clause,
                depth + 1,
            );
        }
    }

    fn validate_field(
        &mut self,
        path: &str,
        field: CollectionRuleField,
        operator: CollectionRuleOperator,
        value: &CollectionRuleValue,
    ) {
        if matches!(operator, CollectionRuleOperator::Exists) {
            self.validate_exists(path, value);
            return;
        }

        match field {
            CollectionRuleField::MediaType => {
                self.validate_media_type(path, operator, value);
            }
            CollectionRuleField::LibraryId => {
                self.validate_uuid_field(path, operator, value, "library_id");
            }
            CollectionRuleField::Title
            | CollectionRuleField::SortTitle
            | CollectionRuleField::Overview
            | CollectionRuleField::SearchText => {
                self.validate_search_text(path, field, operator, value);
            }
            CollectionRuleField::Genre | CollectionRuleField::Keyword => {
                self.validate_text_set(path, field, operator, value);
            }
            CollectionRuleField::Person => {
                self.validate_person(path, operator, value);
            }
            CollectionRuleField::ReleaseYear => {
                self.validate_integer_field(
                    path, field, operator, value, false,
                );
            }
            CollectionRuleField::ReleaseDate
            | CollectionRuleField::AddedAt
            | CollectionRuleField::DiscoveredAt
            | CollectionRuleField::CreatedAt
            | CollectionRuleField::UpdatedAt => {
                self.validate_date_field(path, field, operator, value);
            }
            CollectionRuleField::RuntimeMinutes
            | CollectionRuleField::FileSizeBytes
            | CollectionRuleField::BitrateKbps
            | CollectionRuleField::ResolutionWidth
            | CollectionRuleField::ResolutionHeight
            | CollectionRuleField::AudioChannelCount => {
                self.validate_integer_field(path, field, operator, value, true);
            }
            CollectionRuleField::AudienceRating
            | CollectionRuleField::CriticRating
            | CollectionRuleField::UserRating
            | CollectionRuleField::Rating
            | CollectionRuleField::Popularity => {
                self.validate_decimal_field(path, field, operator, value, true);
            }
            CollectionRuleField::ContentRating
            | CollectionRuleField::VideoCodec
            | CollectionRuleField::AudioCodec
            | CollectionRuleField::SubtitleLanguage => {
                self.validate_text_set(path, field, operator, value);
            }
            CollectionRuleField::WatchStatus => {
                self.validate_watch_status(path, operator, value);
            }
            CollectionRuleField::WatchProgress => {
                self.validate_watch_progress(path, operator, value);
            }
            CollectionRuleField::Availability => {
                self.validate_availability(path, operator, value);
            }
            CollectionRuleField::TmdbId => {
                self.validate_integer_field(path, field, operator, value, true);
            }
            CollectionRuleField::ActorName
            | CollectionRuleField::DirectorName => {
                self.validate_search_text(path, field, operator, value);
            }
            CollectionRuleField::HasSubtitles => {
                self.validate_boolean_field(
                    path,
                    operator,
                    value,
                    "has_subtitles",
                );
            }
        }
    }

    fn validate_exists(&mut self, path: &str, value: &CollectionRuleValue) {
        if !matches!(value, CollectionRuleValue::Boolean(_)) {
            self.error(
                format!("{path}.value"),
                CollectionRuleValidationCode::InvalidValue,
                "exists predicates must use a boolean value",
            );
        }
    }

    fn validate_media_type(
        &mut self,
        path: &str,
        operator: CollectionRuleOperator,
        value: &CollectionRuleValue,
    ) {
        if !self.require_operator(
            path,
            operator,
            &[
                CollectionRuleOperator::Equals,
                CollectionRuleOperator::NotEquals,
                CollectionRuleOperator::In,
                CollectionRuleOperator::NotIn,
            ],
        ) {
            return;
        }

        let valid = match (operator, value) {
            (
                CollectionRuleOperator::Equals
                | CollectionRuleOperator::NotEquals,
                CollectionRuleValue::MediaType(_),
            ) => true,
            (
                CollectionRuleOperator::In | CollectionRuleOperator::NotIn,
                CollectionRuleValue::MediaTypes(values),
            ) => !values.is_empty(),
            _ => false,
        };
        if !valid {
            self.error(
                format!("{path}.value"),
                CollectionRuleValidationCode::InvalidValue,
                "media_type predicates require media_type for equality or media_types for set membership",
            );
        }
    }

    fn validate_uuid_field(
        &mut self,
        path: &str,
        operator: CollectionRuleOperator,
        value: &CollectionRuleValue,
        label: &str,
    ) {
        if !self.require_operator(
            path,
            operator,
            &[
                CollectionRuleOperator::Equals,
                CollectionRuleOperator::NotEquals,
                CollectionRuleOperator::In,
                CollectionRuleOperator::NotIn,
            ],
        ) {
            return;
        }

        let valid = match (operator, value) {
            (
                CollectionRuleOperator::Equals
                | CollectionRuleOperator::NotEquals,
                CollectionRuleValue::Uuid(_),
            ) => true,
            (
                CollectionRuleOperator::In | CollectionRuleOperator::NotIn,
                CollectionRuleValue::Uuids(values),
            ) => !values.is_empty(),
            _ => false,
        };
        if !valid {
            self.error(
                format!("{path}.value"),
                CollectionRuleValidationCode::InvalidValue,
                format!("{label} predicates require uuid or uuids values"),
            );
        }
    }

    fn validate_search_text(
        &mut self,
        path: &str,
        field: CollectionRuleField,
        operator: CollectionRuleOperator,
        value: &CollectionRuleValue,
    ) {
        if !self.require_operator(
            path,
            operator,
            &[
                CollectionRuleOperator::Equals,
                CollectionRuleOperator::NotEquals,
                CollectionRuleOperator::Contains,
                CollectionRuleOperator::StartsWith,
                CollectionRuleOperator::In,
                CollectionRuleOperator::NotIn,
            ],
        ) {
            return;
        }
        if !string_or_string_set_matches(operator, value) {
            self.error(
                format!("{path}.value"),
                CollectionRuleValidationCode::InvalidValue,
                format!(
                    "{} predicates require a non-empty string or strings value",
                    field_label(field)
                ),
            );
        }
    }

    fn validate_text_set(
        &mut self,
        path: &str,
        field: CollectionRuleField,
        operator: CollectionRuleOperator,
        value: &CollectionRuleValue,
    ) {
        if !self.require_operator(
            path,
            operator,
            &[
                CollectionRuleOperator::Equals,
                CollectionRuleOperator::NotEquals,
                CollectionRuleOperator::Contains,
                CollectionRuleOperator::ContainsAny,
                CollectionRuleOperator::ContainsAll,
                CollectionRuleOperator::In,
                CollectionRuleOperator::NotIn,
            ],
        ) {
            return;
        }
        if !string_or_string_set_matches(operator, value) {
            self.error(
                format!("{path}.value"),
                CollectionRuleValidationCode::InvalidValue,
                format!(
                    "{} predicates require non-empty string values",
                    field_label(field)
                ),
            );
        }
    }

    fn validate_person(
        &mut self,
        path: &str,
        operator: CollectionRuleOperator,
        value: &CollectionRuleValue,
    ) {
        if !self.require_operator(
            path,
            operator,
            &[
                CollectionRuleOperator::Equals,
                CollectionRuleOperator::Contains,
            ],
        ) {
            return;
        }
        let CollectionRuleValue::Person(person) = value else {
            self.error(
                format!("{path}.value"),
                CollectionRuleValidationCode::InvalidValue,
                "person predicates require a person value with a role and name or TMDB id",
            );
            return;
        };
        let has_name = person
            .name
            .as_ref()
            .is_some_and(|name| !normalize_text(name, true).is_empty());
        if !has_name && person.tmdb_id.is_none() {
            self.error(
                format!("{path}.value"),
                CollectionRuleValidationCode::InvalidValue,
                "person predicates require either name or tmdb_id",
            );
        }
    }

    fn validate_integer_field(
        &mut self,
        path: &str,
        field: CollectionRuleField,
        operator: CollectionRuleOperator,
        value: &CollectionRuleValue,
        non_negative: bool,
    ) {
        if !self.require_operator(path, operator, numeric_operators()) {
            return;
        }
        if !integer_value_matches(operator, value, non_negative) {
            self.error(
                format!("{path}.value"),
                CollectionRuleValidationCode::InvalidValue,
                format!(
                    "{} predicates require integer values compatible with {:?}",
                    field_label(field),
                    operator
                ),
            );
        }
    }

    fn validate_decimal_field(
        &mut self,
        path: &str,
        field: CollectionRuleField,
        operator: CollectionRuleOperator,
        value: &CollectionRuleValue,
        non_negative: bool,
    ) {
        if !self.require_operator(path, operator, numeric_operators()) {
            return;
        }
        if !decimal_value_matches(operator, value, non_negative) {
            self.error(
                format!("{path}.value"),
                CollectionRuleValidationCode::InvalidValue,
                format!(
                    "{} predicates require decimal or integer values compatible with {:?}",
                    field_label(field),
                    operator
                ),
            );
        }
    }

    fn validate_date_field(
        &mut self,
        path: &str,
        field: CollectionRuleField,
        operator: CollectionRuleOperator,
        value: &CollectionRuleValue,
    ) {
        if !self.require_operator(path, operator, date_operators()) {
            return;
        }
        if !date_value_matches(operator, value) {
            self.error(
                format!("{path}.value"),
                CollectionRuleValidationCode::InvalidValue,
                format!(
                    "{} predicates require ISO date strings compatible with {:?}",
                    field_label(field),
                    operator
                ),
            );
        }
    }

    fn validate_watch_status(
        &mut self,
        path: &str,
        operator: CollectionRuleOperator,
        value: &CollectionRuleValue,
    ) {
        if !self.require_operator(
            path,
            operator,
            &[
                CollectionRuleOperator::Equals,
                CollectionRuleOperator::NotEquals,
                CollectionRuleOperator::In,
                CollectionRuleOperator::NotIn,
            ],
        ) {
            return;
        }
        let CollectionRuleValue::WatchStatus(watch_status) = value else {
            self.error(
                format!("{path}.value"),
                CollectionRuleValidationCode::MissingUserScope,
                "watch_status predicates must use a watch_status value with explicit user_id",
            );
            return;
        };
        if watch_status.statuses.is_empty() {
            self.error(
                format!("{path}.value.statuses"),
                CollectionRuleValidationCode::InvalidValue,
                "watch_status predicates require at least one status",
            );
        }
        self.watch_user_ids.insert(watch_status.user_id);
    }

    fn validate_watch_progress(
        &mut self,
        path: &str,
        operator: CollectionRuleOperator,
        value: &CollectionRuleValue,
    ) {
        if !self.require_operator(path, operator, numeric_operators()) {
            return;
        }
        let CollectionRuleValue::WatchProgress(progress) = value else {
            self.error(
                format!("{path}.value"),
                CollectionRuleValidationCode::MissingUserScope,
                "watch_progress predicates must use a watch_progress value with explicit user_id",
            );
            return;
        };
        if let (Some(min), Some(max)) =
            (progress.min_percent, progress.max_percent)
        {
            if min > max {
                self.error(
                    format!("{path}.value"),
                    CollectionRuleValidationCode::InvalidValue,
                    "watch_progress min_percent must be less than or equal to max_percent",
                );
            }
        }
        if progress
            .min_percent
            .into_iter()
            .chain(progress.max_percent)
            .any(|percent| percent > 100)
        {
            self.error(
                format!("{path}.value"),
                CollectionRuleValidationCode::InvalidValue,
                "watch_progress percentages must be between 0 and 100",
            );
        }
        self.watch_user_ids.insert(progress.user_id);
    }

    fn validate_availability(
        &mut self,
        path: &str,
        operator: CollectionRuleOperator,
        value: &CollectionRuleValue,
    ) {
        if !self.require_operator(
            path,
            operator,
            &[
                CollectionRuleOperator::Equals,
                CollectionRuleOperator::NotEquals,
            ],
        ) {
            return;
        }
        if !matches!(value, CollectionRuleValue::Availability(_)) {
            self.error(
                format!("{path}.value"),
                CollectionRuleValidationCode::InvalidValue,
                "availability predicates require an availability value",
            );
        }
    }

    fn validate_boolean_field(
        &mut self,
        path: &str,
        operator: CollectionRuleOperator,
        value: &CollectionRuleValue,
        label: &str,
    ) {
        if !self.require_operator(
            path,
            operator,
            &[
                CollectionRuleOperator::Equals,
                CollectionRuleOperator::NotEquals,
            ],
        ) {
            return;
        }
        if !matches!(value, CollectionRuleValue::Boolean(_)) {
            self.error(
                format!("{path}.value"),
                CollectionRuleValidationCode::InvalidValue,
                format!("{label} predicates require a boolean value"),
            );
        }
    }

    fn validate_sort_policy(&mut self, sort: &CollectionSortPolicy) {
        if sort.schema_version != COLLECTION_SORT_SCHEMA_VERSION {
            self.error(
                "sort.schema_version",
                CollectionRuleValidationCode::UnsupportedSchemaVersion,
                format!(
                    "unsupported collection sort schema version {}; expected {}",
                    sort.schema_version, COLLECTION_SORT_SCHEMA_VERSION
                ),
            );
        }
        if sort.keys.len() > 4 {
            self.error(
                "sort.keys",
                CollectionRuleValidationCode::TooComplex,
                "sort policy supports at most four explicit keys plus the stable tie-breaker",
            );
        }

        let mut seen = BTreeSet::new();
        for (index, key) in sort.keys.iter().enumerate() {
            let path = format!("sort.keys[{index}]");
            let seen_key = format!("{:?}:{:?}", key.field, key.user_id);
            if !seen.insert(seen_key) {
                self.error(
                    format!("{path}.field"),
                    CollectionRuleValidationCode::NonDeterministicSort,
                    "duplicate sort fields are not supported",
                );
            }

            match key.field {
                CollectionSortField::ManualPosition => self.error(
                    format!("{path}.field"),
                    CollectionRuleValidationCode::UnsupportedField,
                    "manual_position sorting is only valid for manual collections, not dynamic rule evaluation",
                ),
                CollectionSortField::RandomStable => self.error(
                    format!("{path}.field"),
                    CollectionRuleValidationCode::UnsupportedField,
                    "random_stable sorting is deferred until the evaluator has a stable seed contract",
                ),
                CollectionSortField::LastWatchedAt | CollectionSortField::WatchProgress => {
                    let Some(user_id) = key.user_id else {
                        self.error(
                            format!("{path}.user_id"),
                            CollectionRuleValidationCode::MissingUserScope,
                            "watch sort keys require an explicit user_id",
                        );
                        continue;
                    };
                    self.watch_user_ids.insert(user_id);
                }
                CollectionSortField::RecentlyAdded | CollectionSortField::RecentlyReleased => {
                    if !matches!(key.direction, CollectionSortDirection::Desc) {
                        self.error(
                            format!("{path}.direction"),
                            CollectionRuleValidationCode::InvalidValue,
                            "recently-added and recently-released sorts must use descending direction",
                        );
                    }
                }
                _ => {}
            }
        }

        if matches!(
            sort.tie_breaker,
            CollectionSortTieBreaker::ManualPositionThenStableKey
        ) {
            self.error(
                "sort.tie_breaker",
                CollectionRuleValidationCode::NonDeterministicSort,
                "dynamic rules must tie-break by stable media key or title then stable media key",
            );
        }
    }

    fn validate_limit_policy(&mut self, limit: &CollectionLimitPolicy) {
        if limit.schema_version != COLLECTION_LIMIT_SCHEMA_VERSION {
            self.error(
                "limit.schema_version",
                CollectionRuleValidationCode::UnsupportedSchemaVersion,
                format!(
                    "unsupported collection limit schema version {}; expected {}",
                    limit.schema_version, COLLECTION_LIMIT_SCHEMA_VERSION
                ),
            );
        }

        for (path, value) in [
            ("limit.max_items", limit.max_items),
            ("limit.per_media_type", limit.per_media_type),
        ] {
            if let Some(value) = value {
                if value == 0 {
                    self.error(
                        path,
                        CollectionRuleValidationCode::InvalidLimit,
                        "finite limits must be greater than zero",
                    );
                }
                if value > MAX_COLLECTION_LIMIT_ITEMS {
                    self.error(
                        path,
                        CollectionRuleValidationCode::InvalidLimit,
                        format!(
                            "finite limits must not exceed {} items",
                            MAX_COLLECTION_LIMIT_ITEMS
                        ),
                    );
                }
            }
        }

        if limit.max_items.is_none()
            && limit.per_media_type.is_none()
            && !matches!(limit.window, CollectionLimitWindow::All)
        {
            self.error(
                "limit.window",
                CollectionRuleValidationCode::InvalidLimit,
                "windowed limits require max_items or per_media_type; use window=all for unlimited mode",
            );
        }
    }

    fn require_operator(
        &mut self,
        path: &str,
        operator: CollectionRuleOperator,
        allowed: &[CollectionRuleOperator],
    ) -> bool {
        if allowed.contains(&operator) {
            return true;
        }
        self.error(
            format!("{path}.operator"),
            CollectionRuleValidationCode::UnsupportedOperator,
            format!(
                "operator {:?} is not supported for this field; expected one of {}",
                operator,
                join_operator_names(allowed)
            ),
        );
        false
    }

    fn error(
        &mut self,
        path: impl Into<String>,
        code: CollectionRuleValidationCode,
        message: impl Into<String>,
    ) {
        self.errors.push(CollectionRuleValidationError {
            path: path.into(),
            code,
            message: message.into(),
        });
    }
}

fn numeric_operators() -> &'static [CollectionRuleOperator] {
    &[
        CollectionRuleOperator::Equals,
        CollectionRuleOperator::NotEquals,
        CollectionRuleOperator::In,
        CollectionRuleOperator::NotIn,
        CollectionRuleOperator::GreaterThan,
        CollectionRuleOperator::GreaterThanOrEqual,
        CollectionRuleOperator::LessThan,
        CollectionRuleOperator::LessThanOrEqual,
        CollectionRuleOperator::Between,
    ]
}

fn date_operators() -> &'static [CollectionRuleOperator] {
    &[
        CollectionRuleOperator::Equals,
        CollectionRuleOperator::NotEquals,
        CollectionRuleOperator::GreaterThan,
        CollectionRuleOperator::GreaterThanOrEqual,
        CollectionRuleOperator::LessThan,
        CollectionRuleOperator::LessThanOrEqual,
        CollectionRuleOperator::Between,
    ]
}

fn string_or_string_set_matches(
    operator: CollectionRuleOperator,
    value: &CollectionRuleValue,
) -> bool {
    match (operator, value) {
        (
            CollectionRuleOperator::Equals
            | CollectionRuleOperator::NotEquals
            | CollectionRuleOperator::Contains
            | CollectionRuleOperator::StartsWith,
            CollectionRuleValue::String(value),
        ) => !normalize_text(value, true).is_empty(),
        (
            CollectionRuleOperator::In
            | CollectionRuleOperator::NotIn
            | CollectionRuleOperator::ContainsAny
            | CollectionRuleOperator::ContainsAll,
            CollectionRuleValue::Strings(values),
        ) => values
            .iter()
            .any(|value| !normalize_text(value, true).is_empty()),
        _ => false,
    }
}

fn integer_value_matches(
    operator: CollectionRuleOperator,
    value: &CollectionRuleValue,
    non_negative: bool,
) -> bool {
    let integer_ok = |value: i64| !non_negative || value >= 0;
    match (operator, value) {
        (
            CollectionRuleOperator::Equals
            | CollectionRuleOperator::NotEquals
            | CollectionRuleOperator::GreaterThan
            | CollectionRuleOperator::GreaterThanOrEqual
            | CollectionRuleOperator::LessThan
            | CollectionRuleOperator::LessThanOrEqual,
            CollectionRuleValue::Integer(value),
        ) => integer_ok(*value),
        (
            CollectionRuleOperator::In | CollectionRuleOperator::NotIn,
            CollectionRuleValue::Integers(values),
        ) => !values.is_empty() && values.iter().copied().all(integer_ok),
        (
            CollectionRuleOperator::Between,
            CollectionRuleValue::Integers(values),
        ) => {
            values.len() == 2
                && values.iter().copied().all(integer_ok)
                && values[0] <= values[1]
        }
        _ => false,
    }
}

fn decimal_value_matches(
    operator: CollectionRuleOperator,
    value: &CollectionRuleValue,
    non_negative: bool,
) -> bool {
    let decimal_ok = |value: &str| {
        value.trim().parse::<f64>().is_ok_and(|value| {
            value.is_finite() && (!non_negative || value >= 0.0)
        })
    };
    match (operator, value) {
        (
            CollectionRuleOperator::Equals
            | CollectionRuleOperator::NotEquals
            | CollectionRuleOperator::GreaterThan
            | CollectionRuleOperator::GreaterThanOrEqual
            | CollectionRuleOperator::LessThan
            | CollectionRuleOperator::LessThanOrEqual,
            CollectionRuleValue::Decimal(value),
        ) => decimal_ok(value),
        (
            CollectionRuleOperator::Equals
            | CollectionRuleOperator::NotEquals
            | CollectionRuleOperator::GreaterThan
            | CollectionRuleOperator::GreaterThanOrEqual
            | CollectionRuleOperator::LessThan
            | CollectionRuleOperator::LessThanOrEqual,
            CollectionRuleValue::Integer(value),
        ) => !non_negative || *value >= 0,
        (
            CollectionRuleOperator::In | CollectionRuleOperator::NotIn,
            CollectionRuleValue::Decimals(values),
        ) => !values.is_empty() && values.iter().all(|value| decimal_ok(value)),
        (
            CollectionRuleOperator::Between,
            CollectionRuleValue::Decimals(values),
        ) => {
            values.len() == 2
                && values.iter().all(|value| decimal_ok(value))
                && values[0].trim().parse::<f64>().ok()
                    <= values[1].trim().parse::<f64>().ok()
        }
        _ => false,
    }
}

fn date_value_matches(
    operator: CollectionRuleOperator,
    value: &CollectionRuleValue,
) -> bool {
    match (operator, value) {
        (
            CollectionRuleOperator::Equals
            | CollectionRuleOperator::NotEquals
            | CollectionRuleOperator::GreaterThan
            | CollectionRuleOperator::GreaterThanOrEqual
            | CollectionRuleOperator::LessThan
            | CollectionRuleOperator::LessThanOrEqual,
            CollectionRuleValue::Date(value),
        ) => is_valid_date(value),
        (
            CollectionRuleOperator::Between,
            CollectionRuleValue::Dates(values),
        ) => {
            values.len() == 2 && values.iter().all(|value| is_valid_date(value))
        }
        _ => false,
    }
}

fn is_valid_date(value: &str) -> bool {
    NaiveDate::parse_from_str(value.trim(), "%Y-%m-%d").is_ok()
        || DateTime::parse_from_rfc3339(value.trim()).is_ok()
}

fn collect_predicate_watch_user_ids(
    predicate: &CollectionRulePredicate,
    ids: &mut BTreeSet<Uuid>,
) {
    match predicate {
        CollectionRulePredicate::All { clauses }
        | CollectionRulePredicate::Any { clauses } => {
            for clause in clauses {
                collect_predicate_watch_user_ids(clause, ids);
            }
        }
        CollectionRulePredicate::Not { clause } => {
            collect_predicate_watch_user_ids(clause, ids)
        }
        CollectionRulePredicate::Field { value, .. } => match value {
            CollectionRuleValue::WatchStatus(value) => {
                ids.insert(value.user_id);
            }
            CollectionRuleValue::WatchProgress(value) => {
                ids.insert(value.user_id);
            }
            _ => {}
        },
    }
}

fn predicate_summary(predicate: &CollectionRulePredicate) -> String {
    match predicate {
        CollectionRulePredicate::All { clauses } if clauses.is_empty() => {
            "all media".to_string()
        }
        CollectionRulePredicate::All { clauses } => format!(
            "all of ({})",
            clauses
                .iter()
                .map(predicate_summary)
                .collect::<Vec<_>>()
                .join("; ")
        ),
        CollectionRulePredicate::Any { clauses } => format!(
            "any of ({})",
            clauses
                .iter()
                .map(predicate_summary)
                .collect::<Vec<_>>()
                .join("; ")
        ),
        CollectionRulePredicate::Not { clause } => {
            format!("not ({})", predicate_summary(clause))
        }
        CollectionRulePredicate::Field {
            field,
            operator,
            value,
        } => format!(
            "{} {} {}",
            field_label(*field),
            operator_label(*operator),
            value_summary(value)
        ),
    }
}

fn sort_summary(sort: &CollectionSortPolicy) -> String {
    if sort.keys.is_empty() {
        return format!("sort by {}", tie_breaker_label(sort.tie_breaker));
    }
    let keys = sort
        .keys
        .iter()
        .map(|key| {
            let mut item = format!(
                "{} {} nulls {}",
                sort_field_label(key.field),
                sort_direction_label(key.direction),
                sort_nulls_label(key.nulls)
            );
            if let Some(user_id) = key.user_id {
                let _ = write!(item, " for user {user_id}");
            }
            item
        })
        .collect::<Vec<_>>()
        .join(", then ");
    format!(
        "sort by {keys}; tie-break by {}",
        tie_breaker_label(sort.tie_breaker)
    )
}

fn limit_summary(limit: &CollectionLimitPolicy) -> String {
    match (limit.max_items, limit.per_media_type, limit.window) {
        (None, None, CollectionLimitWindow::All) => "unlimited".to_string(),
        (max_items, per_media_type, window) => {
            let mut parts = Vec::new();
            if let Some(max_items) = max_items {
                parts.push(format!("limit to {max_items} items"));
            }
            if let Some(per_media_type) = per_media_type {
                parts.push(format!("limit to {per_media_type} per media type"));
            }
            if !matches!(window, CollectionLimitWindow::All) {
                parts.push(format!("window {}", limit_window_label(window)));
            }
            parts.join(", ")
        }
    }
}

fn value_summary(value: &CollectionRuleValue) -> String {
    match value {
        CollectionRuleValue::String(value) => quote(value),
        CollectionRuleValue::Strings(values) => values
            .iter()
            .map(|value| quote(value))
            .collect::<Vec<_>>()
            .join(", "),
        CollectionRuleValue::Integer(value) => value.to_string(),
        CollectionRuleValue::Integers(values) => values
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(", "),
        CollectionRuleValue::Decimal(value) => value.clone(),
        CollectionRuleValue::Decimals(values) => values.join(", "),
        CollectionRuleValue::Boolean(value) => value.to_string(),
        CollectionRuleValue::Date(value) => value.clone(),
        CollectionRuleValue::Dates(values) => values.join(" to "),
        CollectionRuleValue::Uuid(value) => value.to_string(),
        CollectionRuleValue::Uuids(values) => values
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(", "),
        CollectionRuleValue::MediaType(value) => value.as_slug().to_string(),
        CollectionRuleValue::MediaTypes(values) => values
            .iter()
            .map(|value| value.as_slug())
            .collect::<Vec<_>>()
            .join(", "),
        CollectionRuleValue::Availability(value) => {
            format!("{:?}", value).to_lowercase()
        }
        CollectionRuleValue::Person(value) => {
            let subject = value
                .name
                .as_deref()
                .map(quote)
                .or_else(|| value.tmdb_id.map(|id| format!("tmdb:{id}")))
                .unwrap_or_else(|| "unknown person".to_string());
            format!("{} {subject}", person_role_label(value.role))
        }
        CollectionRuleValue::WatchStatus(value) => format!(
            "{} for user {}",
            value
                .statuses
                .iter()
                .map(|status| watch_status_label(*status))
                .collect::<Vec<_>>()
                .join(", "),
            value.user_id
        ),
        CollectionRuleValue::WatchProgress(value) => {
            match (value.min_percent, value.max_percent) {
                (Some(min), Some(max)) => {
                    format!("{min}% to {max}% for user {}", value.user_id)
                }
                (Some(min), None) => {
                    format!(">= {min}% for user {}", value.user_id)
                }
                (None, Some(max)) => {
                    format!("<= {max}% for user {}", value.user_id)
                }
                (None, None) => {
                    format!("any progress for user {}", value.user_id)
                }
            }
        }
    }
}

fn quote(value: &str) -> String {
    format!("\"{value}\"")
}

fn field_uses_case_insensitive_text(field: CollectionRuleField) -> bool {
    matches!(
        field,
        CollectionRuleField::Title
            | CollectionRuleField::SortTitle
            | CollectionRuleField::Overview
            | CollectionRuleField::SearchText
            | CollectionRuleField::Genre
            | CollectionRuleField::Keyword
            | CollectionRuleField::Person
            | CollectionRuleField::ContentRating
            | CollectionRuleField::ActorName
            | CollectionRuleField::DirectorName
            | CollectionRuleField::VideoCodec
            | CollectionRuleField::AudioCodec
            | CollectionRuleField::SubtitleLanguage
    )
}

fn is_user_scoped_sort_field(field: CollectionSortField) -> bool {
    matches!(
        field,
        CollectionSortField::LastWatchedAt | CollectionSortField::WatchProgress
    )
}

fn join_operator_names(operators: &[CollectionRuleOperator]) -> String {
    operators
        .iter()
        .map(|operator| operator_label(*operator))
        .collect::<Vec<_>>()
        .join(", ")
}

fn field_label(field: CollectionRuleField) -> &'static str {
    match field {
        CollectionRuleField::MediaType => "media type",
        CollectionRuleField::LibraryId => "library id",
        CollectionRuleField::Title => "title",
        CollectionRuleField::SortTitle => "sort title",
        CollectionRuleField::Overview => "overview",
        CollectionRuleField::SearchText => "search text",
        CollectionRuleField::Genre => "genre",
        CollectionRuleField::Keyword => "keyword",
        CollectionRuleField::Person => "person",
        CollectionRuleField::ReleaseYear => "release year",
        CollectionRuleField::ReleaseDate => "release date",
        CollectionRuleField::AddedAt => "date added",
        CollectionRuleField::DiscoveredAt => "date discovered",
        CollectionRuleField::CreatedAt => "date created",
        CollectionRuleField::UpdatedAt => "date updated",
        CollectionRuleField::RuntimeMinutes => "runtime minutes",
        CollectionRuleField::AudienceRating => "audience rating",
        CollectionRuleField::CriticRating => "critic rating",
        CollectionRuleField::UserRating => "user rating",
        CollectionRuleField::Rating => "rating",
        CollectionRuleField::Popularity => "popularity",
        CollectionRuleField::ContentRating => "content rating",
        CollectionRuleField::WatchStatus => "watch status",
        CollectionRuleField::WatchProgress => "watch progress",
        CollectionRuleField::Availability => "availability",
        CollectionRuleField::TmdbId => "TMDB id",
        CollectionRuleField::ActorName => "actor name",
        CollectionRuleField::DirectorName => "director name",
        CollectionRuleField::FileSizeBytes => "file size",
        CollectionRuleField::BitrateKbps => "bitrate",
        CollectionRuleField::ResolutionWidth => "resolution width",
        CollectionRuleField::ResolutionHeight => "resolution height",
        CollectionRuleField::VideoCodec => "video codec",
        CollectionRuleField::AudioCodec => "audio codec",
        CollectionRuleField::AudioChannelCount => "audio channel count",
        CollectionRuleField::SubtitleLanguage => "subtitle language",
        CollectionRuleField::HasSubtitles => "has subtitles",
    }
}

fn operator_label(operator: CollectionRuleOperator) -> &'static str {
    match operator {
        CollectionRuleOperator::Equals => "is",
        CollectionRuleOperator::NotEquals => "is not",
        CollectionRuleOperator::Contains => "contains",
        CollectionRuleOperator::StartsWith => "starts with",
        CollectionRuleOperator::In => "is in",
        CollectionRuleOperator::NotIn => "is not in",
        CollectionRuleOperator::ContainsAny => "contains any of",
        CollectionRuleOperator::ContainsAll => "contains all of",
        CollectionRuleOperator::GreaterThan => "is greater than",
        CollectionRuleOperator::GreaterThanOrEqual => "is at least",
        CollectionRuleOperator::LessThan => "is less than",
        CollectionRuleOperator::LessThanOrEqual => "is at most",
        CollectionRuleOperator::Between => "is between",
        CollectionRuleOperator::Exists => "exists",
    }
}

fn sort_field_label(field: CollectionSortField) -> &'static str {
    match field {
        CollectionSortField::RecentlyAdded => "recently added",
        CollectionSortField::RecentlyReleased => "recently released",
        CollectionSortField::Title => "title",
        CollectionSortField::SortTitle => "sort title",
        CollectionSortField::ReleaseDate => "release date",
        CollectionSortField::AddedAt => "date added",
        CollectionSortField::DiscoveredAt => "date discovered",
        CollectionSortField::CreatedAt => "date created",
        CollectionSortField::UpdatedAt => "date updated",
        CollectionSortField::RuntimeMinutes => "runtime",
        CollectionSortField::AudienceRating => "audience rating",
        CollectionSortField::CriticRating => "critic rating",
        CollectionSortField::UserRating => "user rating",
        CollectionSortField::Rating => "rating",
        CollectionSortField::Popularity => "popularity",
        CollectionSortField::FileSizeBytes => "file size",
        CollectionSortField::BitrateKbps => "bitrate",
        CollectionSortField::ResolutionWidth => "resolution width",
        CollectionSortField::ResolutionHeight => "resolution height",
        CollectionSortField::LastWatchedAt => "last watched",
        CollectionSortField::WatchProgress => "watch progress",
        CollectionSortField::ManualPosition => "manual position",
        CollectionSortField::RandomStable => "stable random",
    }
}

fn sort_direction_label(direction: CollectionSortDirection) -> &'static str {
    match direction {
        CollectionSortDirection::Asc => "ascending",
        CollectionSortDirection::Desc => "descending",
    }
}

fn sort_nulls_label(nulls: CollectionSortNulls) -> &'static str {
    match nulls {
        CollectionSortNulls::First => "first",
        CollectionSortNulls::Last => "last",
    }
}

fn tie_breaker_label(tie_breaker: CollectionSortTieBreaker) -> &'static str {
    match tie_breaker {
        CollectionSortTieBreaker::StableMediaKey => "stable media key",
        CollectionSortTieBreaker::TitleThenStableKey => {
            "title then stable media key"
        }
        CollectionSortTieBreaker::ManualPositionThenStableKey => {
            "manual position then stable media key"
        }
    }
}

fn limit_window_label(window: CollectionLimitWindow) -> &'static str {
    match window {
        CollectionLimitWindow::All => "all",
        CollectionLimitWindow::Newest => "newest",
        CollectionLimitWindow::Oldest => "oldest",
        CollectionLimitWindow::RecentlyAdded => "recently added",
        CollectionLimitWindow::RecentlyReleased => "recently released",
        CollectionLimitWindow::RecentlyUpdated => "recently updated",
    }
}

fn person_role_label(role: CollectionPersonRole) -> &'static str {
    match role {
        CollectionPersonRole::Actor => "actor",
        CollectionPersonRole::Director => "director",
        CollectionPersonRole::Writer => "writer",
        CollectionPersonRole::Producer => "producer",
        CollectionPersonRole::Creator => "creator",
        CollectionPersonRole::Crew => "crew",
        CollectionPersonRole::Any => "person",
    }
}

fn watch_status_label(status: CollectionWatchStatus) -> &'static str {
    match status {
        CollectionWatchStatus::Unwatched => "unwatched",
        CollectionWatchStatus::InProgress => "in progress",
        CollectionWatchStatus::Watched => "watched",
        CollectionWatchStatus::Completed => "completed",
        CollectionWatchStatus::Abandoned => "abandoned",
    }
}
