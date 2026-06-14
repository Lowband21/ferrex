# ferrex-model

Shared data models for the Ferrex media platform.

This crate provides the core domain types used across the Ferrex ecosystem:
- Media item representations (movies, shows, episodes)
- Library and collection structures
- User and authentication types
- Metadata types (TMDB integration)

## Features

- `serde` - Enable serde serialization/deserialization
- `rkyv` - Enable zero-copy deserialization with rkyv
- `chrono` - Enable chrono datetime types
- `sqlx` - Enable SQLx database integration

## Usage

```toml
[dependencies]
ferrex-model = { version = "0.1.0-alpha", features = ["serde"] }
```

## License

Licensed under MIT OR Apache-2.0.
