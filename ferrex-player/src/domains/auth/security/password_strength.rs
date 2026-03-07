#[derive(Debug, Clone, Default)]
pub struct PasswordStrength {
    pub score: u8,
    pub warning: Option<String>,
    pub suggestions: Vec<String>,
}

pub fn evaluate(password: &str) -> PasswordStrength {
    if password.is_empty() {
        return PasswordStrength {
            score: 0,
            warning: Some("Password is empty".to_string()),
            suggestions: vec![
                "Use a password with a mix of words, numbers, and symbols"
                    .to_string(),
            ],
        };
    }

    match zxcvbn::zxcvbn(password, &[]) {
        Ok(result) => PasswordStrength {
            score: result.score() as u8,
            warning: result
                .feedback()
                .and_then(|feedback| feedback.warning().map(|w| w.to_string()))
                .filter(|w| !w.is_empty()),
            suggestions: result
                .feedback()
                .map(|feedback| {
                    feedback
                        .suggestions()
                        .iter()
                        .filter_map(|s| {
                            let trimmed = s.trim();
                            if trimmed.is_empty() {
                                None
                            } else {
                                Some(trimmed.to_string())
                            }
                        })
                        .collect()
                })
                .unwrap_or_default(),
        },
        Err(_) => PasswordStrength {
            score: 0,
            warning: Some("Unable to evaluate password strength".to_string()),
            suggestions: vec![],
        },
    }
}
