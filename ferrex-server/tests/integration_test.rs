use serde_json::{Value, json};
use std::time::Duration;
use tempfile::TempDir;

const BASE_URL: &str = "http://localhost:3000";

#[tokio::test]
#[ignore = "requires server running"]
async fn test_library_crud_operations() {
    // Wait for server to be ready
    tokio::time::sleep(Duration::from_secs(1)).await;

    let client = reqwest::Client::new();

    // Test creating a library
    let temp_dir = TempDir::new().unwrap();
    let path = temp_dir.path().to_str().unwrap();

    let create_request = json!({
        "name": "Test Movies Library",
        "library_type": "movies",
        "paths": [path],
        "scan_interval_minutes": 60,
        "enabled": true
    });

    let response = client
        .post(&format!("{}/libraries", BASE_URL))
        .json(&create_request)
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 200);

    let json: Value = response.json().await.unwrap();
    assert_eq!(json["status"], "success");
    let library_id = json["id"].as_str().unwrap();

    // Test listing libraries
    let response = client
        .get(&format!("{}/libraries", BASE_URL))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 200);
    let json: Value = response.json().await.unwrap();
    assert_eq!(json["status"], "success");
    assert!(json["libraries"].is_array());
    assert!(!json["libraries"].as_array().unwrap().is_empty());

    // Test getting a specific library
    let response = client
        .get(&format!("{}/libraries/{}", BASE_URL, library_id))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 200);
    let json: Value = response.json().await.unwrap();
    assert_eq!(json["status"], "success");
    assert_eq!(json["library"]["name"], "Test Movies Library");
    assert_eq!(json["library"]["library_type"], "Movies");

    // Test updating a library
    let update_request = json!({
        "name": "Updated Test Library",
        "scan_interval_minutes": 120
    });

    let response = client
        .put(&format!("{}/libraries/{}", BASE_URL, library_id))
        .json(&update_request)
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 200);
    let json: Value = response.json().await.unwrap();
    assert_eq!(json["status"], "success");

    // Verify the update
    let response = client
        .get(&format!("{}/libraries/{}", BASE_URL, library_id))
        .send()
        .await
        .unwrap();

    let json: Value = response.json().await.unwrap();
    assert_eq!(json["library"]["name"], "Updated Test Library");
    assert_eq!(json["library"]["scan_interval_minutes"], 120);

    // Test deleting the library
    let response = client
        .delete(&format!("{}/libraries/{}", BASE_URL, library_id))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 200);
    let json: Value = response.json().await.unwrap();
    assert_eq!(json["status"], "success");

    // Verify it's deleted
    let response = client
        .get(&format!("{}/libraries/{}", BASE_URL, library_id))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 404);
}

#[tokio::test]
#[ignore = "requires server running"]
async fn test_library_validation() {
    let client = reqwest::Client::new();

    // Test invalid library type
    let temp_dir = TempDir::new().unwrap();
    let path = temp_dir.path().to_str().unwrap();

    let create_request = json!({
        "name": "Invalid Library",
        "library_type": "invalid_type",
        "paths": [path],
        "scan_interval_minutes": 60,
        "enabled": true
    });

    let response = client
        .post(&format!("{}/libraries", BASE_URL))
        .json(&create_request)
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 200);
    let json: Value = response.json().await.unwrap();
    assert_eq!(json["status"], "error");
    assert!(
        json["error"]
            .as_str()
            .unwrap()
            .contains("Invalid library type")
    );

    // Test non-existent path
    let create_request = json!({
        "name": "Invalid Path Library",
        "library_type": "movies",
        "paths": ["/non/existent/path"],
        "scan_interval_minutes": 60,
        "enabled": true
    });

    let response = client
        .post(&format!("{}/libraries", BASE_URL))
        .json(&create_request)
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 200);
    let json: Value = response.json().await.unwrap();
    assert_eq!(json["status"], "error");
    assert!(
        json["error"]
            .as_str()
            .unwrap()
            .contains("Path does not exist")
    );
}

#[tokio::test]
#[ignore = "requires server running"]
async fn test_multiple_libraries() {
    let client = reqwest::Client::new();

    // Create directories
    let movies_dir = TempDir::new().unwrap();
    let tv_dir = TempDir::new().unwrap();

    // Create movies library
    let create_request = json!({
        "name": "Movies Library",
        "library_type": "movies",
        "paths": [movies_dir.path().to_str().unwrap()],
        "scan_interval_minutes": 60,
        "enabled": true
    });

    let response = client
        .post(&format!("{}/libraries", BASE_URL))
        .json(&create_request)
        .send()
        .await
        .unwrap();

    let json: Value = response.json().await.unwrap();
    let movies_id = json["id"].as_str().unwrap();

    // Create TV shows library
    let create_request = json!({
        "name": "TV Shows Library",
        "library_type": "tvshows",
        "paths": [tv_dir.path().to_str().unwrap()],
        "scan_interval_minutes": 30,
        "enabled": true
    });

    let response = client
        .post(&format!("{}/libraries", BASE_URL))
        .json(&create_request)
        .send()
        .await
        .unwrap();

    let json: Value = response.json().await.unwrap();
    let tv_id = json["id"].as_str().unwrap();

    // List libraries and verify both exist
    let response = client
        .get(&format!("{}/libraries", BASE_URL))
        .send()
        .await
        .unwrap();

    let json: Value = response.json().await.unwrap();
    let libraries = json["libraries"].as_array().unwrap();

    assert!(libraries.len() >= 2);

    let has_movies = libraries.iter().any(|lib| {
        lib["id"].as_str() == Some(movies_id) && lib["library_type"].as_str() == Some("Movies")
    });
    let has_tv = libraries.iter().any(|lib| {
        lib["id"].as_str() == Some(tv_id) && lib["library_type"].as_str() == Some("TvShows")
    });

    assert!(has_movies);
    assert!(has_tv);

    // Clean up
    client
        .delete(&format!("{}/libraries/{}", BASE_URL, movies_id))
        .send()
        .await
        .unwrap();
    client
        .delete(&format!("{}/libraries/{}", BASE_URL, tv_id))
        .send()
        .await
        .unwrap();
}
