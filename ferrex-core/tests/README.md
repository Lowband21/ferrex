# Ferrex Core Test Suite

## Running Query Optimization Tests

The query optimization tests require a PostgreSQL database to be running with the following configuration:

### Test Database Setup

1. Create a test database:
```bash
createdb ferrex_test
```

2. Create a test user:
```sql
CREATE USER ferrex WITH PASSWORD 'ferrex';
GRANT ALL PRIVILEGES ON DATABASE ferrex_test TO ferrex;
```

3. Apply migrations to the test database:
```bash
export DATABASE_URL="postgresql://ferrex:ferrex@localhost:5432/ferrex_test"
cargo sqlx database reset
cargo sqlx migrate run
```

### Running Tests

Run all query tests:
```bash
export TEST_DATABASE_URL="postgresql://ferrex:ferrex@localhost:5432/ferrex_test"
cargo test -p ferrex-core --test query_tests
cargo test -p ferrex-core --test query_search_tests
cargo test -p ferrex-core --test query_performance_tests
```

Run with single thread to avoid database conflicts:
```bash
cargo test -p ferrex-core --test query_tests -- --test-threads=1
```

### Test Coverage

The test suite covers:

1. **Filter Tests** (`query_tests.rs`)
   - Genre filtering with GIN indexes
   - Year range filtering with B-tree indexes
   - Rating range filtering with B-tree indexes
   - Complex multi-filter queries

2. **Search Tests** (`query_search_tests.rs`)
   - Exact text search with ILIKE
   - Fuzzy text search with trigram similarity
   - Field-specific search (title, overview, cast)
   - Combined search across all fields

3. **Performance Tests** (`query_performance_tests.rs`)
   - Large dataset queries (1000+ records)
   - Query execution time < 100ms verification
   - Concurrent query handling
   - Deep pagination performance

### Performance Expectations

All queries should complete within these time limits:
- Simple queries: < 50ms
- Complex filtered queries: < 100ms
- Search queries: < 150ms
- Fuzzy search: < 200ms