# ferrex-contracts

Trait surfaces and domain contracts built atop `ferrex-model`.

This crate provides the trait definitions and contracts that define
the boundaries between Ferrex components:
- Repository traits for data access
- Service traits for business logic
- Event contracts for async communication

## Features

- `serde` - Enable serde support via ferrex-model
- `rkyv` - Enable rkyv support via ferrex-model
- `chrono` - Enable chrono support via ferrex-model

## Usage

```toml
[dependencies]
ferrex-contracts = { version = "0.1.0-alpha", features = ["serde"] }
```

## License

Licensed under MIT OR Apache-2.0.
