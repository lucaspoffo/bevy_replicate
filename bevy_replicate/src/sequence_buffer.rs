use core::ops::Range;

pub type SequenceNumber = u64;

pub struct SequenceBuffer<T> {
    sequences: Box<[Option<SequenceNumber>]>,
    data: Box<[Option<T>]>,
}

impl<T> SequenceBuffer<T> {
    pub fn with_capacity(capacity: usize) -> Self {
        assert!(capacity > 0, "tried to initialize SequenceBuffer with 0 capacity");
        let mut data = Vec::with_capacity(capacity);
        for _ in 0..capacity {
            data.push(None);
        }

        Self {
            sequences: vec![None; capacity].into_boxed_slice(),
            data: data.into_boxed_slice(),
        }
    }

    pub fn size(&self) -> usize {
        self.sequences.len()
    }

    #[inline]
    pub fn index_of(&self, sequence: SequenceNumber) -> usize {
        sequence as usize % self.data.len()
    }

    pub fn contains(&self, sequence: SequenceNumber) -> bool {
        self.sequences[self.index_of(sequence)] == Some(sequence)
    }

    #[allow(dead_code)]
    pub fn get(&self, sequence: SequenceNumber) -> Option<&T> {
        let index = self.index_of(sequence);
        if self.sequences[index] == Some(sequence) {
            self.data[index].as_ref()
        } else {
            None
        }
    }

    pub fn get_mut(&mut self, sequence: SequenceNumber) -> Option<&mut T> {
        let index = self.index_of(sequence);
        if self.sequences[index] == Some(sequence) {
            self.data[index].as_mut()
        } else {
            None
        }
    }

    pub fn get_index(&self, index: usize) -> (&Option<SequenceNumber>, &Option<T>) {
        (&self.sequences[index], &self.data[index])
    }

    pub fn get_index_mut(&mut self, index: usize) -> (&mut Option<SequenceNumber>, &mut Option<T>) {
        (&mut self.sequences[index], &mut self.data[index])
    }

    pub fn get_or_insert(&mut self, sequence: SequenceNumber, data: T) -> Option<&mut T> {
        if self.contains(sequence) {
            self.get_mut(sequence)
        } else {
            self.insert(sequence, data)
        }
    }

    pub fn get_or_insert_with<F: FnOnce() -> T>(&mut self, sequence: SequenceNumber, f: F) -> Option<&mut T> {
        if self.contains(sequence) {
            self.get_mut(sequence)
        } else {
            self.insert(sequence, f())
        }
    }

    pub fn insert(&mut self, sequence: SequenceNumber, data: T) -> Option<&mut T> {
        let index = self.index_of(sequence);
        if let Some(current_sequence) = self.sequences[index] {
            if sequence < current_sequence {
                return None;
            }
        }
        self.sequences[index] = Some(sequence);
        self.data[index] = Some(data);
        self.data[index].as_mut()
    }

    pub fn remove(&mut self, sequence: SequenceNumber) -> Option<T> {
        let index = self.index_of(sequence);
        self.sequences[index].take();
        self.data[index].take()
    }

    pub fn remove_index(&mut self, index: usize) -> (Option<SequenceNumber>, Option<T>) {
        (self.sequences[index].take(), self.data[index].take())
    }

    pub fn remove_range(&mut self, range: Range<SequenceNumber>) {
        let start_idx = self.index_of(range.start);
        let end_idx = self.index_of(range.end);

        if end_idx < start_idx {
            for index in start_idx..=end_idx {
                self.remove(index as u64);
            }
        } else {
            for index in 0..self.data.len() {
                self.data[index] = None;
                self.sequences[index] = None;
            }
        }
    }

    pub fn entries(&self) -> &[Option<T>] {
        &self.data
    }
}
