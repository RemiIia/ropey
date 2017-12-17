#![allow(dead_code)]

use std;
use std::fmt;
use std::iter::{Iterator, Zip};
use std::mem;
use std::mem::ManuallyDrop;
use std::ptr;
use std::slice;
use std::sync::Arc;

use node;
use node::Node;
use str_utils::nearest_internal_grapheme_boundary;
use text_info::TextInfo;

const MAX_LEN: usize = node::MAX_CHILDREN;

pub(crate) struct ChildArray {
    nodes: ManuallyDrop<[Arc<Node>; MAX_LEN]>,
    info: [TextInfo; MAX_LEN],
    len: u8,
}

impl ChildArray {
    /// Creates a new empty array.
    pub fn new() -> ChildArray {
        ChildArray {
            nodes: ManuallyDrop::new(unsafe { std::mem::uninitialized() }),
            info: unsafe { std::mem::uninitialized() },
            len: 0,
        }
    }

    /// Current length of the array.
    pub fn len(&self) -> usize {
        self.len as usize
    }

    /// Returns whether the array is full or not.
    pub fn is_full(&self) -> bool {
        (self.len as usize) == MAX_LEN
    }

    /// Returns a slice to the nodes array.
    pub fn nodes(&self) -> &[Arc<Node>] {
        &self.nodes[..(self.len as usize)]
    }

    /// Returns a mutable slice to the nodes array.
    pub fn nodes_mut(&mut self) -> &mut [Arc<Node>] {
        &mut self.nodes[..(self.len as usize)]
    }

    /// Returns a slice to the info array.
    pub fn info(&self) -> &[TextInfo] {
        &self.info[..(self.len as usize)]
    }

    /// Returns a mutable slice to the info array.
    pub fn info_mut(&mut self) -> &mut [TextInfo] {
        &mut self.info[..(self.len as usize)]
    }

    /// Returns mutable slices to both the nodes and info arrays.
    pub fn info_and_nodes_mut(&mut self) -> (&mut [TextInfo], &mut [Arc<Node>]) {
        (
            &mut self.info[..(self.len as usize)],
            &mut self.nodes[..(self.len as usize)],
        )
    }

    /// Pushes an item into the end of the array.
    ///
    /// Increases length by one.  Panics if already full.
    pub fn push(&mut self, item: (TextInfo, Arc<Node>)) {
        assert!(self.len() < MAX_LEN);
        self.info[self.len as usize] = item.0;
        mem::forget(mem::replace(&mut self.nodes[self.len as usize], item.1));
        self.len += 1;
    }

    /// Pushes an element onto the end of the array, and then splits it in half,
    /// returning the right half.
    ///
    /// This works even when the array is full.
    pub fn push_split(&mut self, new_child: (TextInfo, Arc<Node>)) -> ChildArray {
        let r_count = (self.len() + 1) / 2;
        let l_count = (self.len() + 1) - r_count;

        let mut right = self.split_off(l_count);
        right.push(new_child);
        right
    }

    /// Attempts to merge two nodes, and if it's too much data to merge
    /// equi-distributes it between the two.
    ///
    /// Returns:
    ///
    /// - True: merge was successful.
    /// - False: merge failed, equidistributed instead.
    pub fn merge_distribute(&mut self, idx1: usize, idx2: usize) -> bool {
        assert!(idx1 < idx2);
        assert!(idx2 < self.len());
        let remove_right = {
            let ((_, node1), (_, node2)) = self.get_two_mut(idx1, idx2);
            let node1 = Arc::make_mut(node1);
            let node2 = Arc::make_mut(node2);
            match node1 {
                &mut Node::Leaf(ref mut text1) => {
                    if let &mut Node::Leaf(ref mut text2) = node2 {
                        text1.push_str(text2);

                        if text1.len() <= node::MAX_BYTES {
                            true
                        } else {
                            let split_pos = {
                                let pos = text1.len() - (text1.len() / 2);
                                nearest_internal_grapheme_boundary(&text1, pos)
                            };
                            *text2 = text1.split_off(split_pos);
                            if text2.len() > 0 {
                                text1.shrink_to_fit();
                                false
                            } else {
                                true
                            }
                        }
                    } else {
                        panic!("Siblings have different node types");
                    }
                }

                &mut Node::Internal(ref mut children1) => {
                    if let &mut Node::Internal(ref mut children2) = node2 {
                        if (children1.len() + children2.len()) < MAX_LEN {
                            for _ in 0..children2.len() {
                                children1.push(children2.remove(0));
                            }
                            true
                        } else {
                            let r_target_len = (children1.len() + children2.len()) / 2;
                            while children2.len() < r_target_len {
                                children2.insert(0, children1.pop());
                            }
                            while children2.len() > r_target_len {
                                children1.push(children2.remove(0));
                            }
                            false
                        }
                    } else {
                        panic!("Siblings have different node types");
                    }
                }
            }
        };

        if remove_right {
            self.remove(idx2);
            self.info[idx1] = self.nodes[idx1].text_info();
            return true;
        } else {
            self.info[idx1] = self.nodes[idx1].text_info();
            self.info[idx2] = self.nodes[idx2].text_info();
            return false;
        }
    }

    /// Pops an item off the end of the array and returns it.
    ///
    /// Decreases length by one.  Panics if already empty.
    pub fn pop(&mut self) -> (TextInfo, Arc<Node>) {
        assert!(self.len() > 0);
        self.len -= 1;
        let item = (self.info[self.len as usize], unsafe {
            ptr::read(&self.nodes[self.len as usize])
        });
        item
    }

    /// Inserts an item into the the array at the given index.
    ///
    /// Increases length by one.  Panics if already full.  Preserves ordering
    /// of the other items.
    pub fn insert(&mut self, idx: usize, item: (TextInfo, Arc<Node>)) {
        assert!(idx <= self.len());
        assert!(self.len() < MAX_LEN);

        let len = self.len as usize;
        unsafe {
            ptr::copy(
                self.nodes.as_ptr().offset(idx as isize),
                self.nodes.as_mut_ptr().offset((idx + 1) as isize),
                len - idx,
            );
            ptr::copy(
                self.info.as_ptr().offset(idx as isize),
                self.info.as_mut_ptr().offset((idx + 1) as isize),
                len - idx,
            );
        }

        self.info[idx] = item.0;
        mem::forget(mem::replace(&mut self.nodes[idx], item.1));

        self.len += 1;
    }

    /// Inserts an element into a the array, and then splits it in half, returning
    /// the right half.
    ///
    /// This works even when the array is full.
    pub fn insert_split(&mut self, idx: usize, item: (TextInfo, Arc<Node>)) -> ChildArray {
        assert!(self.len() > 0);
        assert!(idx <= self.len());
        let extra = if idx < self.len() {
            let extra = self.pop();
            self.insert(idx, item);
            extra
        } else {
            item
        };

        self.push_split(extra)
    }

    /// Removes the item at the given index from the the array.
    ///
    /// Decreases length by one.  Preserves ordering of the other items.
    pub fn remove(&mut self, idx: usize) -> (TextInfo, Arc<Node>) {
        assert!(self.len() > 0);
        assert!(idx < self.len());

        let item = (self.info[idx], unsafe { ptr::read(&self.nodes[idx]) });

        let len = self.len as usize;
        unsafe {
            ptr::copy(
                self.nodes.as_ptr().offset(idx as isize + 1),
                self.nodes.as_mut_ptr().offset(idx as isize),
                len - idx - 1,
            );
            ptr::copy(
                self.info.as_ptr().offset(idx as isize + 1),
                self.info.as_mut_ptr().offset(idx as isize),
                len - idx - 1,
            );
        }

        self.len -= 1;
        return item;
    }

    /// Splits the array in two at `idx`, returning the right part of the split.
    ///
    /// TODO: implement this more efficiently.
    pub fn split_off(&mut self, idx: usize) -> ChildArray {
        assert!(idx <= self.len());

        let mut other = ChildArray::new();
        let count = self.len() - idx;
        for _ in 0..count {
            other.push(self.remove(idx));
        }

        other
    }

    /// Gets references to the nth item's node and info.
    pub fn i(&self, n: usize) -> (&TextInfo, &Arc<Node>) {
        assert!(n < self.len());
        (
            &self.info[self.len as usize],
            &self.nodes[self.len as usize],
        )
    }

    /// Gets mut references to the nth item's node and info.
    pub fn i_mut(&mut self, n: usize) -> (&mut TextInfo, &mut Arc<Node>) {
        assert!(n < self.len());
        (
            &mut self.info[self.len as usize],
            &mut self.nodes[self.len as usize],
        )
    }

    /// Fetches two children simultaneously, returning mutable references
    /// to their info and nodes.
    ///
    /// `idx1` must be less than `idx2`.
    pub fn get_two_mut(
        &mut self,
        idx1: usize,
        idx2: usize,
    ) -> ((&mut TextInfo, &mut Arc<Node>), (&mut TextInfo, &mut Arc<Node>)) {
        assert!(idx1 < idx2);
        assert!(idx2 < self.len());

        let split_idx = idx1 + 1;
        let (info1, info2) = self.info.split_at_mut(split_idx);
        let (nodes1, nodes2) = self.nodes.split_at_mut(split_idx);

        ((&mut info1[idx1], &mut nodes1[idx1]), (
            &mut info2
                [idx2 - split_idx],
            &mut nodes2
                [idx2 - split_idx],
        ))
    }

    /// Creates an iterator over the array's items.
    pub fn iter(&self) -> Zip<slice::Iter<TextInfo>, slice::Iter<Arc<Node>>> {
        Iterator::zip(
            (&self.info[..(self.len as usize)]).iter(),
            (&self.nodes[..(self.len as usize)]).iter(),
        )
    }

    /// Creates an iterator over the array's items.
    pub fn iter_mut(&mut self) -> Zip<slice::IterMut<TextInfo>, slice::IterMut<Arc<Node>>> {
        Iterator::zip(
            (&mut self.info[..(self.len as usize)]).iter_mut(),
            (&mut self.nodes[..(self.len as usize)]).iter_mut(),
        )
    }

    pub fn combined_info(&self) -> TextInfo {
        self.info[..self.len()].iter().fold(
            TextInfo::new(),
            |a, b| a.combine(b),
        )
    }

    pub fn search_combine_info<F: Fn(&TextInfo) -> bool>(&self, pred: F) -> (usize, TextInfo) {
        let mut accum = TextInfo::new();
        for (idx, inf) in self.info[..self.len()].iter().enumerate() {
            if pred(&accum.combine(inf)) {
                return (idx, accum);
            } else {
                accum = accum.combine(inf);
            }
        }
        panic!("Predicate is mal-formed and never evaluated true.")
    }
}

impl fmt::Debug for ChildArray {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("ChildArray")
            .field("nodes", &&self.nodes[0..self.len()])
            .field("info", &&self.info[0..self.len()])
            .field("len", &self.len)
            .finish()
    }
}

impl Drop for ChildArray {
    fn drop(&mut self) {
        for node in &mut self.nodes[..self.len as usize] {
            let mptr: *mut Arc<Node> = node; // Make sure we have the right dereference
            unsafe { ptr::drop_in_place(mptr) };
        }
    }
}

impl Clone for ChildArray {
    fn clone(&self) -> ChildArray {
        let mut clone_array = ChildArray::new();

        // Copy nodes... carefully.
        for (clone_arc, arc) in Iterator::zip(
            clone_array.nodes[..self.len()].iter_mut(),
            self.nodes[..self.len()].iter(),
        )
        {
            mem::forget(mem::replace(clone_arc, arc.clone()));
        }

        // Copy TextInfo
        for (clone_info, info) in
            Iterator::zip(
                clone_array.info[..self.len()].iter_mut(),
                self.info[..self.len()].iter(),
            )
        {
            *clone_info = *info;
        }

        // Set length
        clone_array.len = self.len;

        // Some sanity checks for debug builds
        #[cfg(debug_assertions)]
        {
            for (a, b) in Iterator::zip(clone_array.iter(), self.iter()) {
                assert_eq!(a.0, b.0);
                assert!(Arc::ptr_eq(a.1, b.1));
            }
        }

        clone_array
    }
}