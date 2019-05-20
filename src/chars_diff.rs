use std::ops::Index;

use diffs::{myers, Diff, Replace};
use lsp_types::{Position, Range, TextDocumentContentChangeEvent};
use ropey::str_utils::count_line_breaks;
use smallvec::SmallVec;

pub type Changes = SmallVec<[TextDocumentContentChangeEvent; 12]>;
#[derive(Debug, Clone)]
pub struct RopeSlice<'a> {
    pub slice: ropey::RopeSlice<'a>,
    /// The line index that this slice was created at.
    pub absolute_pos: ropey::Position,
}

pub struct Incremental<'o, 'n> {
    changes: Changes,
    old: RopeSlice<'o>,
    new: &'n str,
    char_offset: isize,
    line_offset: isize,
    on_line: usize,
}

impl<'o, 'n> Incremental<'o, 'n> {
    pub fn diff(old: RopeSlice<'o>, new: &'n str) -> Changes {
        let mut cd = Incremental {
            new,
            old: old.clone(),
            changes: SmallVec::with_capacity(10),
            line_offset: old.absolute_pos.line as isize,
            char_offset: old.absolute_pos.character as isize,
            on_line: 0,
        };

        myers::diff(
            &mut Replace::new(&mut cd),
            &old.slice,
            0,
            old.slice.len_bytes(),
            &Str(new),
            0,
            new.len(),
        )
        .unwrap();
        cd.changes
    }
}

struct Str<'l>(&'l str);
impl<'l> Index<usize> for Str<'l> {
    type Output = u8;
    fn index(&self, idx: usize) -> &u8 {
        self.0.as_bytes().get(idx).unwrap()
    }
}

impl<'o, 'n> Diff for Incremental<'o, 'n> {
    type Error = ();
    fn delete(&mut self, old: usize, len: usize) -> Result<(), Self::Error> {
        dbg!("\n\n DELETE");
        dbg!(old);
        dbg!(len);
        dbg!(self.char_offset);
        let start = self.old.slice.byte_to_position(old);

        // This requires `old` indices to be ordered.
        if start.line != self.on_line {
            self.on_line = start.line;
            self.char_offset = 0;
        };

        debug_assert_eq!(self.old.slice.byte_to_line(old), start.line);
        dbg!(
            String::from(self.old.slice.line(self.old.slice.byte_to_line(old)))
                .split_at(start.character as usize)
        );

        let start = Position::new(
            dbg!(self.line_offset + start.line as isize) as u64,
            dbg!(self.char_offset + start.character as isize) as u64,
        );

        let end = self.old.slice.byte_to_position(old + len);

        debug_assert_eq!(self.old.slice.byte_to_line(old + len), end.line);

        dbg!(
            String::from(self.old.slice.line(self.old.slice.byte_to_line(old + len)))
                .split_at(end.character as usize)
        );
        let end = Position::new(
            dbg!(self.line_offset + end.line as isize) as u64,
            dbg!(self.char_offset + end.character as isize) as u64,
        );

        self.changes.push(dbg!(TextDocumentContentChangeEvent {
            range: Some(Range { start, end }),
            range_length: None,
            text: "".to_owned(),
        }));

        // foo\nbar\nbuzz
        // delete: {line: 1, char: 2} .. {line: 2, char: 3} = "r\nbuz"
        // foo\nbaz
        // self.line_offset -= 2 - 1; // == -1
        // self.char_offset -= 3 - 2; // == -1
        //
        // "foo\nbar\nbuzz"[11] == 'z'; // {line: 2 char: 3}
        // "foo\nbaz"[{line: 2 + (-1), char: 3 + (-1)}] == 'z'

        // foobarbazz
        // delete: {line: 0, char: 3} ..{line: 0, char: 6}
        // foobazz
        // self.line_offset -= 0 - 0;
        // self.char_offset -= 6 - 3; // == -3
        // "foobazz"[{line: 0 + 0, char: 7 + (-3)}] == 'a'

        self.line_offset -= (end.line - start.line) as isize;
        self.char_offset -= end.character as isize - start.character as isize;

        Ok(())
    }

    fn insert(&mut self, old: usize, new: usize, new_len: usize) -> Result<(), Self::Error> {
        dbg!("\n\n INSERT");
        dbg!(old);
        dbg!(new);
        dbg!(new_len);
        dbg!(self.char_offset);
        let start = self.old.slice.byte_to_position(old);

        // This requires `old` indices to be ordered.
        if start.line != self.on_line {
            self.on_line = start.line;
            self.char_offset = 0;
        };

        debug_assert_eq!(self.old.slice.byte_to_line(old), start.line);
        dbg!(
            String::from(self.old.slice.line(self.old.slice.byte_to_line(old)))
                .split_at(start.character as usize)
        );

        let start = Position::new(
            dbg!(self.line_offset + start.line as isize) as u64,
            dbg!(self.char_offset + start.character as isize) as u64,
        );

        let new = pre_char_boundary(self.new, new);
        let new_end = pre_char_boundary(self.new, new + new_len);

        let text = &self.new[new..new_end];

        self.changes.push(dbg!(TextDocumentContentChangeEvent {
            range: Some(Range { start, end: start }),
            range_length: Some(0),
            text: dbg!(text.to_owned()),
        }));

        // foo\nbar\nbuzz
        // insert: {line: 1, char: 2} "rrr"
        // foo\nbarrrr\nbuzz
        // self.line_offset += count_lines("rrr"); // == 0
        // if count_lines("rrr") > 0 { // false
        //   self.char_offset = "rrr".lines().last().len() - 2; // == 1
        // } else {
        //   self.char_offset += "rrr".len(); // == 3
        // }
        //
        // "foo\nbar\nbuzz"[7] == '\n'; // {line: 1 char: 3}
        // "foo\nbarrrr\nbuzz"[{line: 1 + 0, char: 3 + 3}] == '\n'

        // foobarbazz
        // insert: {line: 0, char: 3} "\nfo"
        // foo\nfobarbazz
        // self.line_offset += count_lines("\nfo"); // == 1
        // if count_lines("\nfo") > 0 { // true
        //   self.char_offset = "\nfo".lines().last().len() - 3; // == -1
        // } else {
        //   self.char_offset += "\nfo".len(); // == 3
        // }
        //
        // "foobarbazz"[4] == 'a'; // {line: 0 char: 4}
        // "foo\nfobarbazz"[{line: 0 + 1, char: 4 + (-1)}] == 'a'
        //
        // "foobarbazz"[9] == 'z'; // {line: 0 char: 9}
        // "foo\nfobarbazz"[{line: 0 + 1, char: 9 + (-1)}] == 'z'

        let new_line_count = count_line_breaks(text) as isize;
        self.line_offset += new_line_count;
        if new_line_count > 0 {
            // false
            self.char_offset =
                text.lines().rev().next().unwrap().len() as isize - start.line as isize;
        } else {
            self.char_offset += text.len() as isize;
        };
        Ok(())
    }
    fn replace(
        &mut self,
        old: usize,
        old_len: usize,
        new: usize,
        new_len: usize,
    ) -> Result<(), Self::Error> {
        dbg!("REPLACE");
        dbg!(old);
        dbg!(old_len);
        dbg!(new);
        dbg!(new_len);
        dbg!(self.char_offset);

        let start = self.old.slice.byte_to_position(old);

        // This requires `old` indices to be ordered.
        if start.line != self.on_line {
            self.on_line = start.line;
            self.char_offset = 0;
        };

        dbg!(
            String::from(self.old.slice.line(self.old.slice.byte_to_line(old)))
                .split_at(start.character as usize)
        );

        let start = Position::new(
            dbg!(self.line_offset + start.line as isize) as u64,
            dbg!(self.char_offset + start.character as isize) as u64,
        );
        let end = self.old.slice.byte_to_position(old + old_len);

        dbg!(String::from(
            self.old
                .slice
                .line(self.old.slice.byte_to_line(old + old_len))
        )
        .split_at(end.character as usize));

        let end = Position::new(
            dbg!(self.line_offset + end.line as isize) as u64,
            dbg!(self.char_offset + end.character as isize) as u64,
        );

        let new = pre_char_boundary(self.new, new);
        let new_end = pre_char_boundary(self.new, new + new_len);

        let text = &self.new[new..new_end];

        self.changes.push(dbg!(TextDocumentContentChangeEvent {
            range: Some(Range { start, end }),
            range_length: None,
            text: dbg!(text.to_owned()),
        }));

        // foo\nbar\nbuzz
        // delete: {line: 1, char: 2} .. {line: 2, char: 3} = "r\nbuz"; insert "rrr"
        // foo\nbarrrz
        // self.line_offset += "rrr".lines().count() - (2 - 1); // == -1
        // let char_delete_offset = ; // == -1
        // if count_lines("rrr") > 0 { // false
        //   self.char_offset = "rrr".lines().last().len() - 3; // == -6
        // } else {
        //   self.char_offset += "rrr".len() + 2 - 3; // == 2
        // }
        //
        // "foo\nbar\nbuzz"[11] == 'z'; // {line: 2 char: 3}
        // "foo\nbarrrz"[{line: 2 + (-1), char: 3 + 2}] == 'z'

        // foobarbazz
        // delete: {line: 0, char: 3} .. {line: 0, char: 6} = "bar"; insert "lol\n"
        // foolol\nbazz
        // self.line_offset += "lol\n".lines().count() - (0 - 0); // == 1
        // if count_lines("lol\n") > 0 { // true
        //   self.char_offset = "lol\n".lines().last().len() - 6; // == -6
        // } else {
        //   self.char_offset += "lol\n".len() + 3 - 6; // == 1
        // }
        //
        // foobarbazz[7] = 'a' {line 0, char: 7}
        // "foolol\nbazz"[{line: 0 + 1, char: 7 +3 - 6 (-6)}] == 'a'

        // foobarbazz
        // delete: {line: 0, char: 3} .. {line: 03 - 6, char: 6} = "bar"; insert "lol\nz"
        // foolol\nbazz
        // self.line_offset += "lol\nz".lines().count() - (0 - 0); // == 1
        // if count_lines("lol\nz") > 0 { // true
        //   self.char_offset = "lol\nz".lines().last().len() - 6; // == -5
        // } else {
        //   self.char_offset += "lol\nz".len() + 3 - 6; // == 2
        // }
        //
        // "foobarbazz"[7] = 'a' {line 0, char: 7}
        // "foolol\nzbazz"[{line: 0 + 1, char: 7 + (-5)}] == 'a'

        let new_line_count = count_line_breaks(text) as isize;
        self.line_offset += new_line_count;
        if dbg!(new_line_count) > 0 {
            self.char_offset =
                dbg!(text.lines().rev().next().unwrap().len() as isize - end.character as isize);
        } else {
            self.char_offset +=
                dbg!(text.len() as isize + start.character as isize - end.character as isize);
        };
        Ok(())
    }
}

fn pre_char_boundary(s: &str, mut idx: usize) -> usize {
    while !s.is_char_boundary(idx) {
        idx -= 1
    }
    idx
}
