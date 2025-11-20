-- Check current schema state

\echo '=== media_files columns ==='
SELECT column_name, data_type, is_nullable 
FROM information_schema.columns 
WHERE table_name = 'media_files' 
ORDER BY ordinal_position;

\echo '\n=== media_metadata columns ==='
SELECT column_name, data_type, is_nullable 
FROM information_schema.columns 
WHERE table_name = 'media_metadata' 
ORDER BY ordinal_position;

\echo '\n=== tv_shows columns ==='
SELECT column_name, data_type, is_nullable 
FROM information_schema.columns 
WHERE table_name = 'tv_shows' 
ORDER BY ordinal_position;

\echo '\n=== tv_episodes columns ==='
SELECT column_name, data_type, is_nullable 
FROM information_schema.columns 
WHERE table_name = 'tv_episodes' 
ORDER BY ordinal_position;

\echo '\n=== Constraints on tv_episodes ==='
SELECT constraint_name, constraint_type 
FROM information_schema.table_constraints 
WHERE table_name = 'tv_episodes';

\echo '\n=== Primary keys ==='
SELECT table_name, constraint_name 
FROM information_schema.table_constraints 
WHERE constraint_type = 'PRIMARY KEY' 
AND table_name IN ('media_files', 'media_metadata', 'tv_shows', 'tv_episodes');