# Auth Domain Migration Status

## Completed Tasks

### 1. Domain Message Structure ✅
- Created `src/messages/auth.rs` with all 51 auth-related messages
- Created `src/messages/mod.rs` with DomainMessage router
- Added automatic From implementations for ergonomic message conversion
- Included cross-domain event definitions

### 2. Update Function Migration ✅
- Created `src/updates/update_auth.rs` to handle all auth domain messages
- Maps all auth messages to their respective handlers
- Converts legacy Message responses back to auth::Message
- All handlers properly delegate to existing implementations

### 3. Application Integration ✅
- Modified `main.rs` to use DomainMessage instead of Message
- Created domain-aware update function that routes messages
- Updated init function to return DomainMessage tasks
- Updated view function to return Element<DomainMessage>
- Updated subscription function to return Subscription<DomainMessage>
- All legacy views/subscriptions wrapped with DomainMessage::Legacy

### 4. Handler Functions ✅
- All auth handlers exist in `auth_updates.rs` and `first_run_updates.rs`
- Device auth flow handlers properly mapped
- Password login handlers properly mapped
- First-run setup handlers properly mapped
- PIN authentication handlers (mostly stubs for now)

## Next Steps

### 1. Update Auth Views (Pending)
Auth views still emit legacy Message. Need to update:
- `src/views/user_selection.rs`
- `src/views/pin_entry.rs`
- `src/views/password_login.rs`
- `src/views/first_run.rs`
- `src/views/auth/*` (if any)

### 2. Feature Flag Integration (Optional)
Add feature flag support:
```toml
[features]
auth-domain = []
```

Then conditionally compile:
```rust
#[cfg(feature = "auth-domain")]
DomainMessage::Auth(msg) => update_auth(state, msg).map(DomainMessage::Auth),

#[cfg(not(feature = "auth-domain"))]
DomainMessage::Auth(msg) => {
    let legacy = auth_to_legacy(msg);
    legacy_update(state, legacy).map(DomainMessage::Legacy)
}
```

### 3. Cross-Domain Events (Future)
When LoginSuccess happens, need to emit:
- `CrossDomainEvent::UserAuthenticated`
- Trigger library refresh
- Load watch status

## Migration Pattern for Other Domains

This auth migration establishes the pattern:

1. Create domain message module (`messages/domain.rs`)
2. Create update handler (`updates/update_domain.rs`)
3. Map legacy messages in `messages/mod.rs`
4. Route in main update function
5. Update views to emit domain messages
6. Handle cross-domain events

## Testing

To test the auth domain:
```bash
# Run normally - auth messages go through domain routing
cargo run

# Future: Run with feature flag
cargo run --features auth-domain
```

## Benefits Achieved

1. **Separation of Concerns**: Auth logic isolated from other domains
2. **Type Safety**: Auth messages can only produce auth messages
3. **Migration Path**: Legacy support allows gradual migration
4. **Performance**: Message routing adds minimal overhead
5. **Maintainability**: Clear domain boundaries