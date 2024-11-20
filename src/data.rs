use std::fmt::Debug;

use nom::number::Endianness;

pub struct MatFile {
    header: Header,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ByteOrder {
    LittleEndian,
    BigEndian,
}

impl Into<Endianness> for ByteOrder {
    fn into(self) -> Endianness {
        match self {
            ByteOrder::LittleEndian => Endianness::Little,
            ByteOrder::BigEndian => Endianness::Big,
        }
    }
}

pub struct Header {
    text: [u8; 116],
    byte_order: ByteOrder,
}

pub enum Array {
    Numeric(NumericArray),
    Sparse(SparseArray),
    Character(CharacterArray),
    Structure(StructureArray),
    // Cell
    // Object,
    Unsupported(UnsupportedArray),
}

#[derive(Clone)]
pub struct Dimensions {
    rows: u32,
    columns: u32,
    more: Vec<u32>,
}

impl Dimensions {
    pub fn new(rows: u32, columns: u32) -> Self {
        Self {
            rows,
            columns,
            more: Vec::new(),
        }
    }

    pub fn dimensions(&self) -> usize {
        2 + self.more.len()
    }
}

impl Debug for Dimensions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}×{}", self.rows, self.columns)?;

        for el in &self.more {
            write!(f, "×{}", el)?;
        }

        Ok(())
    }
}

pub struct ArrayHeader {
    dimensions: Dimensions,
    name: String,
}

pub struct NumericArray {
    header: ArrayHeader,
    real: NumericData,
    imag: Option<NumericArray>,
}

pub enum NumericData {
    Int8(Vec<i8>),
    UInt8(Vec<u8>),
    Int16(Vec<i16>),
    UInt16(Vec<u16>),
    Int32(Vec<i32>),
    UInt32(Vec<u32>),
    Int64(Vec<i64>),
    UInt64(Vec<u64>),
    Single(Vec<f32>),
    Double(Vec<f64>),
}

pub struct SparseArray {}

pub struct CharacterArray {}

pub struct StructureArray {}

pub struct UnsupportedArray {
    header: ArrayHeader,
    data: Vec<u8>,
}
