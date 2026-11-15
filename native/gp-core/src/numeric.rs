pub(crate) fn round2(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

pub(crate) fn round6(value: f64) -> f64 {
    (value * 1_000_000.0).round() / 1_000_000.0
}

pub(crate) fn round4(value: f64) -> f64 {
    (value * 10_000.0).round() / 10_000.0
}

pub(crate) fn finite_round4(value: f64) -> Option<f64> {
    if value.is_finite() {
        Some(round4(value))
    } else {
        None
    }
}

pub(crate) fn ema(values: &[f64], span: usize) -> Vec<f64> {
    let alpha = 2.0 / (span as f64 + 1.0);
    let mut output = Vec::with_capacity(values.len());
    let mut previous = None;
    for value in values {
        let current = match previous {
            Some(previous_value) => alpha * *value + (1.0 - alpha) * previous_value,
            None => *value,
        };
        previous = Some(current);
        output.push(current);
    }
    output
}
