use crate::tree::TextWrap;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct MixedInlineLine {
    pub top: f32,
    pub width: f32,
    pub ascent: f32,
    pub descent: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct MixedInlineMeasurement {
    pub width: f32,
    pub height: f32,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct MixedInlineBreaker {
    wrap_width: Option<f32>,
    base_ascent: f32,
    base_descent: f32,
    line_height: f32,
    x: f32,
    line_top: f32,
    line_ascent: f32,
    line_descent: f32,
    max_width: f32,
    total_height: f32,
}

impl MixedInlineBreaker {
    pub(crate) fn new(
        wrap: TextWrap,
        wrap_width: Option<f32>,
        base_ascent: f32,
        base_descent: f32,
        line_height: f32,
    ) -> Self {
        Self {
            wrap_width: if matches!(wrap, TextWrap::Wrap) {
                wrap_width
            } else {
                None
            },
            base_ascent,
            base_descent,
            line_height,
            x: 0.0,
            line_top: 0.0,
            line_ascent: base_ascent,
            line_descent: base_descent,
            max_width: 0.0,
            total_height: 0.0,
        }
    }

    pub(crate) fn x(&self) -> f32 {
        self.x
    }

    pub(crate) fn line_is_empty(&self) -> bool {
        self.x == 0.0
    }

    pub(crate) fn skips_leading_space(&self, is_space: bool) -> bool {
        is_space && self.line_is_empty()
    }

    pub(crate) fn wraps_before(&self, is_space: bool, width: f32) -> bool {
        self.wrap_width
            .is_some_and(|limit| !is_space && self.x > 0.0 && self.x + width > limit)
    }

    pub(crate) fn skips_overflowing_space(&self, is_space: bool, width: f32) -> bool {
        self.wrap_width
            .is_some_and(|limit| is_space && self.x + width > limit)
    }

    pub(crate) fn push(&mut self, width: f32, ascent: f32, descent: f32) {
        self.x += width;
        self.line_ascent = self.line_ascent.max(ascent);
        self.line_descent = self.line_descent.max(descent);
    }

    pub(crate) fn finish_line(&mut self) -> MixedInlineLine {
        let line = MixedInlineLine {
            top: self.line_top,
            width: self.x,
            ascent: self.line_ascent,
            descent: self.line_descent,
        };
        self.max_width = self.max_width.max(self.x);
        self.total_height += self.line_advance();
        self.line_top = self.total_height;
        self.x = 0.0;
        self.line_ascent = self.base_ascent;
        self.line_descent = self.base_descent;
        line
    }

    pub(crate) fn finish(mut self) -> MixedInlineMeasurement {
        self.finish_line();
        MixedInlineMeasurement {
            width: self
                .wrap_width
                .map(|limit| self.max_width.min(limit))
                .unwrap_or(self.max_width),
            height: self.total_height,
        }
    }

    fn line_advance(&self) -> f32 {
        (self.line_ascent + self.line_descent).max(self.line_height)
    }
}
