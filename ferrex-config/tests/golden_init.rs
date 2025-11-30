use std::{
    collections::{HashMap, HashSet},
    fs,
    os::unix::fs::PermissionsExt,
};

use ferrex_config::{
    cli::{InitOptions, RotateTarget, gen_init_merge_env, generate_init_kv},
    constants::{IGNORED_KEYS, MANAGED_KEYS},
    env_writer::{
        merge_env_contents, merge_env_with_template, write_env_atomically,
    },
};
use once_cell::sync::Lazy;
use tempfile::tempdir;

fn clear_host_overrides() {
    for key in [
        "FERREX_CONFIG_INIT_DATABASE_URL",
        "FERREX_CONFIG_INIT_HOST_DATABASE_URL",
        "FERREX_CONFIG_INIT_REDIS_URL",
        "FERREX_CONFIG_INIT_HOST_REDIS_URL",
        "DATABASE_APP_PASSWORD_FILE",
        "DATABASE_ADMIN_PASSWORD_FILE",
        "FERREX_APP_PASSWORD_FILE",
        "DATABASE_PASSWORD_FILE",
        "AUTH_PASSWORD_PEPPER_FILE",
        "AUTH_TOKEN_KEY_FILE",
        "FERREX_SETUP_TOKEN_FILE",
        "FERREX_INTERNAL_DB_RESET",
    ] {
        unsafe { std::env::remove_var(key) };
    }
}

fn render_kv(lines: &[(String, String)]) -> String {
    lines
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[tokio::test]
async fn golden_init_non_interactive_basic() {
    let _guard = ENV_LOCK.lock().await;
    clear_host_overrides();
    unsafe { std::env::set_var("FERREX_INIT_TEST_SEED", "1") };

    let dir = tempdir().expect("tempdir");
    let env_path = dir.path().join(".env");

    let opts = InitOptions::new_non_interactive(env_path, false);

    let kv = generate_init_kv(&opts).await.expect("generate init");
    let rendered = render_kv(&kv);

    unsafe { std::env::remove_var("FERREX_INIT_TEST_SEED") };

    // Just verify key fields are present with expected values, don't check exact order
    assert!(rendered.contains("DEV_MODE=true"));
    assert!(rendered.contains("SERVER_HOST=0.0.0.0"));
    assert!(rendered.contains("SERVER_PORT=3000"));
    assert!(rendered.contains("DATABASE_HOST=localhost"));
    assert!(
        rendered
            .contains("DATABASE_APP_PASSWORD=0zsMbNLxQh9yYtHhJYiMaDz7zbJMXJN5")
    );
    assert!(rendered.contains("DATABASE_URL=postgresql://ferrex_app:0zsMbNLxQh9yYtHhJYiMaDz7zbJMXJN5@localhost:5432/ferrex"));
    assert!(rendered.contains("AUTH_PASSWORD_PEPPER=0zsMbNLxQh9yYtHhJYiMaDz7zbJMXJN5fYT4VCfmUQP8nl9oCHXQhMKncW6eDPB9"));
    assert!(rendered.contains(
        "FERREX_SETUP_TOKEN=0zsMbNLxQh9yYtHhJYiMaDz7zbJMXJN5fYT4VCfmUQP8nl9o"
    ));
}

#[tokio::test]
async fn golden_init_non_interactive_existing_prod_advanced() {
    let _guard = ENV_LOCK.lock().await;
    clear_host_overrides();
    unsafe { std::env::set_var("FERREX_INIT_TEST_SEED", "1") };
    // Signal that DB rotation is safe (simulates --reset-db path)
    unsafe { std::env::set_var("FERREX_INTERNAL_DB_RESET", "1") };

    let dir = tempdir().expect("tempdir");
    let env_path = dir.path().join(".env");
    fs::write(
        &env_path,
        "\
DEV_MODE=false
SERVER_HOST=10.0.0.5
SERVER_PORT=443
TMDB_API_KEY=abc123
ENFORCE_HTTPS=true
TRUST_PROXY_HEADERS=false
DATABASE_HOST=db
DATABASE_URL=postgresql://ferrex_app:secret@db:5432/ferrex
DATABASE_ADMIN_PASSWORD=changeme_admin
DATABASE_APP_PASSWORD=changeme_app
REDIS_URL=redis://10.0.0.20:6379
",
    )
    .expect("write env");

    let mut opts = InitOptions::new_non_interactive(env_path, true);
    opts.rotate = RotateTarget::Db;

    let kv = generate_init_kv(&opts).await.expect("generate init");
    let rendered = render_kv(&kv);

    unsafe { std::env::remove_var("FERREX_INIT_TEST_SEED") };
    unsafe { std::env::remove_var("FERREX_INTERNAL_DB_RESET") };

    assert!(rendered.contains("DATABASE_HOST=db"));
    assert!(rendered.contains("DATABASE_URL=postgresql://ferrex_app:"));
    assert!(rendered.contains("REDIS_URL=redis://10.0.0.20:6379"));
    assert!(rendered.contains("HSTS_MAX_AGE=31536000"));
    assert!(rendered.contains("DATABASE_ADMIN_PASSWORD="));
    assert!(rendered.contains("DATABASE_APP_PASSWORD="));
    assert!(rendered.contains("AUTH_PASSWORD_PEPPER=0zsMbN"));
}

#[tokio::test]
async fn tailscale_mode_overrides_container_hosts() {
    let _guard = ENV_LOCK.lock().await;
    clear_host_overrides();
    unsafe { std::env::set_var("FERREX_INIT_TEST_SEED", "2") };

    let dir = tempdir().expect("tempdir");
    let env_path = dir.path().join(".env");

    let mut opts = InitOptions::new_non_interactive(env_path, false);
    opts.tailscale = true;

    let kv = generate_init_kv(&opts).await.expect("generate init");
    let rendered = render_kv(&kv);

    assert!(rendered.contains("DATABASE_HOST_CONTAINER=127.0.0.1"));
    assert!(rendered.contains("REDIS_URL_CONTAINER=redis://127.0.0.1:6379"));
}

#[tokio::test]
async fn file_secrets_are_preferred_over_placeholders() {
    let _guard = ENV_LOCK.lock().await;
    clear_host_overrides();
    let dir = tempdir().expect("tempdir");
    let env_path = dir.path().join(".env");

    let app_password_file = dir.path().join("app_pw");
    fs::write(&app_password_file, "file-secret-app").unwrap();
    unsafe {
        std::env::set_var(
            "DATABASE_APP_PASSWORD_FILE",
            app_password_file.display().to_string(),
        );
    }

    let opts = InitOptions::new_non_interactive(env_path, false);
    let kv = generate_init_kv(&opts).await.expect("generate init");
    let rendered = render_kv(&kv);

    assert!(rendered.contains("DATABASE_APP_PASSWORD=file-secret-app"));
}

#[test]
fn managed_keys_preserve_unmanaged_lines() {
    let existing = "CUSTOM=1\nDATABASE_HOST=old\n# comment stays\n";
    let managed = vec![
        ("DATABASE_HOST".to_string(), "newhost".to_string()),
        ("DATABASE_PORT".to_string(), "5432".to_string()),
    ];
    let managed_keys: HashSet<String> =
        MANAGED_KEYS.iter().map(|s: &&str| s.to_string()).collect();

    let merged = merge_env_contents(existing, &managed, &managed_keys);

    assert!(merged.contains("CUSTOM=1"));
    assert!(merged.contains("DATABASE_HOST=newhost"));
    assert!(merged.contains("DATABASE_PORT=5432"));
    assert!(merged.contains("comment stays"));
}

#[test]
fn env_example_keys_are_classified() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let example_path =
        std::path::Path::new(&manifest_dir).join("../.env.example");
    let contents = fs::read_to_string(example_path).expect("read .env.example");

    let managed: HashSet<&str> = MANAGED_KEYS.iter().copied().collect();
    let ignored: HashSet<&str> = IGNORED_KEYS.iter().copied().collect();

    for line in contents.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some((key, _)) = trimmed.split_once('=') {
            if managed.contains(key) || ignored.contains(key) {
                continue;
            }
            panic!("Unclassified key in .env.example: {key}");
        }
    }
}

#[test]
fn atomic_write_preserves_permissions() {
    let dir = tempdir().unwrap();
    let env_path = dir.path().join(".env");
    fs::write(&env_path, "FOO=bar\n").unwrap();
    fs::set_permissions(&env_path, fs::Permissions::from_mode(0o640)).unwrap();

    write_env_atomically(&env_path, "FOO=baz\nBAR=qux\n").unwrap();

    let metadata = fs::metadata(&env_path).unwrap();
    assert_eq!(metadata.permissions().mode() & 0o777, 0o640);

    let content = fs::read_to_string(&env_path).unwrap();
    assert!(content.contains("FOO=baz"));
    assert!(content.contains("BAR=qux"));
}

static ENV_LOCK: Lazy<tokio::sync::Mutex<()>> =
    Lazy::new(|| tokio::sync::Mutex::new(()));

#[test]
fn template_layout_preserves_headers_and_overrides() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let example_path =
        std::path::Path::new(&manifest_dir).join("../.env.example");
    let template = fs::read_to_string(example_path).expect("read .env.example");

    // Existing env overrides a known key from the template and adds a custom one.
    let existing = "\
TMDB_API_KEY=my-key
CUSTOM_VAR=1
# custom comment
";

    // Simulate managed values as the CLI would provide them.
    let managed = vec![
        ("SERVER_HOST".to_string(), "0.0.0.0".to_string()),
        ("SERVER_PORT".to_string(), "3000".to_string()),
        ("TMDB_API_KEY".to_string(), "my-key".to_string()),
    ];
    let managed_keys: HashSet<String> =
        MANAGED_KEYS.iter().map(|s: &&str| s.to_string()).collect();

    let merged =
        merge_env_with_template(existing, &managed, &managed_keys, &template);

    // ASCII banner from the template should be present.
    assert!(
        merged.contains(
            "# Run `just start` to populate `.env` based on this template"
        ),
        "expected template header to be preserved"
    );

    // Managed keys should use managed values.
    assert!(merged.contains("SERVER_HOST=0.0.0.0"));
    assert!(merged.contains("SERVER_PORT=3000"));

    // Known non-managed key should preserve the override from `existing`.
    assert!(merged.contains("TMDB_API_KEY=my-key"));

    // Custom value and comment should be present in a custom overrides section.
    assert!(
        merged.contains("# Custom overrides (preserved from previous runs)")
    );
    assert!(merged.contains("CUSTOM_VAR=1"));
    assert!(merged.contains("# custom comment"));
}

#[test]
fn template_merge_is_idempotent() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let example_path =
        std::path::Path::new(&manifest_dir).join("../.env.example");
    let template = fs::read_to_string(example_path).expect("read .env.example");

    let existing = "\
TMDB_API_KEY=my-key
CUSTOM_VAR=1
";

    let managed = vec![
        ("SERVER_HOST".to_string(), "0.0.0.0".to_string()),
        ("SERVER_PORT".to_string(), "3000".to_string()),
    ];
    let managed_keys: HashSet<String> =
        MANAGED_KEYS.iter().map(|s: &&str| s.to_string()).collect();

    let first =
        merge_env_with_template(existing, &managed, &managed_keys, &template);
    let second =
        merge_env_with_template(&first, &managed, &managed_keys, &template);

    assert_eq!(first, second, "template-based merge should be idempotent");
}

#[tokio::test]
async fn rotate_db_only_changes_database_secrets() {
    let _guard = ENV_LOCK.lock().await;
    clear_host_overrides();
    unsafe { std::env::set_var("FERREX_INIT_TEST_SEED", "3") };
    // Signal that DB rotation is safe (simulates --reset-db path)
    unsafe { std::env::set_var("FERREX_INTERNAL_DB_RESET", "1") };

    let dir = tempdir().expect("tempdir");
    let env_path = dir.path().join(".env");

    // Start from an env with non-placeholder DB and auth secrets so rotation
    // semantics are explicit.
    fs::write(
        &env_path,
        "\
DATABASE_ADMIN_PASSWORD=keep_admin
DATABASE_APP_PASSWORD=keep_app
AUTH_PASSWORD_PEPPER=keep_pepper
AUTH_TOKEN_KEY=keep_token
FERREX_SETUP_TOKEN=keep_setup
",
    )
    .expect("write env");

    let mut opts = InitOptions::new_non_interactive(env_path, false);
    opts.rotate = RotateTarget::Db;

    let outcome = gen_init_merge_env(&opts)
        .await
        .expect("generate init with rotation");

    unsafe { std::env::remove_var("FERREX_INIT_TEST_SEED") };
    unsafe { std::env::remove_var("FERREX_INTERNAL_DB_RESET") };

    let map: HashMap<_, _> = outcome.kv.into_iter().collect();

    // DB passwords should have changed.
    assert_ne!(map.get("DATABASE_ADMIN_PASSWORD").unwrap(), "keep_admin");
    assert_ne!(map.get("DATABASE_APP_PASSWORD").unwrap(), "keep_app");

    // Auth secrets should be preserved.
    assert_eq!(map.get("AUTH_PASSWORD_PEPPER").unwrap(), "keep_pepper");
    assert_eq!(map.get("AUTH_TOKEN_KEY").unwrap(), "keep_token");
    assert_eq!(map.get("FERREX_SETUP_TOKEN").unwrap(), "keep_setup");

    // Outcome should report only DB secrets as rotated.
    let rotated: HashSet<_> = outcome.rotated_keys.into_iter().collect();
    assert!(rotated.contains("DATABASE_ADMIN_PASSWORD"));
    assert!(rotated.contains("DATABASE_APP_PASSWORD"));
    assert!(!rotated.contains("AUTH_PASSWORD_PEPPER"));
    assert!(!rotated.contains("AUTH_TOKEN_KEY"));
    assert!(!rotated.contains("FERREX_SETUP_TOKEN"));
}

#[tokio::test]
async fn rotate_auth_only_changes_auth_secrets() {
    let _guard = ENV_LOCK.lock().await;
    clear_host_overrides();
    unsafe { std::env::set_var("FERREX_INIT_TEST_SEED", "4") };

    let dir = tempdir().expect("tempdir");
    let env_path = dir.path().join(".env");

    fs::write(
        &env_path,
        "\
DATABASE_ADMIN_PASSWORD=keep_admin
DATABASE_APP_PASSWORD=keep_app
AUTH_PASSWORD_PEPPER=keep_pepper
AUTH_TOKEN_KEY=keep_token
FERREX_SETUP_TOKEN=keep_setup
",
    )
    .expect("write env");

    let mut opts = InitOptions::new_non_interactive(env_path, false);
    opts.rotate = RotateTarget::Auth;

    let outcome = gen_init_merge_env(&opts)
        .await
        .expect("generate init with auth rotate");

    unsafe { std::env::remove_var("FERREX_INIT_TEST_SEED") };

    let map: HashMap<_, _> = outcome.kv.into_iter().collect();

    // DB passwords should be preserved.
    assert_eq!(map.get("DATABASE_ADMIN_PASSWORD").unwrap(), "keep_admin");
    assert_eq!(map.get("DATABASE_APP_PASSWORD").unwrap(), "keep_app");

    // Auth secrets should have changed.
    assert_ne!(map.get("AUTH_PASSWORD_PEPPER").unwrap(), "keep_pepper");
    assert_ne!(map.get("AUTH_TOKEN_KEY").unwrap(), "keep_token");
    assert_ne!(map.get("FERREX_SETUP_TOKEN").unwrap(), "keep_setup");

    let rotated: HashSet<_> = outcome.rotated_keys.into_iter().collect();
    assert!(!rotated.contains("DATABASE_ADMIN_PASSWORD"));
    assert!(!rotated.contains("DATABASE_APP_PASSWORD"));
    assert!(rotated.contains("AUTH_PASSWORD_PEPPER"));
    assert!(rotated.contains("AUTH_TOKEN_KEY"));
    assert!(rotated.contains("FERREX_SETUP_TOKEN"));
}

#[tokio::test]
async fn rotate_all_changes_db_and_auth_even_with_file_secrets() {
    let _guard = ENV_LOCK.lock().await;
    clear_host_overrides();
    unsafe { std::env::set_var("FERREX_INIT_TEST_SEED", "5") };
    // Signal that DB rotation is safe (simulates --reset-db path)
    // Note: --rotate all only rotates DB if FERREX_INTERNAL_DB_RESET is set
    unsafe { std::env::set_var("FERREX_INTERNAL_DB_RESET", "1") };

    let dir = tempdir().expect("tempdir");
    let env_path = dir.path().join(".env");

    // Seed file-backed secrets to ensure --rotate overrides them.
    let app_pw_file = dir.path().join("app_pw");
    let admin_pw_file = dir.path().join("admin_pw");
    let pepper_file = dir.path().join("pepper");
    let token_file = dir.path().join("token");
    let setup_file = dir.path().join("setup");

    fs::write(&app_pw_file, "file_app").unwrap();
    fs::write(&admin_pw_file, "file_admin").unwrap();
    fs::write(&pepper_file, "file_pepper").unwrap();
    fs::write(&token_file, "file_token").unwrap();
    fs::write(&setup_file, "file_setup").unwrap();

    fs::write(
        &env_path,
        format!(
            "\
DATABASE_APP_PASSWORD_FILE={}
DATABASE_ADMIN_PASSWORD_FILE={}
AUTH_PASSWORD_PEPPER_FILE={}
AUTH_TOKEN_KEY_FILE={}
FERREX_SETUP_TOKEN_FILE={}
",
            app_pw_file.display(),
            admin_pw_file.display(),
            pepper_file.display(),
            token_file.display(),
            setup_file.display()
        ),
    )
    .expect("write env");

    let mut opts = InitOptions::new_non_interactive(env_path, false);
    opts.rotate = RotateTarget::All;

    let outcome = gen_init_merge_env(&opts)
        .await
        .expect("generate init with rotate all");

    unsafe { std::env::remove_var("FERREX_INIT_TEST_SEED") };
    unsafe { std::env::remove_var("FERREX_INTERNAL_DB_RESET") };

    let map: HashMap<_, _> = outcome.kv.into_iter().collect();

    // All managed secrets should no longer match the file contents.
    assert_ne!(map.get("DATABASE_APP_PASSWORD").unwrap(), "file_app");
    assert_ne!(map.get("DATABASE_ADMIN_PASSWORD").unwrap(), "file_admin");
    assert_ne!(map.get("AUTH_PASSWORD_PEPPER").unwrap(), "file_pepper");
    assert_ne!(map.get("AUTH_TOKEN_KEY").unwrap(), "file_token");
    assert_ne!(map.get("FERREX_SETUP_TOKEN").unwrap(), "file_setup");

    let rotated: HashSet<_> = outcome.rotated_keys.into_iter().collect();
    assert!(rotated.contains("DATABASE_ADMIN_PASSWORD"));
    assert!(rotated.contains("DATABASE_APP_PASSWORD"));
    assert!(rotated.contains("AUTH_PASSWORD_PEPPER"));
    assert!(rotated.contains("AUTH_TOKEN_KEY"));
    assert!(rotated.contains("FERREX_SETUP_TOKEN"));
}
