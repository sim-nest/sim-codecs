//! Bounded, located byte lanes used by the classfile grammar.

use core::fmt;

/// A precise byte-lane failure category.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ByteErrorKind {
    /// The input ended before the requested value was complete.
    Truncated,
    /// Offset or length arithmetic overflowed.
    LengthOverflow,
    /// A declared or produced value exceeded its allocation budget.
    BudgetExceeded,
    /// Modified UTF-8 used a longer representation than necessary.
    OverlongModifiedUtf8,
    /// Modified UTF-8 contained a literal zero byte.
    IllegalZero,
    /// Modified UTF-8 contained an invalid byte sequence.
    InvalidModifiedUtf8,
    /// Modified UTF-8 contained an unpaired UTF-16 surrogate.
    MalformedSurrogate,
}

/// A byte-lane error located at an absolute input or output offset.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ByteError {
    /// Stable machine-matchable failure category.
    pub kind: ByteErrorKind,
    /// Absolute byte offset at which the failure was detected.
    pub offset: usize,
    /// Human-readable context.
    pub message: String,
}

impl ByteError {
    pub(crate) fn new(kind: ByteErrorKind, offset: usize, message: impl Into<String>) -> Self {
        Self {
            kind,
            offset,
            message: message.into(),
        }
    }
}

impl fmt::Display for ByteError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} at byte {}", self.message, self.offset)
    }
}

impl std::error::Error for ByteError {}

/// A zero-copy big-endian reader confined to one declared byte region.
#[derive(Clone, Debug)]
pub struct ByteReader<'a> {
    bytes: &'a [u8],
    position: usize,
    origin: usize,
    allocation_budget: usize,
}

impl<'a> ByteReader<'a> {
    /// Construct a reader whose owned outputs may contain at most `allocation_budget` bytes/items.
    pub fn new(bytes: &'a [u8], allocation_budget: usize) -> Self {
        Self {
            bytes,
            position: 0,
            origin: 0,
            allocation_budget,
        }
    }

    fn with_origin(bytes: &'a [u8], allocation_budget: usize, origin: usize) -> Self {
        Self {
            bytes,
            position: 0,
            origin,
            allocation_budget,
        }
    }

    /// Current absolute byte offset.
    pub fn offset(&self) -> usize {
        self.origin + self.position
    }

    /// Bytes remaining in this reader's declared region.
    pub fn remaining(&self) -> usize {
        self.bytes.len() - self.position
    }

    /// Allocation budget inherited by values decoded from this region.
    pub fn allocation_budget(&self) -> usize {
        self.allocation_budget
    }

    /// Reject an allocation before reserving it if its declared size exceeds the budget.
    pub fn preflight_allocation(&self, amount: usize) -> Result<(), ByteError> {
        if amount > self.allocation_budget {
            return Err(ByteError::new(
                ByteErrorKind::BudgetExceeded,
                self.offset(),
                format!(
                    "declared allocation {amount} exceeds budget {}",
                    self.allocation_budget
                ),
            ));
        }
        Ok(())
    }

    /// Read one byte.
    pub fn read_u1(&mut self) -> Result<u8, ByteError> {
        Ok(self.take(1)?[0])
    }

    /// Read an unsigned big-endian two-byte integer.
    pub fn read_u2(&mut self) -> Result<u16, ByteError> {
        let value: [u8; 2] = self.take(2)?.try_into().expect("exact length");
        Ok(u16::from_be_bytes(value))
    }

    /// Read an unsigned big-endian four-byte integer.
    pub fn read_u4(&mut self) -> Result<u32, ByteError> {
        let value: [u8; 4] = self.take(4)?.try_into().expect("exact length");
        Ok(u32::from_be_bytes(value))
    }

    /// Borrow exactly `length` bytes.
    pub fn take(&mut self, length: usize) -> Result<&'a [u8], ByteError> {
        let start = self.position;
        let end = start.checked_add(length).ok_or_else(|| {
            ByteError::new(
                ByteErrorKind::LengthOverflow,
                self.offset(),
                "byte length overflow",
            )
        })?;
        let result = self.bytes.get(start..end).ok_or_else(|| {
            ByteError::new(
                ByteErrorKind::Truncated,
                self.origin + self.bytes.len(),
                format!("needed {length} bytes, only {} remain", self.remaining()),
            )
        })?;
        self.position = end;
        Ok(result)
    }

    /// Create a child reader bounded to exactly `length` bytes and advance the parent.
    pub fn sub_reader(&mut self, length: usize) -> Result<ByteReader<'a>, ByteError> {
        let origin = self.offset();
        let bytes = self.take(length)?;
        Ok(Self::with_origin(bytes, self.allocation_budget, origin))
    }
}

/// A checked big-endian writer with a hard output budget.
#[derive(Clone, Debug)]
pub struct ByteWriter {
    bytes: Vec<u8>,
    budget: usize,
}

impl ByteWriter {
    /// Construct an empty writer that can produce at most `budget` bytes.
    pub fn new(budget: usize) -> Self {
        Self {
            bytes: Vec::new(),
            budget,
        }
    }

    fn reserve_for(&mut self, additional: usize) -> Result<(), ByteError> {
        let target = self.bytes.len().checked_add(additional).ok_or_else(|| {
            ByteError::new(
                ByteErrorKind::LengthOverflow,
                self.bytes.len(),
                "output length overflow",
            )
        })?;
        if target > self.budget {
            return Err(ByteError::new(
                ByteErrorKind::BudgetExceeded,
                self.bytes.len(),
                format!("output length {target} exceeds budget {}", self.budget),
            ));
        }
        self.bytes.try_reserve_exact(additional).map_err(|error| {
            ByteError::new(
                ByteErrorKind::BudgetExceeded,
                self.bytes.len(),
                format!("output allocation failed: {error}"),
            )
        })
    }

    /// Write one byte.
    pub fn write_u1(&mut self, value: u8) -> Result<(), ByteError> {
        self.write_bytes(&[value])
    }
    /// Write an unsigned big-endian two-byte integer.
    pub fn write_u2(&mut self, value: u16) -> Result<(), ByteError> {
        self.write_bytes(&value.to_be_bytes())
    }
    /// Write an unsigned big-endian four-byte integer.
    pub fn write_u4(&mut self, value: u32) -> Result<(), ByteError> {
        self.write_bytes(&value.to_be_bytes())
    }
    /// Append exact bytes after checking the output budget.
    pub fn write_bytes(&mut self, value: &[u8]) -> Result<(), ByteError> {
        self.reserve_for(value.len())?;
        self.bytes.extend_from_slice(value);
        Ok(())
    }
    /// Borrow all bytes written so far.
    pub fn as_slice(&self) -> &[u8] {
        &self.bytes
    }
    /// Finish and return the written bytes.
    pub fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }
}
