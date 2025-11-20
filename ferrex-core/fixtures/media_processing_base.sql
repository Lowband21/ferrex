INSERT INTO media_files (
    id,
    library_id,
    file_path,
    filename,
    file_size,
    technical_metadata,
    parsed_info
)
VALUES
    (
        '11111111-1111-1111-1111-111111111111',
        'aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa',
        '/fixture/library/a/movie_one.mkv',
        'movie_one.mkv',
        1,
        NULL,
        NULL
    ),
    (
        '22222222-2222-2222-2222-222222222222',
        'aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa',
        '/fixture/library/a/movie_two.mkv',
        'movie_two.mkv',
        2,
        NULL,
        NULL
    ),
    (
        '33333333-3333-3333-3333-333333333333',
        'aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa',
        '/fixture/library/a/movie_three.mkv',
        'movie_three.mkv',
        3,
        NULL,
        NULL
    )
ON CONFLICT (file_path) DO NOTHING;

INSERT INTO movie_references (
    id,
    library_id,
    file_id,
    tmdb_id,
    title,
    theme_color
)
VALUES (
    '44444444-4444-4444-4444-444444444444',
    'aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa',
    '11111111-1111-1111-1111-111111111111',
    1044,
    'Fixture Movie One',
    '#112233'
)
ON CONFLICT (id) DO NOTHING;
