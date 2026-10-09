//! Content indexing (T202, ADR-029): what turns catalogued files into searchable content.
//!
//! - [`run_content_pass`]: text files that are new, changed or stale are extracted and
//!   chunked (`lumen-extract`) and their chunks replace the old ones in the store.
//! - [`run_queue`]: the persistent embedding queue — chunks without a vector in the target
//!   generation are embedded in small batches, under a [`Control`] that pauses, throttles
//!   (duty cycle) and yields to interactive work.
//!
//! Both run on the single indexing thread that owns the SQLite writer (ADR-025); the shell
//! decides when (start-up, after catalog syncs, power and resource policy).

#![forbid(unsafe_code)]

mod code;
mod images;
mod pass;
mod queue;

pub use images::{run_image_pass, run_image_slice};
pub use lumen_image::EXTENSIONS as IMAGE_EXTENSIONS;
pub use pass::{PassConfig, PassReport, Slice, run_content_pass, run_content_slice};
pub use queue::{Control, Hold, QueueConfig, QueueError, QueueJob, QueueReport, Stop, run_queue};

#[cfg(test)]
mod tests;
