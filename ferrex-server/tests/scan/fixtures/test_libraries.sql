INSERT INTO libraries (
    id,
    name,
    library_type,
    paths,
    scan_interval_minutes,
    enabled,
    auto_scan,
    watch_for_changes,
    analyze_on_scan,
    max_retry_attempts
) VALUES
    ('aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa', 'Fixture Library A', 'movies', ARRAY['/fixture/library/a'], 60, TRUE, TRUE, TRUE, FALSE, 3),
    ('bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb', 'Fixture Library B', 'movies', ARRAY['/fixture/library/b'], 60, TRUE, TRUE, TRUE, FALSE, 3),
    ('cccccccc-cccc-cccc-cccc-cccccccccccc', 'Fixture Library C', 'tvshows', ARRAY['/fixture/library/c'], 60, TRUE, TRUE, TRUE, FALSE, 3)
ON CONFLICT (id) DO NOTHING;
