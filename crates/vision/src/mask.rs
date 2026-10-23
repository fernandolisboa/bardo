//! MediaPipe's multiclass selfie segmenter: the whole frame stretched to
//! 256×256, values -1..1, six class logits per cell (background, hair,
//! body skin, face skin, clothes, other). The performer is everything but
//! background.

use bardo_domain::{Device, ModelInfo, PersonMask, PersonSegmenter, RgbFrame, VisionError};

use crate::frame::stretch_hwc;
use crate::runtime::Model;

pub(crate) const SIZE: usize = 256;
const CLASSES: usize = 6;

pub(crate) struct OnnxSegmenter {
    model: Model,
    info: ModelInfo,
    input: Vec<f32>,
}

impl OnnxSegmenter {
    pub(crate) fn new(model: Model, info: ModelInfo) -> OnnxSegmenter {
        OnnxSegmenter {
            model,
            info,
            input: vec![0.0; SIZE * SIZE * 3],
        }
    }
}

/// Per cell, softmax of the logits: the person probability (all but
/// background) and the likeliest class.
fn mask_from_logits(logits: &[f32]) -> PersonMask {
    let mut person = Vec::with_capacity(SIZE * SIZE);
    let mut class = Vec::with_capacity(SIZE * SIZE);
    for cell in logits.chunks_exact(CLASSES) {
        let max = cell.iter().copied().fold(f32::MIN, f32::max);
        let mut p = [0.0f32; CLASSES];
        let mut sum = 0.0;
        for c in 0..CLASSES {
            p[c] = (cell[c] - max).exp();
            sum += p[c];
        }
        let best = (0..CLASSES)
            .max_by(|&a, &b| p[a].total_cmp(&p[b]))
            .unwrap_or(0);
        person.push(((1.0 - p[0] / sum) * 255.0).round() as u8);
        class.push(best as u8);
    }
    PersonMask {
        width: SIZE as u32,
        height: SIZE as u32,
        person,
        class,
    }
}

impl PersonSegmenter for OnnxSegmenter {
    fn model(&self) -> &ModelInfo {
        &self.info
    }

    fn device(&self) -> &Device {
        self.model.device()
    }

    fn segment(&mut self, frame: &RgbFrame) -> Result<PersonMask, VisionError> {
        stretch_hwc(frame, SIZE, SIZE, &mut self.input, |v| v / 127.5 - 1.0);
        let out = self.model.run(&[1, SIZE, SIZE, 3], &self.input)?;
        let logits = out
            .first()
            .filter(|l| l.len() == SIZE * SIZE * CLASSES)
            .ok_or_else(|| {
                VisionError::Failed("the segmenter's output is not 256×256×6".to_owned())
            })?;
        Ok(mask_from_logits(logits))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logits_become_person_probability_and_class() {
        let mut logits = vec![0.0; SIZE * SIZE * CLASSES];
        // Cell 0 sure background, cell 1 sure face skin.
        logits[0] = 10.0;
        logits[CLASSES + 3] = 10.0;
        let mask = mask_from_logits(&logits);
        assert_eq!(mask.person[0], 0);
        assert_eq!(mask.class[0], 0);
        assert_eq!(mask.person[1], 255);
        assert_eq!(mask.class[1], 3);
        // Even odds: five classes out of six are the person.
        assert_eq!(mask.person[2], 213);
    }
}
