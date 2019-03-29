use hash_diff::*;
use passing_arg_iter::*;

#[derive(Clone)]
pub struct Rope(ropey::Rope);
impl<'a> Segments for &'a Rope {
    #[allow(clippy::type_complexity)]
    type Iter = std::iter::Map<
        std::iter::Enumerate<ropey::iter::Lines<'a>>,
        fn((usize, ropey::RopeSlice<'a>)) -> RopeSegment<'a>,
    >;
    type Segment = RopeSegment<'a>;
    fn segments(self) -> Self::Iter {
        self.0
            .lines()
            .enumerate()
            .map(|(absolute_index, slice)| RopeSegment {
                slice,
                absolute_index,
            })
    }
}

#[derive(Clone)]
pub struct RopeSlice<'a> {
    slice: ropey::RopeSlice<'a>,
    /// The line index that this slice was created at.
    at_index: usize,
}

impl<'a> Segments for &RopeSlice<'a> {
    #[allow(clippy::type_complexity)]
    type Iter = std::iter::Map<
        PassingArgument<std::iter::Enumerate<ropey::iter::Lines<'a>>, usize>,
        fn((usize, (usize, ropey::RopeSlice<'a>))) -> RopeSegment<'a>,
    >;
    type Segment = RopeSegment<'a>;
    fn segments(self) -> Self::Iter {
        fn rs(t: (usize, (usize, ropey::RopeSlice<'_>))) -> RopeSegment<'_> {
            let (offset, (index, slice)) = t;
            RopeSegment {
                slice,
                absolute_index: offset + index,
            }
        }

        self.slice
            .lines()
            .enumerate()
            .passing_arg(self.at_index)
            .map(rs)
    }
}

#[derive(Debug, Clone)]
pub struct RopeSegment<'a> {
    slice: ropey::RopeSlice<'a>,
    /// The line index this `RopeSlice`: `R` is found in the original `Rope`.
    absolute_index: usize,
}

impl<'a> ContentPosition for RopeSegment<'a> {
    type Position = usize;
    fn pos(&self) -> Self::Position {
        self.absolute_index
    }
    type Content = ropey::RopeSlice<'a>;
    fn content(&self) -> Self::Content {
        self.slice
    }
}
