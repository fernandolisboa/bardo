//! What a tracker or estimator carries from frame to frame, as bytes: a
//! tag naming what wrote it, then little-endian numbers and arrays.

use bardo_domain::VisionError;

pub(crate) struct StateWriter(Vec<u8>);

impl StateWriter {
    pub(crate) fn new(tag: &str) -> StateWriter {
        let mut writer = StateWriter(Vec::new());
        writer.bytes(tag.as_bytes());
        writer
    }

    pub(crate) fn u32(&mut self, value: u32) {
        self.0.extend_from_slice(&value.to_le_bytes());
    }

    pub(crate) fn bool(&mut self, value: bool) {
        self.0.push(u8::from(value));
    }

    pub(crate) fn f32(&mut self, value: f32) {
        self.0.extend_from_slice(&value.to_le_bytes());
    }

    pub(crate) fn f64(&mut self, value: f64) {
        self.0.extend_from_slice(&value.to_le_bytes());
    }

    pub(crate) fn floats(&mut self, values: &[f32]) {
        self.u32(values.len() as u32);
        for &value in values {
            self.f32(value);
        }
    }

    pub(crate) fn bytes(&mut self, values: &[u8]) {
        self.u32(values.len() as u32);
        self.0.extend_from_slice(values);
    }

    pub(crate) fn finish(self) -> Vec<u8> {
        self.0
    }
}

pub(crate) struct StateReader<'a>(&'a [u8]);

fn short() -> VisionError {
    VisionError::State("it ends too soon".to_owned())
}

impl<'a> StateReader<'a> {
    /// Reads a state written with `tag`.
    pub(crate) fn new(bytes: &'a [u8], tag: &str) -> Result<StateReader<'a>, VisionError> {
        let mut reader = StateReader(bytes);
        if reader.bytes()? != tag.as_bytes() {
            return Err(VisionError::State(format!("not a {tag} state")));
        }
        Ok(reader)
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], VisionError> {
        if self.0.len() < n {
            return Err(short());
        }
        let (head, rest) = self.0.split_at(n);
        self.0 = rest;
        Ok(head)
    }

    pub(crate) fn u32(&mut self) -> Result<u32, VisionError> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    pub(crate) fn bool(&mut self) -> Result<bool, VisionError> {
        Ok(self.take(1)?[0] != 0)
    }

    pub(crate) fn f32(&mut self) -> Result<f32, VisionError> {
        let b = self.take(4)?;
        Ok(f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    pub(crate) fn f64(&mut self) -> Result<f64, VisionError> {
        let b = self.take(8)?;
        let mut a = [0; 8];
        a.copy_from_slice(b);
        Ok(f64::from_le_bytes(a))
    }

    pub(crate) fn floats(&mut self) -> Result<Vec<f32>, VisionError> {
        let n = self.u32()? as usize;
        let raw = self.take(n.checked_mul(4).ok_or_else(short)?)?;
        Ok(raw
            .chunks_exact(4)
            .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .collect())
    }

    pub(crate) fn bytes(&mut self) -> Result<&'a [u8], VisionError> {
        let n = self.u32()? as usize;
        self.take(n)
    }

    /// Fails unless every byte was read.
    pub(crate) fn end(self) -> Result<(), VisionError> {
        if self.0.is_empty() {
            Ok(())
        } else {
            Err(VisionError::State("it has more than expected".to_owned()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_read_back_in_order() {
        let mut w = StateWriter::new("face");
        w.u32(7);
        w.bool(true);
        w.f32(-1.5);
        w.f64(0.25);
        w.floats(&[1.0, 2.0]);
        let bytes = w.finish();
        let mut r = StateReader::new(&bytes, "face").expect("tag");
        assert_eq!(r.u32().unwrap(), 7);
        assert!(r.bool().unwrap());
        assert_eq!(r.f32().unwrap(), -1.5);
        assert_eq!(r.f64().unwrap(), 0.25);
        assert_eq!(r.floats().unwrap(), [1.0, 2.0]);
        r.end().expect("all read");
        assert!(StateReader::new(&bytes, "depth").is_err());
        assert!(StateReader::new(&bytes[..3], "face").is_err());
    }
}
