//! Test fixtures and data generators
//!
//! Provides reusable test data and scenario generators.

use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

/// Test data generator with deterministic randomization
pub struct FixtureGenerator {
    rng: StdRng,
    counters: HashMap<String, usize>,
}

impl FixtureGenerator {
    /// Create a new fixture generator with a seed
    pub fn new(seed: u64) -> Self {
        Self {
            rng: StdRng::seed_from_u64(seed),
            counters: HashMap::new(),
        }
    }

    /// Create a fixture generator with a random seed
    pub fn random() -> Self {
        Self::new(rand::random())
    }

    /// Generate a unique ID
    pub fn unique_id(&mut self) -> Uuid {
        Uuid::now_v7()
    }

    /// Generate a unique string with a prefix
    pub fn unique_string(&mut self, prefix: &str) -> String {
        let counter = self.counters.entry(prefix.to_string()).or_insert(0);
        *counter += 1;
        format!("{}_{}", prefix, counter)
    }

    /// Generate a random string of given length
    pub fn random_string(&mut self, length: usize) -> String {
        (0..length).map(|_| self.rng.r#gen::<char>()).collect()
    }

    /// Generate a random number in range
    pub fn random_in_range(&mut self, min: i32, max: i32) -> i32 {
        self.rng.gen_range(min..=max)
    }

    /// Generate a random boolean
    pub fn random_bool(&mut self) -> bool {
        self.rng.gen_bool(0.5)
    }

    /// Generate a random email
    pub fn random_email(&mut self) -> String {
        format!("{}@example.com", self.unique_string("user"))
    }

    /// Generate a random timestamp
    pub fn random_timestamp(&mut self) -> chrono::DateTime<chrono::Utc> {
        use chrono::Utc;
        let days_ago = self.rng.gen_range(0..365);
        Utc::now() - chrono::Duration::days(days_ago)
    }

    /// Pick a random item from a slice
    pub fn pick<'a, T>(&mut self, items: &'a [T]) -> Option<&'a T> {
        if items.is_empty() {
            None
        } else {
            let index = self.rng.gen_range(0..items.len());
            Some(&items[index])
        }
    }

    /// Generate multiple items
    pub fn generate_many<F, T>(&mut self, count: usize, generator: F) -> Vec<T>
    where
        F: FnMut(&mut Self) -> T,
    {
        let mut generator = generator;
        (0..count).map(|_| generator(self)).collect()
    }
}

/// Base trait for test data
pub trait TestData: Clone + Send + Sync {
    /// Create a minimal valid instance
    fn minimal() -> Self;

    /// Create a typical instance
    fn typical() -> Self;

    /// Create a complex instance with all fields populated
    fn complex() -> Self;

    /// Create an invalid instance for error testing
    fn invalid() -> Self;
}

/// Scenario generator for complex test setups
pub struct Scenario<T> {
    name: String,
    description: String,
    setup: Box<dyn Fn() -> T + Send + Sync>,
}

impl<T> Scenario<T> {
    /// Create a new scenario
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        setup: impl Fn() -> T + Send + Sync + 'static,
    ) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            setup: Box::new(setup),
        }
    }

    /// Get the scenario name
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get the scenario description
    pub fn description(&self) -> &str {
        &self.description
    }

    /// Execute the scenario setup
    pub fn setup(&self) -> T {
        (self.setup)()
    }
}

/// Collection of related scenarios
pub struct ScenarioCollection<T> {
    scenarios: Vec<Scenario<T>>,
}

impl<T> ScenarioCollection<T> {
    /// Create a new scenario collection
    pub fn new() -> Self {
        Self {
            scenarios: Vec::new(),
        }
    }

    /// Add a scenario to the collection
    pub fn add(mut self, scenario: Scenario<T>) -> Self {
        self.scenarios.push(scenario);
        self
    }

    /// Get all scenarios
    pub fn all(&self) -> &[Scenario<T>] {
        &self.scenarios
    }

    /// Get a scenario by name
    pub fn get(&self, name: &str) -> Option<&Scenario<T>> {
        self.scenarios.iter().find(|s| s.name() == name)
    }

    /// Execute all scenarios
    pub fn execute_all(&self) -> Vec<(String, T)> {
        self.scenarios
            .iter()
            .map(|s| (s.name().to_string(), s.setup()))
            .collect()
    }
}

impl<T> Default for ScenarioCollection<T> {
    fn default() -> Self {
        Self::new()
    }
}

/// Factory for creating related test objects
pub struct TestFactory<T> {
    templates: HashMap<String, Arc<dyn Fn() -> T + Send + Sync>>,
}

impl<T> TestFactory<T> {
    /// Create a new test factory
    pub fn new() -> Self {
        Self {
            templates: HashMap::new(),
        }
    }

    /// Register a template
    pub fn register(
        mut self,
        name: impl Into<String>,
        template: impl Fn() -> T + Send + Sync + 'static,
    ) -> Self {
        self.templates.insert(name.into(), Arc::new(template));
        self
    }

    /// Create an object from a template
    pub fn create(&self, template: &str) -> Option<T> {
        self.templates.get(template).map(|f| f())
    }

    /// Create multiple objects from a template
    pub fn create_many(&self, template: &str, count: usize) -> Vec<T> {
        if let Some(f) = self.templates.get(template) {
            (0..count).map(|_| f()).collect()
        } else {
            Vec::new()
        }
    }
}

impl<T> Default for TestFactory<T> {
    fn default() -> Self {
        Self::new()
    }
}

/// Example fixtures for common domain objects
pub mod examples {
    use super::*;

    #[derive(Clone, Debug)]
    pub struct UserFixture {
        pub id: Uuid,
        pub name: String,
        pub email: String,
        pub is_admin: bool,
        pub created_at: chrono::DateTime<chrono::Utc>,
    }

    impl TestData for UserFixture {
        fn minimal() -> Self {
            Self {
                id: Uuid::nil(),
                name: "User".to_string(),
                email: "user@example.com".to_string(),
                is_admin: false,
                created_at: chrono::Utc::now(),
            }
        }

        fn typical() -> Self {
            Self {
                id: Uuid::now_v7(),
                name: "John Doe".to_string(),
                email: "john.doe@example.com".to_string(),
                is_admin: false,
                created_at: chrono::Utc::now(),
            }
        }

        fn complex() -> Self {
            Self {
                id: Uuid::now_v7(),
                name: "Admin User With Long Name".to_string(),
                email: "admin.user.with.long.email@subdomain.example.com"
                    .to_string(),
                is_admin: true,
                created_at: chrono::Utc::now() - chrono::Duration::days(365),
            }
        }

        fn invalid() -> Self {
            Self {
                id: Uuid::nil(),
                name: String::new(), // Invalid: empty name
                email: "not-an-email".to_string(), // Invalid: bad email format
                is_admin: false,
                created_at: chrono::Utc::now(),
            }
        }
    }

    /// Create common user scenarios
    pub fn user_scenarios() -> ScenarioCollection<Vec<UserFixture>> {
        ScenarioCollection::new()
            .add(Scenario::new(
                "empty_system",
                "No users in the system",
                std::vec::Vec::new,
            ))
            .add(Scenario::new("single_admin", "Single admin user", || {
                vec![UserFixture::typical().with_admin(true)]
            }))
            .add(Scenario::new(
                "mixed_users",
                "Mix of admin and regular users",
                || {
                    vec![
                        UserFixture::typical().with_admin(true),
                        UserFixture::typical().with_name("Alice"),
                        UserFixture::typical().with_name("Bob"),
                    ]
                },
            ))
            .add(Scenario::new(
                "large_user_base",
                "Many users for performance testing",
                || {
                    (0..100)
                        .map(|i| {
                            UserFixture::typical()
                                .with_name(&format!("User{}", i))
                        })
                        .collect()
                },
            ))
    }

    impl UserFixture {
        pub fn with_admin(mut self, is_admin: bool) -> Self {
            self.is_admin = is_admin;
            self
        }

        pub fn with_name(mut self, name: &str) -> Self {
            self.name = name.to_string();
            self.email = format!(
                "{}@example.com",
                name.to_lowercase().replace(' ', ".")
            );
            self
        }
    }
}

#[cfg(test)]
mod tests {
    use super::examples::*;
    use super::*;

    #[test]
    fn test_fixture_generator() {
        let mut r#gen = FixtureGenerator::new(42);

        let id1 = r#gen.unique_id();
        let id2 = r#gen.unique_id();
        assert_ne!(id1, id2);

        let str1 = r#gen.unique_string("test");
        let str2 = r#gen.unique_string("test");
        assert_eq!(str1, "test_1");
        assert_eq!(str2, "test_2");

        let email = r#gen.random_email();
        assert!(email.contains("@example.com"));
    }

    #[test]
    fn test_test_data_trait() {
        let minimal = UserFixture::minimal();
        assert_eq!(minimal.name, "User");

        let typical = UserFixture::typical();
        assert!(!typical.name.is_empty());

        let complex = UserFixture::complex();
        assert!(complex.is_admin);

        let invalid = UserFixture::invalid();
        assert!(invalid.name.is_empty());
    }

    #[test]
    fn test_scenarios() {
        let scenarios = user_scenarios();

        let empty = scenarios.get("empty_system").unwrap();
        assert_eq!(empty.setup().len(), 0);

        let single = scenarios.get("single_admin").unwrap();
        let users = single.setup();
        assert_eq!(users.len(), 1);
        assert!(users[0].is_admin);

        let all = scenarios.execute_all();
        assert_eq!(all.len(), 4);
    }

    #[test]
    fn test_factory() {
        let factory = TestFactory::new()
            .register("admin", || UserFixture::typical().with_admin(true))
            .register("regular", || UserFixture::typical().with_admin(false));

        let admin = factory.create("admin").unwrap();
        assert!(admin.is_admin);

        let regulars = factory.create_many("regular", 3);
        assert_eq!(regulars.len(), 3);
        assert!(regulars.iter().all(|u| !u.is_admin));
    }
}
