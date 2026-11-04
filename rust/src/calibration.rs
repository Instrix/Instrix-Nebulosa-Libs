//! Converting 14-bit ADC codes to amps and volts.

/// Per-range current scale and offset, voltage scale and offset.
///
/// `current = code * i_fs[range] / 16384 + i_offset[range]`,
/// `voltage = code * v_scale + v_offset`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Calibration {
    /// Full-scale current of each range, amps (range 0 = 2 A .. 5 = 20 uA).
    pub i_fs: [f64; 8],
    /// Offset of each range, amps.
    pub i_offset: [f64; 8],
    /// Volts per voltage code.
    pub v_scale: f64,
    /// Voltage offset, volts.
    pub v_offset: f64,
}

impl Default for Calibration {
    fn default() -> Self {
        Self {
            i_fs: [2.0, 0.2, 20e-3, 2e-3, 200e-6, 20e-6, 0.0, 0.0],
            i_offset: [0.0; 8],
            v_scale: 1e-3,
            v_offset: 0.0,
        }
    }
}

impl Calibration {
    /// Amps of a current code in `range`.
    #[inline]
    pub fn current(&self, code: u16, range: u8) -> f64 {
        let r = (range & 7) as usize;
        code as f64 * self.i_fs[r] * (1.0 / 16384.0) + self.i_offset[r]
    }

    /// Volts of a voltage code.
    #[inline]
    pub fn voltage(&self, code: u16) -> f64 {
        code as f64 * self.v_scale + self.v_offset
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_scales() {
        let c = Calibration::default();
        assert_eq!(c.current(8192, 0), 1.0);
        assert!((c.current(16384, 5) - 20e-6).abs() < 1e-18);
        assert_eq!(c.current(100, 6), 0.0);
        assert!((c.voltage(3300) - 3.3).abs() < 1e-12);
    }
}
