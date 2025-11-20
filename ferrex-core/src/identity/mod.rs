//! Identity/auth bounded context facade.

pub mod auth {
    pub use crate::auth::*;
}

pub mod users {
    pub use crate::user::*;
    pub use crate::user_management::*;
}

pub mod rbac {
    pub use crate::rbac::*;
}
