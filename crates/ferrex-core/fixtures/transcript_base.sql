-- Timed-text repository fixtures.
--
-- Seeds a source-level transcript artifact, one sidecar transcript source, and
-- bounded non-overlapping cue rows for Arrival. Raw transcript text appears only
-- in transcript segment storage and artifact content, not artifact summaries.

SET search_path TO ferrex, public;

INSERT INTO intelligence_artifacts (
    id,
    artifact_kind,
    scope,
    status,
    library_id,
    media_id,
    media_type,
    title,
    summary,
    content_hash,
    content,
    metadata
) VALUES (
    '88888888-0000-0000-0000-000000000001',
    'transcript_source',
    'global',
    'active',
    'aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa',
    '22222222-0000-0000-0000-000000000001',
    'movie',
    'Arrival English subtitles',
    'English sidecar transcript source for Arrival.',
    repeat('a', 64),
    jsonb_build_object('raw_body', 'Louise translates the alien language without panic.'),
    jsonb_build_object('fixture', true)
)
ON CONFLICT (id) DO NOTHING;

INSERT INTO transcript_sources (
    id,
    library_id,
    media_id,
    media_type,
    media_file_id,
    source_kind,
    status,
    language_code,
    source_key,
    source_name,
    source_path_hash,
    source_content_hash,
    normalized_content_hash,
    artifact_id,
    duration_ms,
    segment_count,
    extracted_at,
    source_locator,
    metadata
) VALUES (
    '88888888-0000-0000-0000-000000000101',
    'aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa',
    '22222222-0000-0000-0000-000000000001',
    'movie',
    '33333333-0000-0000-0000-000000000001',
    'sidecar',
    'active',
    'en',
    'sidecar:' || repeat('b', 64),
    'English sidecar',
    repeat('b', 64),
    repeat('c', 64),
    repeat('d', 64),
    '88888888-0000-0000-0000-000000000001',
    7200000,
    3,
    now(),
    jsonb_build_object('kind', 'sidecar_hash'),
    jsonb_build_object('fixture', true)
)
ON CONFLICT (library_id, media_file_id, source_kind, language_code, source_key) DO NOTHING;

INSERT INTO transcript_segments (
    id,
    transcript_source_id,
    library_id,
    media_id,
    media_type,
    media_file_id,
    language_code,
    cue_index,
    start_ms,
    end_ms,
    cue_text,
    segment_hash,
    metadata
) VALUES
    (
        '88888888-0000-0000-0000-000000000201',
        '88888888-0000-0000-0000-000000000101',
        'aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa',
        '22222222-0000-0000-0000-000000000001',
        'movie',
        '33333333-0000-0000-0000-000000000001',
        'en',
        0,
        1000,
        3500,
        'Humanity listens for visitors from beyond the stars.',
        repeat('e', 64),
        jsonb_build_object('fixture', true)
    ),
    (
        '88888888-0000-0000-0000-000000000202',
        '88888888-0000-0000-0000-000000000101',
        'aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa',
        '22222222-0000-0000-0000-000000000001',
        'movie',
        '33333333-0000-0000-0000-000000000001',
        'en',
        1,
        3500,
        6000,
        'Louise translates the alien language without panic.',
        repeat('f', 64),
        jsonb_build_object('fixture', true)
    ),
    (
        '88888888-0000-0000-0000-000000000203',
        '88888888-0000-0000-0000-000000000101',
        'aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa',
        '22222222-0000-0000-0000-000000000001',
        'movie',
        '33333333-0000-0000-0000-000000000001',
        'en',
        2,
        6000,
        9000,
        'The heptapod message asks humanity to choose trust.',
        repeat('0', 64),
        jsonb_build_object('fixture', true)
    )
ON CONFLICT (transcript_source_id, cue_index) DO NOTHING;

INSERT INTO transcript_processing_status (
    id,
    library_id,
    media_id,
    media_type,
    media_file_id,
    status,
    source_count,
    segment_count,
    attempt_count,
    finished_at,
    metadata
) VALUES (
    '88888888-0000-0000-0000-000000000301',
    'aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa',
    '22222222-0000-0000-0000-000000000001',
    'movie',
    '33333333-0000-0000-0000-000000000001',
    'succeeded',
    1,
    3,
    1,
    now(),
    jsonb_build_object('fixture', true)
)
ON CONFLICT (library_id, media_file_id) DO UPDATE SET
    status = EXCLUDED.status,
    source_count = EXCLUDED.source_count,
    segment_count = EXCLUDED.segment_count,
    attempt_count = EXCLUDED.attempt_count,
    finished_at = EXCLUDED.finished_at,
    updated_at = now();

INSERT INTO intelligence_artifact_sources (
    artifact_id,
    source_ordinal,
    source_kind,
    source_transcript_source_id,
    source_library_id,
    source_media_id,
    source_media_type,
    source_content_hash,
    source_excerpt,
    source_locator
) VALUES (
    '88888888-0000-0000-0000-000000000001',
    0,
    'transcript_source',
    '88888888-0000-0000-0000-000000000101',
    'aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa',
    '22222222-0000-0000-0000-000000000001',
    'movie',
    repeat('c', 64),
    'English sidecar transcript source.',
    jsonb_build_object('kind', 'transcript_source')
)
ON CONFLICT (artifact_id, source_ordinal) DO NOTHING;
