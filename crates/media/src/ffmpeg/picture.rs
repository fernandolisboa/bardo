//! A picture rewritten as a JPEG of the same size, small enough for a
//! limit: a persona's look drawn larger than every take model reads
//! (#188) is kept this way rather than thrown away, and every look is sent
//! to a take model this way, which drops its metadata (#192). A frame of a
//! video is taken the same way.

use std::path::Path;
use std::time::Duration;

use super::{Ffmpeg, MediaError, partial_path, path_arg, process, remove_partial};

/// JPEG qualities tried in turn (ffmpeg's `-q:v`, lower is better) until
/// the file is small enough.
const QUALITIES: [u8; 4] = [2, 4, 7, 12];

impl Ffmpeg {
    /// Writes `source` as a JPEG at `destination`, same width and height,
    /// within `max_bytes`.
    pub fn fit_picture(
        &self,
        source: &Path,
        destination: &Path,
        max_bytes: u64,
    ) -> Result<(), MediaError> {
        self.jpeg(source, None, destination, max_bytes)
    }

    /// Writes the frame of the video at `source` shown at `at` as a JPEG
    /// at `destination`, within `max_bytes`.
    pub fn frame_picture(
        &self,
        source: &Path,
        at: Duration,
        destination: &Path,
        max_bytes: u64,
    ) -> Result<(), MediaError> {
        self.jpeg(source, Some(at), destination, max_bytes)
    }

    /// One picture of `source`, from `at` for a video, as a JPEG with no
    /// metadata (EXIF, GPS, device) within `max_bytes`.
    fn jpeg(
        &self,
        source: &Path,
        at: Option<Duration>,
        destination: &Path,
        max_bytes: u64,
    ) -> Result<(), MediaError> {
        let partial = partial_path(destination);
        for quality in QUALITIES {
            let mut command = self.ffmpeg();
            command.arg("-y");
            if let Some(at) = at {
                command.args(["-ss", &format!("{:.3}", at.as_secs_f64())]);
            }
            command
                .arg("-i")
                .arg(path_arg(source))
                .args(["-map_metadata", "-1"])
                .args(["-frames:v", "1"])
                .args(["-c:v", "mjpeg", "-q:v", &quality.to_string()])
                .args(["-pix_fmt", "yuvj420p", "-f", "mjpeg"])
                .arg(path_arg(&partial));
            if let Err(error) = process::output(command) {
                remove_partial(&partial);
                return Err(error);
            }
            if std::fs::metadata(&partial)?.len() <= max_bytes {
                std::fs::rename(&partial, destination)?;
                return Ok(());
            }
        }
        remove_partial(&partial);
        Err(MediaError::Io(std::io::Error::other(format!(
            "the picture stays over {max_bytes} bytes at every quality"
        ))))
    }
}
