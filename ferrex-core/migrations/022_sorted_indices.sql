-- Migration 022: Add sorted indices table for efficient media sorting
-- This table stores pre-sorted media IDs for each library and sort field combination

-- Create the sorted indices table
CREATE TABLE library_sorted_indices (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    library_id UUID NOT NULL REFERENCES libraries(id) ON DELETE CASCADE,
    sort_field VARCHAR(50) NOT NULL,
    sort_order VARCHAR(10) NOT NULL CHECK (sort_order IN ('ascending', 'descending')),
    media_ids UUID[] NOT NULL,
    metadata JSONB DEFAULT '{}', -- Store additional metadata like user_id for user-specific sorts
    last_updated TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    version INT NOT NULL DEFAULT 1,
    UNIQUE (library_id, sort_field, sort_order, metadata)
);

-- Create indices for efficient lookups
CREATE INDEX idx_sorted_indices_library ON library_sorted_indices(library_id);
CREATE INDEX idx_sorted_indices_sort_field ON library_sorted_indices(sort_field);
CREATE INDEX idx_sorted_indices_last_updated ON library_sorted_indices(last_updated);
CREATE INDEX idx_sorted_indices_metadata ON library_sorted_indices USING GIN(metadata);

-- Add comment explaining the table's purpose
COMMENT ON TABLE library_sorted_indices IS 'Stores pre-sorted media IDs for efficient client-side sorting';
COMMENT ON COLUMN library_sorted_indices.metadata IS 'Additional context like user_id for user-specific sorts (LastWatched, WatchProgress)';
