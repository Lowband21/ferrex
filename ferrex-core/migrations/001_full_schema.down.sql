-- Down migration: remove application schema and extensions created in 001
DROP SCHEMA IF EXISTS ferrex CASCADE;
DROP EXTENSION IF EXISTS citext;
DROP EXTENSION IF EXISTS pg_trgm;
DROP EXTENSION IF EXISTS pgcrypto;
