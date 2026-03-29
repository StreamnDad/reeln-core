pub mod error;
pub mod registry;
pub mod segment;

pub use error::SportError;
pub use registry::{
    SportAlias, SportRegistry, default_event_type_entries, default_event_types, sport_from_dict,
};
pub use segment::{
    Segment, make_segment, make_segments, segment_dir_name, segment_display_name,
    validate_segment_for_sport, validate_segment_number,
};
