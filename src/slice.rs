#![allow(dead_code)]

use std;
use std::sync::Arc;

use iter::{Bytes, Chars, Chunks, Graphemes, Lines};
use tree::{Count, Node};
use rope::Rope;

/// An immutable view into part of a `Rope`.
#[derive(Copy, Clone)]
pub struct RopeSlice<'a> {
    node: &'a Arc<Node>,
    start_byte: Count,
    end_byte: Count,
    start_char: Count,
    end_char: Count,
    start_line_break: Count,
    end_line_break: Count,
}

impl<'a> RopeSlice<'a> {
    pub(crate) fn new(node: &Arc<Node>) -> RopeSlice {
        RopeSlice {
            node: node,
            start_byte: 0,
            end_byte: node.text_info().bytes,
            start_char: 0,
            end_char: node.text_info().chars,
            start_line_break: 0,
            end_line_break: node.text_info().line_breaks,
        }
    }

    pub(crate) fn new_with_range(node: &Arc<Node>, start: usize, end: usize) -> RopeSlice {
        assert!(start <= end);
        assert!(end <= node.text_info().chars as usize);

        // Find the deepest node that still contains the full range given.
        let mut n_start = start;
        let mut n_end = end;
        let mut node = node;
        'outer: loop {
            match *(node as &Node) {
                Node::Leaf(_) => break,

                Node::Internal(ref children) => {
                    let mut start_char = 0;
                    for (i, inf) in children.info().iter().enumerate() {
                        if n_start >= start_char && n_end < (start_char + inf.chars as usize) {
                            n_start -= start_char;
                            n_end -= start_char;
                            node = &children.nodes()[i];
                            continue 'outer;
                        }
                        start_char += inf.chars as usize;
                    }
                    break;
                }
            }
        }

        // Create the slice
        RopeSlice {
            node: node,
            start_byte: node.char_to_byte(n_start) as Count,
            end_byte: node.char_to_byte(n_end) as Count,
            start_char: n_start as Count,
            end_char: n_end as Count,
            start_line_break: node.char_to_line(n_start) as Count,
            end_line_break: node.char_to_line(n_end) as Count,
        }
    }

    //-----------------------------------------------------------------------
    // Informational methods

    /// Total number of bytes in the `RopeSlice`.
    pub fn len_bytes(&self) -> usize {
        (self.end_byte - self.start_byte) as usize
    }

    /// Total number of chars in the `RopeSlice`.
    pub fn len_chars(&self) -> usize {
        (self.end_char - self.start_char) as usize
    }

    /// Total number of lines in the `RopeSlice`.
    pub fn len_lines(&self) -> usize {
        (self.end_line_break - self.start_line_break) as usize + 1
    }

    //-----------------------------------------------------------------------
    // Index conversion methods

    /// Returns the line index of the given char.
    ///
    /// # Panics
    ///
    /// Panics if `char_idx` is out of bounds (i.e. `char_idx > len_chars()`).
    pub fn char_to_line(&self, char_idx: usize) -> usize {
        // Bounds check
        assert!(
            char_idx <= self.len_chars(),
            "Attempt to index past end of slice: char index {}, slice char length {}",
            char_idx,
            self.len_chars()
        );

        if char_idx == self.len_chars() {
            self.len_lines()
        } else {
            self.node.char_to_line(self.start_char as usize + char_idx)
                - (self.start_line_break as usize)
        }
    }

    /// Returns the char index of the start of the given line.
    ///
    /// Note: lines are zero-indexed.
    ///
    /// # Panics
    ///
    /// Panics if `line_idx` is out of bounds (i.e. `line_idx > len_lines()`).
    pub fn line_to_char(&self, line_idx: usize) -> usize {
        // Bounds check
        assert!(
            line_idx <= self.len_lines(),
            "Attempt to index past end of slice: line index {}, slice line length {}",
            line_idx,
            self.len_lines()
        );

        if line_idx == self.len_lines() {
            self.len_chars()
        } else {
            let raw_char_idx = self.node
                .line_to_char(self.start_line_break as usize + line_idx);

            if raw_char_idx < (self.start_char as usize) {
                0
            } else {
                raw_char_idx - self.start_char as usize
            }
        }
    }

    //-----------------------------------------------------------------------
    // Fetch methods

    /// Returns the char at `char_idx`.
    ///
    /// # Panics
    ///
    /// Panics if `char_idx` is out of bounds (i.e. `char_idx >= len_chars()`).
    pub fn char(&self, char_idx: usize) -> char {
        // Bounds check
        assert!(
            char_idx < self.len_chars(),
            "Attempt to index past end of slice: char index {}, slice char length {}",
            char_idx,
            self.len_chars()
        );

        // TODO: make this more efficient.
        self.slice(char_idx, char_idx + 1).chars().nth(0).unwrap()
    }

    /// Returns the line at `line_idx`.
    ///
    /// Note: lines are zero-indexed.
    ///
    /// # Panics
    ///
    /// Panics if `line_idx` is out of bounds (i.e. `line_idx >= len_lines()`).
    pub fn line(&self, line_idx: usize) -> RopeSlice<'a> {
        // Bounds check
        assert!(
            line_idx < self.len_lines(),
            "Attempt to index past end of slice: line index {}, slice line length {}",
            line_idx,
            self.len_lines()
        );

        let start = self.line_to_char(line_idx);
        let end = self.line_to_char(line_idx + 1);

        self.slice(start, end)
    }

    //-----------------------------------------------------------------------
    // Grapheme methods

    /// Returns whether `char_idx` is a grapheme cluster boundary or not.
    ///
    /// # Panics
    ///
    /// Panics if `char_idx` is out of bounds (i.e. `char_idx > len_chars()`).
    pub fn is_grapheme_boundary(&self, char_idx: usize) -> bool {
        // Bounds check
        assert!(
            char_idx <= self.len_chars(),
            "Attempt to index past end of slice: char index {}, slice char length {}",
            char_idx,
            self.len_chars()
        );

        if char_idx == 0 || char_idx == self.len_chars() {
            true
        } else {
            self.node
                .is_grapheme_boundary(self.start_char as usize + char_idx)
        }
    }

    /// Returns the char index of the grapheme cluster boundary to the left
    /// of `char_idx`.
    ///
    /// This excludes any boundary that might be at `char_idx` itself, unless
    /// `char_idx` is at the beginning of the rope.
    ///
    /// # Panics
    ///
    /// Panics if `char_idx` is out of bounds (i.e. `char_idx > len_chars()`).
    pub fn prev_grapheme_boundary(&self, char_idx: usize) -> usize {
        // Bounds check
        assert!(
            char_idx <= self.len_chars(),
            "Attempt to index past end of slice: char index {}, slice char length {}",
            char_idx,
            self.len_chars()
        );

        let boundary_idx = self.node
            .prev_grapheme_boundary(self.start_char as usize + char_idx);
        if boundary_idx < self.start_char as usize {
            0
        } else {
            boundary_idx - self.start_char as usize
        }
    }

    /// Returns the char index of the grapheme cluster boundary to the right
    /// of `char_idx`.
    ///
    /// This excludes any boundary that might be at `char_idx` itself, unless
    /// `char_idx` is at the end of the rope.
    ///
    /// # Panics
    ///
    /// Panics if `char_idx` is out of bounds (i.e. `char_idx > len_chars()`).
    pub fn next_grapheme_boundary(&self, char_idx: usize) -> usize {
        // Bounds check
        assert!(
            char_idx <= self.len_chars(),
            "Attempt to index past end of slice: char index {}, slice char length {}",
            char_idx,
            self.len_chars()
        );

        let boundary_idx = self.node
            .next_grapheme_boundary(self.start_char as usize + char_idx);
        if boundary_idx >= self.end_char as usize {
            self.len_chars()
        } else {
            boundary_idx - self.start_char as usize
        }
    }

    //-----------------------------------------------------------------------
    // Slicing

    /// Returns an immutable slice of the `RopeSlice` in the char range `start..end`.
    ///
    /// # Panics
    ///
    /// Panics if `start` is greater than `end` or `end` is out of bounds
    /// (i.e. `end > len_chars()`).
    pub fn slice(&self, start: usize, end: usize) -> RopeSlice<'a> {
        // Bounds check
        assert!(start <= end);
        assert!(
            end <= self.len_chars(),
            "Attempt to slice past end of RopeSlice: slice end {}, RopeSlice length {}",
            end,
            self.len_chars()
        );

        RopeSlice::new_with_range(
            self.node,
            self.start_char as usize + start,
            self.start_char as usize + end,
        )
    }

    //-----------------------------------------------------------------------
    // Iterator methods

    /// Creates an iterator over the bytes of the `RopeSlice`.
    pub fn bytes(&self) -> Bytes<'a> {
        Bytes::new_with_range(self.node, self.start_char as usize, self.end_char as usize)
    }

    /// Creates an iterator over the chars of the `RopeSlice`.
    pub fn chars(&self) -> Chars<'a> {
        Chars::new_with_range(self.node, self.start_char as usize, self.end_char as usize)
    }

    /// Creates an iterator over the grapheme clusters of the `RopeSlice`.
    pub fn graphemes(&self) -> Graphemes<'a> {
        Graphemes::new_with_range(
            self.node,
            true,
            self.start_char as usize,
            self.end_char as usize,
        )
    }

    /// Creates an iterator over the lines of the `RopeSlice`.
    pub fn lines(&self) -> Lines<'a> {
        Lines::new_with_range(self.node, self.start_char as usize, self.end_char as usize)
    }

    /// Creates an iterator over the chunks of the `RopeSlice`.
    pub fn chunks(&self) -> Chunks<'a> {
        Chunks::new_with_range(self.node, self.start_char as usize, self.end_char as usize)
    }

    //-----------------------------------------------------------------------
    // Conversion methods

    /// Returns the entire text of the `RopeSlice` as a newly allocated `String`.
    pub fn to_string(&self) -> String {
        let mut text = String::with_capacity(self.len_bytes());
        for chunk in self.chunks() {
            text.push_str(chunk);
        }
        text
    }

    /// Creates a new `Rope` from the contents of the `RopeSlice`.
    pub fn to_rope(&self) -> Rope {
        let mut rope = Rope {
            root: Arc::clone(self.node),
        };

        // Chop off right end if needed
        if self.end_char < self.node.text_info().chars {
            rope.split_off(self.end_char as usize);
        }

        // Chop off left end if needed
        if self.start_char > 0 {
            rope = rope.split_off(self.start_char as usize);
        }

        // Return the rope
        rope
    }
}

//==============================================================

impl<'a> std::fmt::Debug for RopeSlice<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.debug_list().entries(self.chunks()).finish()
    }
}

impl<'a> std::fmt::Display for RopeSlice<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        for chunk in self.chunks() {
            write!(f, "{}", chunk)?
        }
        Ok(())
    }
}

impl<'a, 'b> std::cmp::PartialEq<RopeSlice<'b>> for RopeSlice<'a> {
    fn eq(&self, other: &RopeSlice<'b>) -> bool {
        if self.len_bytes() != other.len_bytes() {
            return false;
        }

        let mut chunk_itr_1 = self.chunks();
        let mut chunk_itr_2 = other.chunks();
        let mut chunk1 = chunk_itr_1.next().unwrap();
        let mut chunk2 = chunk_itr_2.next().unwrap();

        loop {
            if chunk1.len() > chunk2.len() {
                if &chunk1[..chunk2.len()] != chunk2 {
                    return false;
                } else {
                    chunk1 = &chunk1[chunk2.len()..];
                    chunk2 = "";
                }
            } else if &chunk2[..chunk1.len()] != chunk1 {
                return false;
            } else {
                chunk2 = &chunk2[chunk1.len()..];
                chunk1 = "";
            }

            if chunk1.is_empty() {
                if let Some(chunk) = chunk_itr_1.next() {
                    chunk1 = chunk;
                } else {
                    break;
                }
            }

            if chunk2.is_empty() {
                if let Some(chunk) = chunk_itr_2.next() {
                    chunk2 = chunk;
                } else {
                    break;
                }
            }
        }

        return true;
    }
}

impl<'a, 'b> std::cmp::PartialEq<&'b str> for RopeSlice<'a> {
    fn eq(&self, other: &&'b str) -> bool {
        if self.len_bytes() != other.len() {
            return false;
        }

        let mut idx = 0;
        for chunk in self.chunks() {
            if chunk != &other[idx..(idx + chunk.len())] {
                return false;
            }
            idx += chunk.len();
        }

        return true;
    }
}

impl<'a, 'b> std::cmp::PartialEq<RopeSlice<'a>> for &'b str {
    fn eq(&self, other: &RopeSlice<'a>) -> bool {
        other == self
    }
}

impl<'a> std::cmp::PartialEq<Rope> for RopeSlice<'a> {
    fn eq(&self, other: &Rope) -> bool {
        *self == other.to_slice()
    }
}

impl<'a> std::cmp::PartialEq<RopeSlice<'a>> for Rope {
    fn eq(&self, other: &RopeSlice<'a>) -> bool {
        self.to_slice() == *other
    }
}

//===========================================================

#[cfg(test)]
mod tests {
    use rope::Rope;

    // 127 bytes, 103 chars, 1 line
    const TEXT: &str = "Hello there!  How're you doing?  It's \
                        a fine day, isn't it?  Aren't you glad \
                        we're alive?  こんにちは、みんなさん！";
    // 124 bytes, 100 chars, 4 lines
    const TEXT_LINES: &str = "Hello there!  How're you doing?\nIt's \
                              a fine day, isn't it?\nAren't you glad \
                              we're alive?\nこんにちは、みんなさん！";

    #[test]
    fn len_bytes_01() {
        let r = Rope::from_str(TEXT);
        let s = r.slice(7, 98);
        assert_eq!(s.len_bytes(), 105);
    }

    #[test]
    fn len_bytes_02() {
        let r = Rope::from_str(TEXT);
        let s = r.slice(43, 43);
        assert_eq!(s.len_bytes(), 0);
    }

    #[test]
    fn len_chars_01() {
        let r = Rope::from_str(TEXT);
        let s = r.slice(7, 98);
        assert_eq!(s.len_chars(), 91);
    }

    #[test]
    fn len_chars_02() {
        let r = Rope::from_str(TEXT);
        let s = r.slice(43, 43);
        assert_eq!(s.len_chars(), 0);
    }

    #[test]
    fn len_lines_01() {
        let r = Rope::from_str(TEXT_LINES);
        let s = r.slice(34, 98);
        assert_eq!(s.len_lines(), 3);
    }

    #[test]
    fn len_lines_02() {
        let r = Rope::from_str(TEXT_LINES);
        let s = r.slice(43, 43);
        assert_eq!(s.len_lines(), 1);
    }

    #[test]
    fn char_to_line_01() {
        let r = Rope::from_str(TEXT_LINES);
        let s = r.slice(34, 96);

        // 's a fine day, isn't it?\nAren't you glad \
        // we're alive?\nこんにちは、みん

        assert_eq!(0, s.char_to_line(0));
        assert_eq!(0, s.char_to_line(1));

        assert_eq!(0, s.char_to_line(24));
        assert_eq!(1, s.char_to_line(25));
        assert_eq!(1, s.char_to_line(26));

        assert_eq!(1, s.char_to_line(53));
        assert_eq!(2, s.char_to_line(54));
        assert_eq!(2, s.char_to_line(55));

        assert_eq!(3, s.char_to_line(62));
    }

    #[test]
    fn char_to_line_02() {
        let r = Rope::from_str(TEXT_LINES);
        let s = r.slice(43, 43);

        assert_eq!(1, s.char_to_line(0));
    }

    #[test]
    #[should_panic]
    fn char_to_line_03() {
        let r = Rope::from_str(TEXT_LINES);
        let s = r.slice(34, 96);

        s.char_to_line(63);
    }

    #[test]
    fn line_to_char_01() {
        let r = Rope::from_str(TEXT_LINES);
        let s = r.slice(34, 96);

        assert_eq!(0, s.line_to_char(0));
        assert_eq!(25, s.line_to_char(1));
        assert_eq!(54, s.line_to_char(2));
        assert_eq!(62, s.line_to_char(3));
    }

    #[test]
    fn line_to_char_02() {
        let r = Rope::from_str(TEXT_LINES);
        let s = r.slice(43, 43);

        assert_eq!(0, s.line_to_char(0));
        assert_eq!(0, s.line_to_char(1));
    }

    #[test]
    #[should_panic]
    fn line_to_char_03() {
        let r = Rope::from_str(TEXT_LINES);
        let s = r.slice(34, 96);

        s.line_to_char(4);
    }

    #[test]
    fn char_01() {
        let r = Rope::from_str(TEXT);
        let s = r.slice(34, 100);

        // t's \
        // a fine day, isn't it?  Aren't you glad \
        // we're alive?  こんにちは、みんな

        assert_eq!(s.char(0), 't');
        assert_eq!(s.char(10), ' ');
        assert_eq!(s.char(18), 'n');
        assert_eq!(s.char(65), 'な');
    }

    #[test]
    #[should_panic]
    fn char_02() {
        let r = Rope::from_str(TEXT);
        let s = r.slice(34, 100);
        s.char(66);
    }

    #[test]
    #[should_panic]
    fn char_03() {
        let r = Rope::from_str(TEXT);
        let s = r.slice(43, 43);
        s.char(0);
    }

    #[test]
    fn line_01() {
        let r = Rope::from_str(TEXT_LINES);
        let s = r.slice(34, 96);
        // "'s a fine day, isn't it?\nAren't you glad \
        //  we're alive?\nこんにちは、みん"

        assert_eq!(s.line(0), "'s a fine day, isn't it?\n");
        assert_eq!(s.line(1), "Aren't you glad we're alive?\n");
        assert_eq!(s.line(2), "こんにちは、みん");
    }

    #[test]
    fn line_02() {
        let r = Rope::from_str(TEXT_LINES);
        let s = r.slice(34, 59);
        // "'s a fine day, isn't it?\n"

        assert_eq!(s.line(0), "'s a fine day, isn't it?\n");
        assert_eq!(s.line(1), "");
    }

    #[test]
    fn line_03() {
        let r = Rope::from_str(TEXT_LINES);
        let s = r.slice(43, 43);

        assert_eq!(s.line(0), "");
    }

    #[test]
    #[should_panic]
    fn line_04() {
        let r = Rope::from_str(TEXT_LINES);
        let s = r.slice(34, 96);
        s.line(3);
    }

    #[test]
    fn is_grapheme_boundary_01() {
        let r = Rope::from_str(
            "Hello there!\r\nHow're you doing?\r\n\
             It's a fine day,\r\nisn't it?",
        );
        let s = r.slice(13, 50);
        // "\nHow're you doing?\r\n\
        //  It's a fine day,\r"

        assert!(s.is_grapheme_boundary(0));
        assert!(s.is_grapheme_boundary(1));
        assert!(s.is_grapheme_boundary(18));
        assert!(!s.is_grapheme_boundary(19));
        assert!(s.is_grapheme_boundary(20));
        assert!(s.is_grapheme_boundary(36));
        assert!(s.is_grapheme_boundary(37));
    }

    #[test]
    #[should_panic]
    fn is_grapheme_boundary_03() {
        let r = Rope::from_str(
            "Hello there!\r\nHow're you doing?\r\n\
             It's a fine day,\r\nisn't it?",
        );
        let s = r.slice(13, 50);
        // "\nHow're you doing?\r\n\
        //  It's a fine day,\r"

        s.is_grapheme_boundary(38);
    }

    #[test]
    fn prev_grapheme_boundary_01() {
        let r = Rope::from_str(
            "Hello there!\r\nHow're you doing?\r\n\
             It's a fine day,\r\nisn't it?",
        );
        let s = r.slice(13, 50);
        // "\nHow're you doing?\r\n\
        //  It's a fine day,\r"

        assert_eq!(0, s.prev_grapheme_boundary(0));
        assert_eq!(0, s.prev_grapheme_boundary(1));
        assert_eq!(17, s.prev_grapheme_boundary(18));
        assert_eq!(18, s.prev_grapheme_boundary(19));
        assert_eq!(18, s.prev_grapheme_boundary(20));
        assert_eq!(35, s.prev_grapheme_boundary(36));
        assert_eq!(36, s.prev_grapheme_boundary(37));
    }

    #[test]
    #[should_panic]
    fn prev_grapheme_boundary_02() {
        let r = Rope::from_str(
            "Hello there!\r\nHow're you doing?\r\n\
             It's a fine day,\r\nisn't it?",
        );
        let s = r.slice(13, 50);
        // "\nHow're you doing?\r\n\
        //  It's a fine day,\r"

        s.prev_grapheme_boundary(38);
    }

    #[test]
    fn next_grapheme_boundary_01() {
        let r = Rope::from_str(
            "Hello there!\r\nHow're you doing?\r\n\
             It's a fine day,\r\nisn't it?",
        );
        let s = r.slice(13, 50);
        // "\nHow're you doing?\r\n\
        //  It's a fine day,\r"

        assert_eq!(1, s.next_grapheme_boundary(0));
        assert_eq!(2, s.next_grapheme_boundary(1));
        assert_eq!(20, s.next_grapheme_boundary(18));
        assert_eq!(20, s.next_grapheme_boundary(19));
        assert_eq!(21, s.next_grapheme_boundary(20));
        assert_eq!(37, s.next_grapheme_boundary(36));
        assert_eq!(37, s.next_grapheme_boundary(37));
    }

    #[test]
    #[should_panic]
    fn next_grapheme_boundary_02() {
        let r = Rope::from_str(
            "Hello there!\r\nHow're you doing?\r\n\
             It's a fine day,\r\nisn't it?",
        );
        let s = r.slice(13, 50);
        // "\nHow're you doing?\r\n\
        //  It's a fine day,\r"

        s.next_grapheme_boundary(38);
    }

    #[test]
    fn slice_01() {
        let r = Rope::from_str(TEXT);
        let s1 = r.slice(0, r.len_chars());

        let s2 = s1.slice(0, s1.len_chars());

        assert_eq!(TEXT, s2);
    }

    #[test]
    fn slice_02() {
        let r = Rope::from_str(TEXT);
        let s1 = r.slice(5, 43);

        let s2 = s1.slice(3, 25);

        assert_eq!(&TEXT[8..30], s2);
    }

    #[test]
    fn slice_03() {
        let r = Rope::from_str(TEXT);
        let s1 = r.slice(31, 97);

        let s2 = s1.slice(7, 64);

        assert_eq!(&TEXT[38..103], s2);
    }

    #[test]
    fn slice_04() {
        let r = Rope::from_str(TEXT);
        let s1 = r.slice(5, 43);

        let s2 = s1.slice(21, 21);

        assert_eq!("", s2);
    }

    #[test]
    #[should_panic]
    fn slice_05() {
        let r = Rope::from_str(TEXT);
        let s = r.slice(5, 43);

        s.slice(21, 20);
    }

    #[test]
    #[should_panic]
    fn slice_06() {
        let r = Rope::from_str(TEXT);
        let s = r.slice(5, 43);

        s.slice(37, 39);
    }

    #[test]
    fn eq_str_01() {
        let r = Rope::from_str(TEXT);
        let slice = r.to_slice();

        assert_eq!(slice, TEXT);
        assert_eq!(TEXT, slice);
    }

    #[test]
    fn eq_str_02() {
        let r = Rope::from_str(TEXT);
        let slice = r.slice(0, 20);

        assert_ne!(slice, TEXT);
        assert_ne!(TEXT, slice);
    }

    #[test]
    fn eq_str_03() {
        let mut r = Rope::from_str(TEXT);
        r.remove(20, 21);
        r.insert(20, "z");
        let slice = r.to_slice();

        assert_ne!(slice, TEXT);
        assert_ne!(TEXT, slice);
    }

    #[test]
    fn to_rope_01() {
        let r1 = Rope::from_str(TEXT);
        let s = r1.to_slice();
        let r2 = s.to_rope();

        assert_eq!(r1, r2);
        assert_eq!(s, r2);
    }

    #[test]
    fn to_rope_02() {
        let r1 = Rope::from_str(TEXT);
        let s = r1.slice(0, 24);
        let r2 = s.to_rope();

        assert_eq!(s, r2);
    }

    #[test]
    fn to_rope_03() {
        let r1 = Rope::from_str(TEXT);
        let s = r1.slice(13, 89);
        let r2 = s.to_rope();

        assert_eq!(s, r2);
    }

    #[test]
    fn to_rope_04() {
        let r1 = Rope::from_str(TEXT);
        let s = r1.slice(13, 41);
        let r2 = s.to_rope();

        assert_eq!(s, r2);
    }

    // Iterator tests are in the iter module
}
