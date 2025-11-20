-- Add folder inventory table for tracking and managing discovered folders

-- Create folder_inventory table
CREATE TABLE folder_inventory (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    library_id UUID NOT NULL REFERENCES libraries(id) ON DELETE CASCADE,
    folder_path TEXT NOT NULL,
    folder_type VARCHAR(50) NOT NULL CHECK (folder_type IN ('root', 'movie', 'tv_show', 'season', 'extra', 'unknown')),
    parent_folder_id UUID REFERENCES folder_inventory(id) ON DELETE CASCADE,
    
    -- Discovery tracking fields
    discovered_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    last_seen_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    discovery_source VARCHAR(50) NOT NULL DEFAULT 'scan' CHECK (discovery_source IN ('scan', 'watch', 'manual', 'import')),
    
    -- Processing status fields
    processing_status VARCHAR(50) NOT NULL DEFAULT 'pending' CHECK (processing_status IN ('pending', 'processing', 'completed', 'failed', 'skipped', 'queued')),
    last_processed_at TIMESTAMP WITH TIME ZONE,
    processing_error TEXT,
    processing_attempts INTEGER NOT NULL DEFAULT 0,
    next_retry_at TIMESTAMP WITH TIME ZONE,
    
    -- Content tracking fields
    total_files INTEGER NOT NULL DEFAULT 0,
    processed_files INTEGER NOT NULL DEFAULT 0,
    total_size_bytes BIGINT NOT NULL DEFAULT 0,
    file_types JSONB DEFAULT '[]'::jsonb,
    last_modified TIMESTAMP WITH TIME ZONE,
    
    -- Metadata storage
    metadata JSONB DEFAULT '{}'::jsonb,
    
    -- Timestamps
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    
    -- Constraints
    CONSTRAINT unique_library_folder_path UNIQUE (library_id, folder_path),
    CONSTRAINT valid_parent_relationship CHECK (id != parent_folder_id),
    CONSTRAINT valid_file_counts CHECK (processed_files <= total_files)
);

-- Performance indexes for efficient queries
-- Index for library queries
CREATE INDEX idx_folder_inventory_library_id ON folder_inventory(library_id);

-- Index for folder hierarchy traversal
CREATE INDEX idx_folder_inventory_parent_folder_id ON folder_inventory(parent_folder_id);

-- Index for processing queue
CREATE INDEX idx_folder_inventory_processing_queue ON folder_inventory(processing_status, next_retry_at)
    WHERE processing_status IN ('pending', 'queued', 'failed');

-- Index for finding folders needing scan
CREATE INDEX idx_folder_inventory_needs_scan ON folder_inventory(library_id, last_seen_at, processing_status)
    WHERE processing_status != 'skipped';

-- Index for folder type queries
CREATE INDEX idx_folder_inventory_folder_type ON folder_inventory(folder_type, library_id);

-- Index for discovery source tracking
CREATE INDEX idx_folder_inventory_discovery_source ON folder_inventory(discovery_source, discovered_at DESC);

-- Index for error tracking and retry
CREATE INDEX idx_folder_inventory_retry ON folder_inventory(processing_attempts, next_retry_at)
    WHERE processing_status = 'failed' AND next_retry_at IS NOT NULL;

-- Index for content size analysis
CREATE INDEX idx_folder_inventory_size ON folder_inventory(library_id, total_size_bytes DESC);

-- Full text search index on folder path
CREATE INDEX idx_folder_inventory_path_gin ON folder_inventory USING gin(to_tsvector('simple', folder_path));

-- Trigger to update the updated_at timestamp
CREATE TRIGGER update_folder_inventory_updated_at 
    BEFORE UPDATE ON folder_inventory 
    FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();

-- Add comments for documentation
COMMENT ON TABLE folder_inventory IS 'Tracks discovered folders in media libraries for efficient scanning and processing';
COMMENT ON COLUMN folder_inventory.folder_type IS 'Type of content in folder: root, movie, tv_show, season, extra, or unknown';
COMMENT ON COLUMN folder_inventory.discovery_source IS 'How the folder was discovered: scan, watch (file watcher), manual, or import';
COMMENT ON COLUMN folder_inventory.processing_status IS 'Current processing state: pending, processing, completed, failed, skipped, or queued';
COMMENT ON COLUMN folder_inventory.file_types IS 'JSON array of file extensions found in the folder, e.g., ["mp4", "mkv", "srt"]';
COMMENT ON COLUMN folder_inventory.metadata IS 'Flexible JSON storage for additional folder metadata like permissions, attributes, etc.';
COMMENT ON COLUMN folder_inventory.last_modified IS 'Filesystem last modified timestamp for the folder';
COMMENT ON COLUMN folder_inventory.total_size_bytes IS 'Total size of all files in the folder in bytes';