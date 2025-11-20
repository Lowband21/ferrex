use ferrex_core::user::*;
use uuid::Uuid;
use std::time::{SystemTime, UNIX_EPOCH};

pub struct UserBuilder {
    id: Uuid,
    username: String,
    display_name: String,
    password_hash: String,
    created_at: i64,
}

impl Default for UserBuilder {
    fn default() -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        
        Self {
            id: Uuid::new_v4(),
            username: format!("testuser_{}", Uuid::new_v4().to_string().chars().take(8).collect::<String>()),
            display_name: "Test User".to_string(),
            password_hash: "$argon2id$v=19$m=19456,t=2,p=1$VE0e3g7U1FJRYfkUBBWjww$G5o/J7X1k7Qr+kZ8GtWCXMTQKBGEmqOTHZbf1L+si88".to_string(), // "password123"
            created_at: now,
        }
    }
}

impl UserBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_username(mut self, username: &str) -> Self {
        self.username = username.to_string();
        self
    }

    pub fn with_display_name(mut self, display_name: &str) -> Self {
        self.display_name = display_name.to_string();
        self
    }

    pub fn with_password_hash(mut self, hash: &str) -> Self {
        self.password_hash = hash.to_string();
        self
    }

    pub fn with_created_at(mut self, timestamp: i64) -> Self {
        self.created_at = timestamp;
        self
    }

    pub fn build(self) -> User {
        User {
            id: self.id,
            username: self.username,
            display_name: self.display_name,
            password_hash: self.password_hash,
            created_at: self.created_at,
        }
    }
}

pub struct SessionBuilder {
    id: Uuid,
    user_id: Uuid,
    device_name: Option<String>,
    ip_address: Option<String>,
    user_agent: Option<String>,
    last_active: i64,
    created_at: i64,
}

impl Default for SessionBuilder {
    fn default() -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        
        Self {
            id: Uuid::new_v4(),
            user_id: Uuid::new_v4(),
            device_name: None,
            ip_address: None,
            user_agent: None,
            last_active: now,
            created_at: now,
        }
    }
}

impl SessionBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn for_user(mut self, user_id: Uuid) -> Self {
        self.user_id = user_id;
        self
    }

    pub fn with_device(mut self, device_name: &str) -> Self {
        self.device_name = Some(device_name.to_string());
        self
    }

    pub fn with_ip(mut self, ip: &str) -> Self {
        self.ip_address = Some(ip.to_string());
        self
    }

    pub fn with_user_agent(mut self, ua: &str) -> Self {
        self.user_agent = Some(ua.to_string());
        self
    }

    pub fn build(self) -> UserSession {
        UserSession {
            id: self.id,
            user_id: self.user_id,
            device_name: self.device_name,
            ip_address: self.ip_address,
            user_agent: self.user_agent,
            last_active: self.last_active,
            created_at: self.created_at,
        }
    }
}

pub struct AuthTokenBuilder {
    access_token: String,
    refresh_token: String,
    expires_in: u32,
}

impl Default for AuthTokenBuilder {
    fn default() -> Self {
        Self {
            access_token: format!("test_access_{}", Uuid::new_v4()),
            refresh_token: Uuid::new_v4().to_string(),
            expires_in: 900, // 15 minutes
        }
    }
}

impl AuthTokenBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_access_token(mut self, token: &str) -> Self {
        self.access_token = token.to_string();
        self
    }

    pub fn with_refresh_token(mut self, token: &str) -> Self {
        self.refresh_token = token.to_string();
        self
    }

    pub fn with_expiry(mut self, seconds: u32) -> Self {
        self.expires_in = seconds;
        self
    }

    pub fn build(self) -> AuthToken {
        AuthToken {
            access_token: self.access_token,
            refresh_token: self.refresh_token,
            expires_in: self.expires_in,
        }
    }
}

// Test data generators
pub fn create_test_users(count: usize) -> Vec<User> {
    (0..count)
        .map(|i| {
            UserBuilder::new()
                .with_username(&format!("user{}", i))
                .with_display_name(&format!("Test User {}", i))
                .build()
        })
        .collect()
}

pub fn create_test_jwt_claims(user_id: Uuid, expires_in_seconds: i64) -> Claims {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    
    Claims {
        sub: user_id,
        exp: now + expires_in_seconds,
        iat: now,
        jti: Uuid::new_v4().to_string(),
    }
}

// Common test passwords
pub const TEST_PASSWORD: &str = "password123";
pub const TEST_PASSWORD_HASH: &str = "$argon2id$v=19$m=19456,t=2,p=1$VE0e3g7U1FJRYfkUBBWjww$G5o/J7X1k7Qr+kZ8GtWCXMTQKBGEmqOTHZbf1L+si88";