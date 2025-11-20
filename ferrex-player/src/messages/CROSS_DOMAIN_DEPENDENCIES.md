# Cross-Domain Dependencies Analysis

## Critical Dependencies to Handle

### 1. Auth → Library
- `LoginSuccess` triggers:
  - Library refresh (`RefreshAll`)
  - Watch status loading
  - User permissions check

### 2. Library → Media
- `LibrarySelected` triggers:
  - Media list update
  - UI refresh
  - Metadata fetching for visible items

### 3. Media → Streaming
- `PlayMedia` triggers:
  - Transcoding check
  - HLS playlist loading
  - Bandwidth measurement

### 4. UI → Multiple Domains
- `WindowResized` affects:
  - Grid recalculation
  - Image loading priorities
  - Player controls layout
  
- `SetViewMode` affects:
  - Library filtering
  - Metadata fetching priorities
  - Scroll position reset

### 5. Metadata → UI
- `MetadataFetched` triggers:
  - UI updates for affected items
  - Image handle updates
  - Backdrop transitions

## Recommended Event Flow Pattern

```rust
// In your update method
match message {
    DomainMessage::Auth(auth::Message::LoginSuccess(user, perms)) => {
        // Handle auth state update
        let auth_task = self.auth_service.handle(msg);
        
        // Emit cross-domain event
        let events = vec![
            CrossDomainEvent::UserAuthenticated(user, perms),
        ];
        
        // Collect tasks from other domains
        let library_tasks = self.library_service.handle_events(&events);
        let ui_tasks = self.ui_service.handle_events(&events);
        
        // Batch all tasks
        Task::batch(vec![auth_task, library_tasks, ui_tasks])
    }
}
```

## Migration Order Rationale

1. **Auth First** (Week 1)
   - Least coupled
   - Clear boundaries
   - Sets up user context for other domains

2. **Streaming Second** (Week 2)
   - Mostly self-contained
   - Clear async boundaries
   - Can work with legacy media messages

3. **UI Third** (Week 3)
   - Many dependencies but mostly one-way
   - Can emit events without waiting for responses

4. **Library Fourth** (Week 4)
   - Central to app function
   - Needs auth context
   - Triggers many other domains

5. **Media Fifth** (Week 5)
   - Most complex state management
   - Heavy coupling with video player
   - Depends on streaming

6. **Metadata Last** (Week 6)
   - Most dependent on other domains
   - Can operate async in background

## Testing Strategy

1. **Feature Flags**: Test each domain in isolation
   ```bash
   cargo test --features auth-domain
   ```

2. **Integration Points**: Focus testing on:
   - Event emission
   - Task coordination
   - State synchronization

3. **Rollback Plan**: Legacy wrapper allows instant rollback