use serde::Serialize;
use std::collections::VecDeque;
#[derive(Debug, Clone, Default, Serialize)]
pub struct Distribution {
    pub samples: usize,
    pub mean_ms: f64,
    pub p95_ms: f64,
    pub max_ms: f64,
}
#[derive(Clone, Default)]
pub struct Timings {
    samples: VecDeque<f64>,
}
impl Timings {
    pub fn record(&mut self, ms: f64) {
        if self.samples.len() == 2048 {
            self.samples.pop_front();
        }
        self.samples.push_back(ms);
    }
    pub fn report(&self) -> Distribution {
        let mut v: Vec<_> = self.samples.iter().copied().collect();
        if v.is_empty() {
            return Distribution::default();
        }
        v.sort_by(f64::total_cmp);
        Distribution {
            samples: v.len(),
            mean_ms: v.iter().sum::<f64>() / v.len() as f64,
            p95_ms: v[((v.len() - 1) as f64 * 0.95).ceil() as usize],
            max_ms: *v.last().unwrap(),
        }
    }
}
