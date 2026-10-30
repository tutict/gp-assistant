use serde::{Deserialize, Serialize};

use crate::{CoreError, CoreResult};

/// Wire units are declared by the field contract; magnitude never determines units.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RateUnit {
    Ratio,
    PercentagePoints,
}

/// A finite normalized rate. Ratios above one and negative ratios are valid.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "f64", into = "f64")]
pub struct RateRatio(f64);

impl RateRatio {
    pub fn from_unit(value: f64, unit: RateUnit) -> CoreResult<Self> {
        if !value.is_finite() {
            return Err(CoreError::new("rate must be finite"));
        }
        let ratio = match unit {
            RateUnit::Ratio => value,
            RateUnit::PercentagePoints => value / 100.0,
        };
        Ok(Self(ratio))
    }

    pub fn from_ratio(value: f64) -> CoreResult<Self> {
        Self::from_unit(value, RateUnit::Ratio)
    }

    pub fn from_percentage_points(value: f64) -> CoreResult<Self> {
        Self::from_unit(value, RateUnit::PercentagePoints)
    }

    pub fn as_ratio(self) -> f64 {
        self.0
    }

    pub fn as_percentage_points(self) -> CoreResult<f64> {
        let value = self.0 * 100.0;
        if value.is_finite() {
            Ok(value)
        } else {
            Err(CoreError::new("percentage-point conversion overflow"))
        }
    }
}

impl TryFrom<f64> for RateRatio {
    type Error = CoreError;
    fn try_from(value: f64) -> CoreResult<Self> {
        Self::from_ratio(value)
    }
}

impl From<RateRatio> for f64 {
    fn from(value: RateRatio) -> Self {
        value.as_ratio()
    }
}

pub(crate) fn ratio_percentage_points(value: f64) -> Option<f64> {
    RateRatio::from_ratio(value)
        .ok()?
        .as_percentage_points()
        .ok()
}

pub(crate) fn percentage_points(value: f64) -> Option<f64> {
    RateRatio::from_percentage_points(value).ok()?;
    Some(value)
}
