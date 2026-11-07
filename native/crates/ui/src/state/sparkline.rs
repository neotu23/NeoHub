//! CPU sparklines for the dashboard cards.
//!
//! Drawn as block characters in a text run rather than a path: it costs no GPU state, survives
//! any font the user has, and reads correctly at the one size these cards use. When the UI grows
//! a real chart this module is the only thing that goes away.
//!
//! Two rules make the picture honest:
//!
//! * the scale is **per series** (each card against its own recent maximum), because absolute
//!   percentages make a quiet project look dead next to a busy one;
//! * an idle series draws a flat line, not a rising one — a missing sample must not look like
//!   growth.

use std::collections::VecDeque;

/// Block characters from lowest to highest.
const BLOCKS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

/// A fixed-length history of one metric.
#[derive(Clone, Debug, PartialEq)]
pub struct Sparkline {
    samples: VecDeque<f32>,
    capacity: usize,
}

impl Sparkline {
    pub fn new(capacity: usize) -> Self {
        Sparkline {
            samples: VecDeque::with_capacity(capacity.max(1)),
            capacity: capacity.max(1),
        }
    }

    /// Record a sample, dropping the oldest once full.
    pub fn push(&mut self, value: f32) {
        // Negative or NaN values are not meaningful for CPU or memory; clamp rather than poison
        // the scale for every later sample.
        let value = if value.is_finite() {
            value.max(0.0)
        } else {
            0.0
        };
        if self.samples.len() == self.capacity {
            self.samples.pop_front();
        }
        self.samples.push_back(value);
    }

    pub fn len(&self) -> usize {
        self.samples.len()
    }

    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    /// Highest sample so far (the scale's top).
    pub fn peak(&self) -> f32 {
        self.samples.iter().copied().fold(0.0, f32::max)
    }

    /// Most recent sample.
    pub fn last(&self) -> Option<f32> {
        self.samples.back().copied()
    }

    /// Render at most `width` blocks, newest on the right.
    pub fn render(&self, width: usize) -> String {
        if self.samples.is_empty() || width == 0 {
            return String::new();
        }
        // A flat series (all zeros) draws the lowest block so the card keeps its height instead of
        // jumping when traffic starts.
        let peak = self.peak();
        let visible: Vec<f32> = self
            .samples
            .iter()
            .copied()
            .skip(self.samples.len().saturating_sub(width))
            .collect();
        visible
            .iter()
            .map(|value| {
                if peak <= f32::EPSILON {
                    BLOCKS[0]
                } else {
                    let ratio = (value / peak).clamp(0.0, 1.0);
                    let index = ((ratio * (BLOCKS.len() - 1) as f32).round() as usize)
                        .min(BLOCKS.len() - 1);
                    BLOCKS[index]
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_window_keeps_the_newest_samples() {
        let mut line = Sparkline::new(3);
        for value in [1.0, 2.0, 3.0, 4.0] {
            line.push(value);
        }
        assert_eq!(line.len(), 3, "the oldest sample is dropped");
        assert_eq!(line.last(), Some(4.0));
        assert_eq!(line.peak(), 4.0);
    }

    #[test]
    fn an_empty_series_renders_nothing() {
        let line = Sparkline::new(10);
        assert!(line.is_empty());
        assert_eq!(line.render(10), "");
        assert_eq!(line.render(0), "");
    }

    #[test]
    fn the_peak_draws_the_tallest_block_and_zero_the_lowest() {
        let mut line = Sparkline::new(4);
        line.push(0.0);
        line.push(50.0);
        line.push(100.0);
        let rendered = line.render(3);
        assert_eq!(rendered.chars().count(), 3);
        assert_eq!(
            rendered.chars().next(),
            Some(BLOCKS[0]),
            "zero is the floor"
        );
        assert_eq!(rendered.chars().last(), Some(BLOCKS[7]), "the peak is full");
    }

    #[test]
    fn a_flat_series_stays_flat() {
        let mut line = Sparkline::new(4);
        for _ in 0..4 {
            line.push(0.0);
        }
        let rendered = line.render(4);
        assert_eq!(rendered.chars().count(), 4);
        assert!(
            rendered.chars().all(|character| character == BLOCKS[0]),
            "an idle card must not draw growth: {rendered}"
        );
    }

    #[test]
    fn a_constant_non_zero_series_is_also_flat() {
        let mut line = Sparkline::new(3);
        for _ in 0..3 {
            line.push(7.5);
        }
        let rendered = line.render(3);
        assert!(
            rendered.chars().all(|character| character == BLOCKS[7]),
            "with peak == value everything is full: {rendered}"
        );
    }

    #[test]
    fn rendering_a_width_narrower_than_the_history_keeps_the_newest() {
        let mut line = Sparkline::new(6);
        for value in [1.0, 2.0, 3.0, 4.0, 5.0, 6.0] {
            line.push(value);
        }
        let rendered = line.render(3);
        assert_eq!(rendered.chars().count(), 3);
        // The newest sample is the peak, so the last block is full.
        assert_eq!(rendered.chars().last(), Some(BLOCKS[7]));
    }

    #[test]
    fn nonsense_samples_do_not_poison_the_scale() {
        let mut line = Sparkline::new(3);
        line.push(f32::NAN);
        line.push(-5.0);
        line.push(f32::INFINITY);
        assert_eq!(line.peak(), 0.0, "nothing valid was recorded above zero");
        assert_eq!(line.render(3).chars().count(), 3);
    }

    #[test]
    fn a_zero_capacity_still_holds_one_sample() {
        let mut line = Sparkline::new(0);
        line.push(1.0);
        line.push(2.0);
        assert_eq!(line.len(), 1);
        assert_eq!(line.last(), Some(2.0));
    }
}
