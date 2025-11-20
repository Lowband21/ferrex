use ferrex_core::{
    AuthenticationService, UserAuthentication,
    infrastructure::repositories::PostgresUserAuthRepository,
};

#[test]
fn test_auth_domain_accessible() {
    // This test just verifies types are accessible
    // If it compiles, the exports are working
    let _ = std::marker::PhantomData::<PostgresUserAuthRepository>;
    let _ = std::marker::PhantomData::<AuthenticationService>;
    let _ = std::marker::PhantomData::<UserAuthentication>;
}
