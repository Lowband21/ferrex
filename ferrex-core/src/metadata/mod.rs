mod filename_parser;
mod hdr_metadata;
mod metadata_extractor;
mod technical_metadata;

pub use metadata_extractor::MetadataExtractor;

// Re-export internal modules for tests if needed
#[cfg(test)]
pub(crate) use filename_parser::FilenameParser;