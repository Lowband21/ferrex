//! Test data builders with compile-time validation
//!
//! Provides a type-safe builder pattern that enforces required fields at compile time.

use std::marker::PhantomData;

/// Marker trait for required fields
pub trait RequiredField {}

/// Marker type for a field that has been set
pub struct Set;

/// Marker type for a field that has not been set
pub struct NotSet;

impl RequiredField for Set {}

/// Generic builder trait
pub trait Builder {
    type Output;
    
    /// Build the final object
    fn build(self) -> Self::Output;
}

/// Example: User builder with required and optional fields
pub struct UserBuilder<Name = NotSet, Email = NotSet> {
    name: Option<String>,
    email: Option<String>,
    age: Option<u32>,
    is_admin: bool,
    _phantom: PhantomData<(Name, Email)>,
}

impl Default for UserBuilder<NotSet, NotSet> {
    fn default() -> Self {
        Self::new()
    }
}

impl UserBuilder<NotSet, NotSet> {
    /// Create a new user builder
    pub fn new() -> Self {
        Self {
            name: None,
            email: None,
            age: None,
            is_admin: false,
            _phantom: PhantomData,
        }
    }
}

impl<Name, Email> UserBuilder<Name, Email> {
    /// Set the user's name (required)
    pub fn with_name(self, name: impl Into<String>) -> UserBuilder<Set, Email> {
        UserBuilder {
            name: Some(name.into()),
            email: self.email,
            age: self.age,
            is_admin: self.is_admin,
            _phantom: PhantomData,
        }
    }
    
    /// Set the user's email (required)
    pub fn with_email(self, email: impl Into<String>) -> UserBuilder<Name, Set> {
        UserBuilder {
            name: self.name,
            email: Some(email.into()),
            age: self.age,
            is_admin: self.is_admin,
            _phantom: PhantomData,
        }
    }
    
    /// Set the user's age (optional)
    pub fn with_age(mut self, age: u32) -> Self {
        self.age = Some(age);
        self
    }
    
    /// Set admin status (optional)
    pub fn as_admin(mut self) -> Self {
        self.is_admin = true;
        self
    }
}

#[derive(Debug, Clone)]
pub struct User {
    pub name: String,
    pub email: String,
    pub age: Option<u32>,
    pub is_admin: bool,
}

impl Builder for UserBuilder<Set, Set> {
    type Output = User;
    
    fn build(self) -> User {
        User {
            name: self.name.expect("Name should be set"),
            email: self.email.expect("Email should be set"),
            age: self.age,
            is_admin: self.is_admin,
        }
    }
}

/// Macro to generate builders with required fields
#[macro_export]
macro_rules! builder {
    (
        $builder_name:ident for $output:ident {
            required {
                $($req_field:ident: $req_type:ty),* $(,)?
            }
            optional {
                $($opt_field:ident: $opt_type:ty = $opt_default:expr),* $(,)?
            }
        }
    ) => {
        // Generate marker types for each required field
        $(
            #[allow(non_camel_case_types)]
            type $req_field = ();
        )*
        
        // Generate the builder struct
        pub struct $builder_name<$($req_field = NotSet),*> {
            $($req_field: Option<$req_type>,)*
            $($opt_field: $opt_type,)*
            _phantom: PhantomData<($($req_field),*)>,
        }
        
        // Default implementation for new builder
        impl Default for $builder_name<$(NotSet),*> {
            fn default() -> Self {
                Self::new()
            }
        }
        
        impl $builder_name<$(NotSet),*> {
            pub fn new() -> Self {
                Self {
                    $($req_field: None,)*
                    $($opt_field: $opt_default,)*
                    _phantom: PhantomData,
                }
            }
        }
        
        // Methods for setting required fields
        impl<$($req_field),*> $builder_name<$($req_field),*> {
            $(
                paste::paste! {
                    pub fn [<with_ $req_field>](self, value: $req_type) -> $builder_name<$(Set),*> {
                        $builder_name {
                            $req_field: Some(value),
                            $($opt_field: self.$opt_field,)*
                            _phantom: PhantomData,
                        }
                    }
                }
            )*
            
            // Methods for setting optional fields
            $(
                paste::paste! {
                    pub fn [<with_ $opt_field>](mut self, value: $opt_type) -> Self {
                        self.$opt_field = value;
                        self
                    }
                }
            )*
        }
        
        // Build implementation when all required fields are set
        impl Builder for $builder_name<$(Set),*> {
            type Output = $output;
            
            fn build(self) -> $output {
                $output {
                    $($req_field: self.$req_field.expect(concat!(stringify!($req_field), " should be set")),)*
                    $($opt_field: self.$opt_field,)*
                }
            }
        }
    };
}

/// Collection builder for building multiple related objects
pub struct CollectionBuilder<T> {
    items: Vec<T>,
}

impl<T> CollectionBuilder<T> {
    pub fn new() -> Self {
        Self { items: Vec::new() }
    }
    
    /// Add an item to the collection
    pub fn add(mut self, item: T) -> Self {
        self.items.push(item);
        self
    }
    
    /// Add multiple items
    pub fn add_many(mut self, items: impl IntoIterator<Item = T>) -> Self {
        self.items.extend(items);
        self
    }
    
    /// Map over items
    pub fn map<F, U>(self, f: F) -> CollectionBuilder<U>
    where
        F: FnMut(T) -> U,
    {
        CollectionBuilder {
            items: self.items.into_iter().map(f).collect(),
        }
    }
    
    /// Filter items
    pub fn filter<F>(self, f: F) -> Self
    where
        F: FnMut(&T) -> bool,
    {
        Self {
            items: self.items.into_iter().filter(f).collect(),
        }
    }
    
    /// Build the collection
    pub fn build(self) -> Vec<T> {
        self.items
    }
}

impl<T> Default for CollectionBuilder<T> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_user_builder_required_fields() {
        let user = UserBuilder::new()
            .with_name("Alice")
            .with_email("alice@example.com")
            .build();
        
        assert_eq!(user.name, "Alice");
        assert_eq!(user.email, "alice@example.com");
        assert_eq!(user.age, None);
        assert!(!user.is_admin);
    }
    
    #[test]
    fn test_user_builder_all_fields() {
        let user = UserBuilder::new()
            .with_name("Bob")
            .with_email("bob@example.com")
            .with_age(30)
            .as_admin()
            .build();
        
        assert_eq!(user.name, "Bob");
        assert_eq!(user.email, "bob@example.com");
        assert_eq!(user.age, Some(30));
        assert!(user.is_admin);
    }
    
    // This would fail to compile (demonstrating compile-time validation):
    // #[test]
    // fn test_missing_required_field() {
    //     let user = UserBuilder::new()
    //         .with_name("Alice")
    //         .build(); // Error: UserBuilder<Set, NotSet> doesn't implement Builder
    // }
    
    #[test]
    fn test_collection_builder() {
        let users = CollectionBuilder::new()
            .add(User {
                name: "Alice".to_string(),
                email: "alice@example.com".to_string(),
                age: Some(25),
                is_admin: false,
            })
            .add(User {
                name: "Bob".to_string(),
                email: "bob@example.com".to_string(),
                age: Some(30),
                is_admin: true,
            })
            .filter(|u| u.is_admin)
            .build();
        
        assert_eq!(users.len(), 1);
        assert_eq!(users[0].name, "Bob");
    }
}