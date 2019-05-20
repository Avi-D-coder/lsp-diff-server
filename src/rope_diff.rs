use crate::chars_diff::{self, Changes, Incremental};
use diffs::{myers, Diff, Replace};
use hash_diff::*;
use lsp_types::{Position, Range, TextDocumentContentChangeEvent};
use passing_arg_iter::*;
use smallvec::SmallVec;

#[derive(Debug, Clone)]
pub struct LineRopeSlice<'a> {
    pub slice: ropey::RopeSlice<'a>,
    /// The line index that this slice was created at.
    pub absolute_index: usize,
}

#[derive(Clone)]
pub struct Rope(ropey::Rope);
impl<'a> Segments for &'a Rope {
    #[allow(clippy::type_complexity)]
    type Iter = std::iter::Map<
        std::iter::Enumerate<ropey::iter::Lines<'a>>,
        fn((usize, ropey::RopeSlice<'a>)) -> LineRopeSlice<'a>,
    >;
    type Segment = LineRopeSlice<'a>;
    fn segments(self) -> Self::Iter {
        self.0
            .lines()
            .enumerate()
            .map(|(absolute_index, slice)| LineRopeSlice {
                slice,
                absolute_index,
            })
    }
}

impl<'a> Segments for LineRopeSlice<'a> {
    #[allow(clippy::type_complexity)]
    type Iter = std::iter::Map<
        PassingArgument<std::iter::Enumerate<ropey::iter::Lines<'a>>, usize>,
        fn((usize, (usize, ropey::RopeSlice<'a>))) -> LineRopeSlice<'a>,
    >;
    type Segment = LineRopeSlice<'a>;
    fn segments(self) -> Self::Iter {
        fn rs(t: (usize, (usize, ropey::RopeSlice<'_>))) -> LineRopeSlice<'_> {
            let (offset, (index, slice)) = t;
            LineRopeSlice {
                slice,
                absolute_index: offset + index,
            }
        }

        self.slice
            .lines()
            .enumerate()
            .passing_arg(self.absolute_index)
            .map(rs)
    }
}

impl<'a> ContentPosition for LineRopeSlice<'a> {
    type Position = usize;
    fn pos(&self) -> Self::Position {
        self.absolute_index
    }
    type Content = ropey::RopeSlice<'a>;
    fn content(&self) -> Self::Content {
        self.slice
    }
}

pub struct Full<'o, 'n> {
    changes: Changes,
    old: LineRopeSlice<'o>,
    new: LineRopeSlice<'n>,
}

impl<'o, 'n> Full<'o, 'n> {
    pub fn diff(old: ropey::RopeSlice<'o>, new: &'n str) -> Changes {
        let new = LineRopeSlice {
            slice: new.into(),
            absolute_index: 0,
        };
        let old = LineRopeSlice {
            slice: old,
            absolute_index: 0,
        };

        if let Some(hashed) = old.clone().hash_changed(new.clone()) {
            let mut ld = Full {
                changes: SmallVec::new(),
                old,
                new,
            };

            myers::diff(
                &mut Replace::new(&mut ld),
                &hashed.changed_old,
                0,
                hashed.changed_old.len(),
                &hashed.changed_new,
                0,
                hashed.changed_new.len(),
            )
            .unwrap();
            ld.changes
        } else {
            SmallVec::new()
        }
    }
}

impl<'o, 'n> Diff for Full<'o, 'n> {
    type Error = ();
    fn delete(&mut self, old: usize, len: usize) -> Result<(), Self::Error> {
        self.changes.push(TextDocumentContentChangeEvent {
            range: Some(Range {
                start: Position::new((self.old.absolute_index + old) as u64, 0),
                end: Position::new((self.old.absolute_index + old + len) as u64, 0),
            }),
            range_length: None,
            text: "".to_owned(),
        });
        Ok(())
    }

    fn insert(&mut self, old: usize, new: usize, new_len: usize) -> Result<(), Self::Error> {
        let text = ropey::RopeSlice::from(self.new.slice.lines().narrow(new..new + new_len));
        let start = Position::new((self.old.absolute_index + old) as u64, 0);

        self.changes.push(TextDocumentContentChangeEvent {
            range: Some(Range { start, end: start }),
            range_length: Some(0),
            text: text.into(),
        });
        Ok(())
    }

    fn replace(
        &mut self,
        old: usize,
        old_len: usize,
        new: usize,
        new_len: usize,
    ) -> Result<(), Self::Error> {
        let text = ropey::RopeSlice::from(self.new.slice.lines().narrow(new..new + new_len));

        Incremental::diff(
            chars_diff::RopeSlice {
                slice: self.old.slice.lines().narrow(old..old + old_len).into(),
                absolute_pos: ropey::Position {
                    line: self.old.absolute_index + old,
                    character: 0,
                },
            },
            ropey::RopeSlice::from(self.new.slice.lines().narrow(old..old + old_len))
                .as_str()
                .unwrap(),
        );

        self.changes.push(TextDocumentContentChangeEvent {
            range: Some(Range {
                start: Position::new((self.old.absolute_index + old) as u64, 0),
                end: Position::new((self.old.absolute_index + old + old_len) as u64, 0),
            }),
            range_length: None,
            text: text.into(),
        });
        Ok(())
    }
}
