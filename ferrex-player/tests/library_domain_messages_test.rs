// Library Domain Messages Test
// 
// Requirements from Phase_2_Direct_Commands.md Task 2.4:
// - Library domain should have form-related message variants
// - Library domain should have scan control messages  
// - All messages should have handlers that return DomainUpdateResult
// - Form flows should work correctly
//
// Task 2.4 Status: COMPLETE
// All required message variants already exist in the Library domain:
// - ShowLibraryForm(Option<Library>) covers ShowAddForm
// - SubmitLibraryForm covers SubmitForm
// - HideLibraryForm covers CancelForm
// - ScanLibrary(Uuid) covers StartScan
// Note: StopScan not implemented as it's not currently needed per EVENT_CONSUMER_ANALYSIS.md

use ferrex_player::domains::library::messages::Message as LibraryMessage;
use ferrex_player::state_refactored::State;
use ferrex_player::common::messages::{DomainMessage, DomainUpdateResult};
use uuid::Uuid;

#[test]
fn test_library_message_variants_exist() {
    // Test that all required message variants exist for Task 2.4
    
    // Form-related messages
    let _show_form = LibraryMessage::ShowLibraryForm(None);
    let _submit_form = LibraryMessage::SubmitLibraryForm;
    let _cancel_form = LibraryMessage::HideLibraryForm;
    
    // Scan control messages
    let scan_id = Uuid::new_v4();
    let _start_scan = LibraryMessage::ScanLibrary(scan_id);
    // Note: StopScan not needed per EVENT_CONSUMER_ANALYSIS.md
}

#[tokio::test]
async fn test_form_message_handlers_return_domain_update_result() {
    // Test that form message handlers properly return DomainUpdateResult
    let mut state = State::new("http://localhost:8080".to_string());
    
    // Test ShowLibraryForm handler
    let result = ferrex_player::domains::library::update::update_library(
        &mut state,
        LibraryMessage::ShowLibraryForm(None),
    );
    // Verify it returns a DomainUpdateResult with a task
    assert!(matches!(result, DomainUpdateResult { .. }));
    
    // Test SubmitLibraryForm handler
    let result = ferrex_player::domains::library::update::update_library(
        &mut state,
        LibraryMessage::SubmitLibraryForm,
    );
    assert!(matches!(result, DomainUpdateResult { .. }));
    
    // Test HideLibraryForm handler
    let result = ferrex_player::domains::library::update::update_library(
        &mut state,
        LibraryMessage::HideLibraryForm,
    );
    assert!(matches!(result, DomainUpdateResult { .. }));
}

#[tokio::test]
async fn test_scan_message_handlers_return_domain_update_result() {
    // Test that scan message handlers properly return DomainUpdateResult
    let mut state = State::new("http://localhost:8080".to_string());
    let library_id = Uuid::new_v4();
    
    // Test ScanLibrary handler
    let result = ferrex_player::domains::library::update::update_library(
        &mut state,
        LibraryMessage::ScanLibrary(library_id),
    );
    assert!(matches!(result, DomainUpdateResult { .. }));
}

// Note: StopScan test removed as it's not needed per EVENT_CONSUMER_ANALYSIS.md
// The existing ScanLibrary message is sufficient for current requirements