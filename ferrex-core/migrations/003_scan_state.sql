-- Add scan state tracking and media processing status tables

-- Scan state tracking for resumable and persistent scans
CREATE TABLE scan_state (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    library_id UUID NOT NULL REFERENCES libraries(id) ON DELETE CASCADE,
    scan_type VARCHAR(20) NOT NULL CHECK (scan_type IN ('full', 'incremental', 'refresh_metadata', 'analyze')),
    status VARCHAR(20) NOT NULL CHECK (status IN ('pending', 'running', 'paused', 'completed', 'failed', 'cancelled')),
    total_folders INTEGER DEFAULT 0,
    processed_folders INTEGER DEFAULT 0,
    total_files INTEGER DEFAULT 0,
    processed_files INTEGER DEFAULT 0,
    current_path TEXT,
    error_count INTEGER DEFAULT 0,
    errors JSONB DEFAULT '[]'::jsonb,
    started_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    completed_at TIMESTAMP WITH TIME ZONE,
    options JSONB NOT NULL DEFAULT '{}'::jsonb, -- Stores scan options like force_refresh, analyze_only, etc.
    CONSTRAINT valid_progress CHECK (
        processed_folders <= total_folders AND
        processed_files <= total_files
    )
);

-- Media processing status to track what operations have been completed per file
CREATE TABLE media_processing_status (
    media_file_id UUID REFERENCES media_files(id) ON DELETE CASCADE PRIMARY KEY,
    metadata_extracted BOOLEAN NOT NULL DEFAULT false,
    metadata_extracted_at TIMESTAMP WITH TIME ZONE,
    tmdb_matched BOOLEAN NOT NULL DEFAULT false,
    tmdb_matched_at TIMESTAMP WITH TIME ZONE,
    images_cached BOOLEAN NOT NULL DEFAULT false,
    images_cached_at TIMESTAMP WITH TIME ZONE,
    file_analyzed BOOLEAN NOT NULL DEFAULT false,  -- For thumbnail generation, etc.
    file_analyzed_at TIMESTAMP WITH TIME ZONE,
    last_error TEXT,
    error_details JSONB,
    retry_count INTEGER NOT NULL DEFAULT 0,
    next_retry_at TIMESTAMP WITH TIME ZONE,
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW()
);

-- File watch events queue for tracking filesystem changes
CREATE TABLE file_watch_events (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    library_id UUID NOT NULL REFERENCES libraries(id) ON DELETE CASCADE,
    event_type VARCHAR(20) NOT NULL CHECK (event_type IN ('created', 'modified', 'deleted', 'moved')),
    file_path TEXT NOT NULL,
    old_path TEXT, -- For move events
    file_size BIGINT, -- For validation
    detected_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    processed BOOLEAN NOT NULL DEFAULT false,
    processed_at TIMESTAMP WITH TIME ZONE,
    processing_attempts INTEGER NOT NULL DEFAULT 0,
    last_error TEXT,
    CONSTRAINT valid_move_event CHECK (
        (event_type = 'moved' AND old_path IS NOT NULL) OR
        (event_type != 'moved' AND old_path IS NULL)
    )
);

-- Performance indices for scan_state
CREATE INDEX idx_scan_state_library_id ON scan_state(library_id);
CREATE INDEX idx_scan_state_status ON scan_state(status);
CREATE INDEX idx_scan_state_scan_type ON scan_state(scan_type);
CREATE INDEX idx_scan_state_started_at ON scan_state(started_at DESC);
CREATE INDEX idx_scan_state_active ON scan_state(library_id, status) 
    WHERE status IN ('pending', 'running', 'paused');

-- Performance indices for media_processing_status
CREATE INDEX idx_media_processing_status_metadata ON media_processing_status(metadata_extracted) 
    WHERE metadata_extracted = false;
CREATE INDEX idx_media_processing_status_tmdb ON media_processing_status(tmdb_matched) 
    WHERE tmdb_matched = false;
CREATE INDEX idx_media_processing_status_images ON media_processing_status(images_cached) 
    WHERE images_cached = false;
CREATE INDEX idx_media_processing_status_analyzed ON media_processing_status(file_analyzed) 
    WHERE file_analyzed = false;
CREATE INDEX idx_media_processing_status_retry ON media_processing_status(next_retry_at) 
    WHERE retry_count > 0 AND next_retry_at IS NOT NULL;

-- Performance indices for file_watch_events
CREATE INDEX idx_file_watch_events_library_id ON file_watch_events(library_id);
CREATE INDEX idx_file_watch_events_unprocessed ON file_watch_events(library_id, detected_at) 
    WHERE processed = false;
CREATE INDEX idx_file_watch_events_file_path ON file_watch_events(file_path);
CREATE INDEX idx_file_watch_events_detected_at ON file_watch_events(detected_at DESC);

-- Update triggers for maintaining updated_at timestamps
CREATE TRIGGER update_scan_state_updated_at 
    BEFORE UPDATE ON scan_state 
    FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();

CREATE TRIGGER update_media_processing_status_updated_at 
    BEFORE UPDATE ON media_processing_status 
    FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();

-- Add columns to libraries table for enhanced scan configuration
ALTER TABLE libraries ADD COLUMN IF NOT EXISTS auto_scan BOOLEAN NOT NULL DEFAULT true;
ALTER TABLE libraries ADD COLUMN IF NOT EXISTS watch_for_changes BOOLEAN NOT NULL DEFAULT true;
ALTER TABLE libraries ADD COLUMN IF NOT EXISTS analyze_on_scan BOOLEAN NOT NULL DEFAULT false;
ALTER TABLE libraries ADD COLUMN IF NOT EXISTS max_retry_attempts INTEGER NOT NULL DEFAULT 3;

-- Add comments for documentation
COMMENT ON TABLE scan_state IS 'Tracks the state of library scans for resumability and monitoring';
COMMENT ON TABLE media_processing_status IS 'Tracks processing status for each media file to enable incremental scanning';
COMMENT ON TABLE file_watch_events IS 'Queue of filesystem events detected by file watcher';

COMMENT ON COLUMN scan_state.scan_type IS 'Type of scan: full, incremental, refresh_metadata, or analyze';
COMMENT ON COLUMN scan_state.options IS 'JSON object with scan options like {force_refresh: bool, skip_tmdb: bool, analyze_files: bool}';
COMMENT ON COLUMN media_processing_status.file_analyzed IS 'Whether advanced analysis (thumbnails, previews) has been performed';
COMMENT ON COLUMN file_watch_events.event_type IS 'Type of filesystem event detected';