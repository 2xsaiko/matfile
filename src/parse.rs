use libflate::zlib::Decoder;
use nom::branch::alt;
use nom::bytes::complete::tag;
use nom::bytes::complete::take;
use nom::character::complete::char;
use nom::combinator::{complete, cond, eof, map, map_res, not, opt, peek, value};
use nom::multi::{count, length_value, many0};
use nom::number::complete::f32;
use nom::number::complete::f64;
use nom::number::complete::i16;
use nom::number::complete::i32;
use nom::number::complete::i64;
use nom::number::complete::i8;
use nom::number::complete::u16;
use nom::number::complete::u32;
use nom::number::complete::u64;
use nom::number::complete::u8;
use nom::sequence::pair;
use nom::{error_position, IResult};
use num_traits::FromPrimitive;
use std::ffi::CStr;
use std::io::Read;

// https://www.mathworks.com/help/pdf_doc/matlab/matfile_format.pdf
// https://www.mathworks.com/help/matlab/import_export/mat-file-versions.html

#[derive(Clone, Debug)]
pub struct Header {
    text: String,
    is_little_endian: bool,
}

#[derive(Clone, Debug, PartialEq)]
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

impl NumericData {
    fn len(&self) -> usize {
        match self {
            NumericData::Single(vec) => vec.len(),
            NumericData::Double(vec) => vec.len(),
            NumericData::Int8(vec) => vec.len(),
            NumericData::UInt8(vec) => vec.len(),
            NumericData::Int16(vec) => vec.len(),
            NumericData::UInt16(vec) => vec.len(),
            NumericData::Int32(vec) => vec.len(),
            NumericData::UInt32(vec) => vec.len(),
            NumericData::Int64(vec) => vec.len(),
            NumericData::UInt64(vec) => vec.len(),
        }
    }

    fn data_type(&self) -> DataType {
        match self {
            NumericData::Single(_) => DataType::Single,
            NumericData::Double(_) => DataType::Double,
            NumericData::Int8(_) => DataType::Int8,
            NumericData::UInt8(_) => DataType::UInt8,
            NumericData::Int16(_) => DataType::Int16,
            NumericData::UInt16(_) => DataType::UInt16,
            NumericData::Int32(_) => DataType::Int32,
            NumericData::UInt32(_) => DataType::UInt32,
            NumericData::Int64(_) => DataType::Int64,
            NumericData::UInt64(_) => DataType::UInt64,
        }
    }
}

#[derive(Clone, Debug)]
pub enum DataElement {
    NumericMatrix(Numeric),
    SparseMatrix(Sparse),
    CharacterMatrix(Character),
    // Cell Matrix,
    StructureMatrix(Structure),
    // Object Matrix,
    Unsupported,
}

#[derive(Clone, Debug)]
pub struct ArrayHeader {
    pub flags: ArrayFlags,
    pub dimensions: Dimensions,
    pub name: String,
}

#[derive(Clone, Debug)]
pub struct Numeric {
    pub header: ArrayHeader,
    pub real_part: NumericData,
    pub imag_part: Option<NumericData>,
}

#[derive(Clone, Debug)]
pub struct Sparse {
    pub header: ArrayHeader,
    pub row_index: RowIndex,
    pub column_index: ColumnShift,
    pub real_part: NumericData,
    pub imag_part: Option<NumericData>,
}

#[derive(Clone, Debug)]
pub struct Character {
    pub header: ArrayHeader,
    pub real_part: CharacterData,
    pub imag_part: Option<CharacterData>,
}

#[derive(Clone, Debug)]
pub enum CharacterData {
    Unicode(String),
    NonUnicode(Vec<u16>),
}

#[derive(Clone, Debug)]
pub struct Structure {
    pub header: ArrayHeader,
    pub field_names: Vec<String>,
    pub values: Vec<DataElement>,
}

impl Structure {
    pub fn new(header: ArrayHeader) -> Self {
        Structure {
            header,
            field_names: Vec::new(),
            values: Vec::new(),
        }
    }

    pub fn header(&self) -> &ArrayHeader {
        &self.header
    }

    fn index(&self, name: &str) -> Option<usize> {
        // unindexed search, let's assume that structures are small
        self.field_names.iter().position(|v| v == name)
    }

    pub fn len(&self) -> usize {
        self.field_names.len()
    }

    pub fn field_names(&self) -> impl Iterator<Item = &str> {
        self.field_names.iter().map(|v| &**v)
    }

    pub fn values(&self) -> impl Iterator<Item = &DataElement> {
        self.values.iter()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &DataElement)> {
        self.field_names().zip(self.values())
    }

    pub fn get(&self, name: &str) -> Option<&DataElement> {
        let idx = self.index(name)?;
        Some(&self.values[idx])
    }

    pub fn get_mut(&mut self, name: &str) -> Option<&mut DataElement> {
        let idx = self.index(name)?;
        Some(&mut self.values[idx])
    }

    pub fn insert(&mut self, name: &str, v: DataElement) -> Option<DataElement> {
        match self.index(name) {
            Some(idx) => Some(std::mem::replace(&mut self.values[idx], v)),
            None => {
                self.field_names.push(name.to_string());
                self.values.push(v);
                None
            }
        }
    }

    pub fn remove(&mut self, name: &str) -> Option<DataElement> {
        let idx = self.index(name)?;
        self.field_names.remove(idx);
        Some(self.values.remove(idx))
    }
}

// #[cfg(feature = "ndarray")]
// {
//     #[derive(Debug)]
//     enum NumericArrayData {
//         Double(ndarray::ArrayD<f64>),
//     }

//     impl From<NumericData> for NumericArrayData {
//         fn from(nd: NumericData) -> Self;
//     }
// }

pub fn parse_header(i: &[u8]) -> IResult<&[u8], Header> {
    // Make sure that the first four bytes are not null
    let (i, _) = peek(count(pair(not(char('\0')), take(1usize)), 4))(i)?;
    // Header text field
    let (i, text) = take(116usize)(i)?;
    // Header subsystem data offset field
    let (i, _ssdo) = take(8usize)(i)?;
    // Header flag fields
    // Assume little endian for now
    let (i, mut version) = u16(nom::number::Endianness::Little)(i)?;
    // Check the endianness
    let (i, is_little_endian) = alt((value(true, tag("IM")), value(false, tag("MI"))))(i)?;
    // Fix endianness of the version field if we assumed the wrong one
    if !is_little_endian {
        version = version.swap_bytes();
    }
    if version != 0x0100 {
        return Err(nom::Err::Failure(error_position!(
            i,
            // TODO
            nom::error::ErrorKind::Tag
        )));
    }
    Ok((
        i,
        Header {
            text: std::str::from_utf8(text).unwrap_or(&"").to_owned(),
            is_little_endian: is_little_endian,
        },
    ))
}

fn constant<T: Clone>(v: T) -> impl Fn(&[u8]) -> IResult<&[u8], T> {
    move |i: &[u8]| Ok((i, v.clone()))
}

fn parse_next_data_element(
    endianness: nom::number::Endianness,
    name: Option<&str>,
) -> impl Fn(&[u8]) -> IResult<&[u8], DataElement> + '_ {
    move |i: &[u8]| {
        let (i, data_element_tag) = parse_data_element_tag(endianness)(i)?;
        let next_parser: Box<dyn Fn(_) -> _> = match data_element_tag.data_type {
            DataType::Matrix => Box::new(parse_matrix_data_element(endianness, name)),
            DataType::Compressed => {
                if name.is_some() {
                    // only supplied for struct fields, and they are always Matrix
                    unreachable!();
                }

                Box::new(parse_compressed_data_element(endianness))
            }
            _ => {
                println!(
                    "Unsupported variable type: {:?} (must be Matrix or Compressed)",
                    data_element_tag.data_type
                );
                Box::new(parse_unsupported_data_element(endianness))
            }
        };
        let (i, data_element) =
            length_value(constant(data_element_tag.data_byte_size), next_parser)(i)?;
        // Take care of padding. It seems like either all variables in a mat file are compressed or none are.
        // If the variables are compressed there is no alignment to take care of (only uncompressed data
        // needs to be aligned according to the spec). Otherwise make sure that we end up on an 8 byte
        // boundary (ignore if there is not enough data left)
        let num_padding_bytes = if data_element_tag.data_type == DataType::Compressed {
            0
        } else {
            data_element_tag.padding_byte_size
        };
        let (i, _) = opt(complete(take(num_padding_bytes)))(i)?;
        Ok((i, data_element))
    }
}

fn ceil_to_multiple(x: u32, multiple: u32) -> u32 {
    if x > 0 {
        (((x - 1) / multiple) + 1) * multiple
    } else {
        0
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ArrayFlags {
    pub complex: bool,
    pub global: bool,
    pub logical: bool,
    pub class: ArrayType,
    pub nzmax: usize,
}

#[derive(Debug, PartialEq, Clone, Copy, Primitive)]
pub enum DataType {
    Int8 = 1,
    UInt8 = 2,
    Int16 = 3,
    UInt16 = 4,
    Int32 = 5,
    UInt32 = 6,
    Single = 7,
    Double = 9,
    Int64 = 12,
    UInt64 = 13,
    Matrix = 14,
    Compressed = 15,
    Utf8 = 16,
    Utf16 = 17,
    Utf32 = 18,
}

// impl DataType {
//     fn byte_size(&self) -> Option<usize> {
//         match self {
//             DataType::Int8 | DataType::UInt8 | DataType::Utf8 => Some(1),
//             DataType::Int16 | DataType::UInt16 | DataType::Utf16 => Some(2),
//             DataType::Int32 | DataType::UInt32 | DataType::Single | DataType::Utf32 => Some(4),
//             DataType::Int64 | DataType::UInt64 | DataType::Double => Some(8),
//             _ => None,
//         }
//     }
// }

#[derive(Debug, PartialEq, Clone, Copy, Primitive)]
pub enum ArrayType {
    Cell = 1,
    Struct = 2,
    Object = 3,
    Char = 4,
    Sparse = 5,
    Double = 6,
    Single = 7,
    Int8 = 8,
    UInt8 = 9,
    Int16 = 10,
    UInt16 = 11,
    Int32 = 12,
    UInt32 = 13,
    Int64 = 14,
    UInt64 = 15,
}

impl ArrayType {
    // fn is_numeric(&self) -> bool {
    //     match self {
    //         ArrayType::Cell
    //         | ArrayType::Struct
    //         | ArrayType::Object
    //         | ArrayType::Char
    //         | ArrayType::Sparse => false,
    //         _ => true,
    //     }
    // }

    fn numeric_data_type(&self) -> Option<DataType> {
        match self {
            ArrayType::Double => Some(DataType::Double),
            ArrayType::Single => Some(DataType::Single),
            ArrayType::Int8 => Some(DataType::Int8),
            ArrayType::UInt8 => Some(DataType::UInt8),
            ArrayType::Int16 => Some(DataType::Int16),
            ArrayType::UInt16 => Some(DataType::UInt16),
            ArrayType::Int32 => Some(DataType::UInt32),
            ArrayType::UInt32 => Some(DataType::UInt32),
            ArrayType::Int64 => Some(DataType::Int64),
            ArrayType::UInt64 => Some(DataType::UInt64),
            _ => None,
        }
    }
}

pub type Dimensions = Vec<i32>;

#[derive(Clone, Copy, Debug)]
pub struct DataElementTag {
    data_type: DataType,
    data_byte_size: u32,
    padding_byte_size: u32,
}

fn parse_data_element_tag(
    endianness: nom::number::Endianness,
) -> impl Fn(&[u8]) -> IResult<&[u8], DataElementTag> {
    move |i: &[u8]| {
        let (i, starting_bytes) = u32(endianness)(i)?;
        let (i, data_type, byte_size, padding_byte_size) = if starting_bytes & 0xFFFF0000 == 0 {
            // Long Data Element Format
            let data_type = starting_bytes;
            let (i, byte_size) = u32(endianness)(i)?;
            let padding_byte_size = ceil_to_multiple(byte_size, 8) - byte_size;
            (i, data_type, byte_size, padding_byte_size)
        } else {
            // Small Data Element Format
            let data_type = starting_bytes & 0x0000FFFF;
            let byte_size = (starting_bytes & 0xFFFF0000) >> 16;
            // Assert that byte_size is <= 4
            if byte_size > 4 {
                return Err(nom::Err::Failure(error_position!(
                    i,
                    // TODO
                    nom::error::ErrorKind::Tag
                )));
            }
            let padding_byte_size = 4 - byte_size;
            (i, data_type, byte_size, padding_byte_size)
        };
        Ok((
            i,
            DataElementTag {
                data_type: DataType::from_u32(data_type).ok_or(nom::Err::Failure(
                    nom::error::Error {
                        input: i,
                        // TODO
                        code: nom::error::ErrorKind::Tag,
                    },
                ))?,
                data_byte_size: byte_size,
                padding_byte_size: padding_byte_size,
            },
        ))
    }
}

fn parse_array_name_subelement(
    endianness: nom::number::Endianness,
) -> impl Fn(&[u8]) -> IResult<&[u8], Option<String>> {
    move |i: &[u8]| {
        let (i, data_element_tag) = parse_data_element_tag(endianness)(i)?;

        if data_element_tag.data_type != DataType::Int8 {
            return Err(nom::Err::Failure(error_position!(
                i,
                // TODO
                nom::error::ErrorKind::Tag
            )));
        }

        if data_element_tag.data_byte_size == 0 {
            return Ok((i, None));
        }

        let (i, name) = map_res(take(data_element_tag.data_byte_size), |b| {
            std::str::from_utf8(b)
                .map(|s| s.to_owned())
                .map_err(|_err| {
                    nom::Err::Failure((i, nom::error::ErrorKind::Tag)) // TODO
                })
        })(i)?;
        // Padding bytes
        let (i, _) = take(data_element_tag.padding_byte_size)(i)?;
        Ok((i, Some(name)))
    }
}

fn maybe_parse_array_name_subelement(
    endianness: nom::number::Endianness,
    supplied_name: Option<&str>,
) -> impl Fn(&[u8]) -> IResult<&[u8], String> + '_ {
    move |i| {
        let (i, element_name) = parse_array_name_subelement(endianness)(i)?;

        match (supplied_name, element_name) {
            (None, Some(v)) => Ok((i, v)),
            (Some(v), None) => Ok((i, v.to_string())),
            _ => {
                return Err(nom::Err::Failure(error_position!(
                    i,
                    // TODO
                    nom::error::ErrorKind::Tag
                )));
            }
        }
    }
}

fn parse_dimensions_array_subelement(
    endianness: nom::number::Endianness,
) -> impl Fn(&[u8]) -> IResult<&[u8], Dimensions> {
    move |i: &[u8]| {
        let (i, data_element_tag) = parse_data_element_tag(endianness)(i)?;
        if !(data_element_tag.data_type == DataType::Int32
            && data_element_tag.data_byte_size >= 8
            && data_element_tag.data_byte_size % 4 == 0)
        {
            return Err(nom::Err::Failure(error_position!(
                i,
                // TODO
                nom::error::ErrorKind::Tag
            )));
        }
        let (i, dimensions) = count(
            i32(endianness),
            (data_element_tag.data_byte_size / 4) as usize,
        )(i)?;
        let (i, _) = take(data_element_tag.padding_byte_size)(i)?;
        Ok((i, dimensions))
    }
}

fn parse_array_flags_subelement(
    endianness: nom::number::Endianness,
) -> impl Fn(&[u8]) -> IResult<&[u8], ArrayFlags> {
    move |i: &[u8]| {
        let (i, tag_data_type) = u32(endianness)(i)?;
        let (i, tag_data_len) = u32(endianness)(i)?;
        if !(tag_data_type == DataType::UInt32 as u32 && tag_data_len == 8) {
            return Err(nom::Err::Failure(error_position!(
                i,
                // TODO
                nom::error::ErrorKind::Tag
            )));
        }
        let (i, flags_and_class) = u32(endianness)(i)?;
        let (i, nzmax) = u32(endianness)(i)?;

        Ok((
            i,
            ArrayFlags {
                complex: (flags_and_class & 0x0800) != 0,
                global: (flags_and_class & 0x0400) != 0,
                logical: (flags_and_class & 0x0200) != 0,
                class: ArrayType::from_u8((flags_and_class & 0xFF) as u8).ok_or(
                    nom::Err::Failure(nom::error::Error {
                        input: i,
                        code: nom::error::ErrorKind::Tag,
                    }), // TODO
                )?,
                nzmax: nzmax as usize,
            },
        ))
    }
}

fn parse_matrix_data_element(
    endianness: nom::number::Endianness,
    supplied_name: Option<&str>,
) -> impl Fn(&[u8]) -> IResult<&[u8], DataElement> + '_ {
    move |i: &[u8]| {
        let (i, header) = parse_array_header(endianness, supplied_name)(i)?;
        match header.flags.class {
            ArrayType::Char => parse_character_array(endianness, header)(i),
            ArrayType::Struct => parse_struct(endianness, header)(i)
                .map(|(i, v)| (i, DataElement::StructureMatrix(v))),
            ArrayType::Sparse => parse_sparse_matrix_subelements(endianness, header)(i),
            x if x.numeric_data_type().is_some() => {
                parse_numeric_matrix_subelements(endianness, header)(i)
            }
            _ => {
                eprintln!("skipping unsupported {:?}", header.flags.class);
                parse_unsupported_data_element(endianness)(i)
            }
        }
    }
}

fn numeric_data_types_are_compatible(array_type: DataType, subelement_type: DataType) -> bool {
    match array_type {
        DataType::Int8 => match subelement_type {
            DataType::Int8 => true,
            _ => false,
        },
        DataType::UInt8 => match subelement_type {
            DataType::UInt8 => true,
            _ => false,
        },
        DataType::Int16 => match subelement_type {
            DataType::UInt8 | DataType::Int16 => true,
            _ => false,
        },
        DataType::UInt16 => match subelement_type {
            DataType::UInt8 | DataType::UInt16 => true,
            _ => false,
        },
        DataType::Int32 => match subelement_type {
            DataType::UInt8 | DataType::Int16 | DataType::UInt16 | DataType::Int32 => true,
            _ => false,
        },
        DataType::UInt32 => match subelement_type {
            DataType::UInt8 | DataType::Int16 | DataType::UInt16 | DataType::UInt32 => true,
            _ => false,
        },
        DataType::Int64 => match subelement_type {
            DataType::UInt8
            | DataType::Int16
            | DataType::UInt16
            | DataType::Int32
            | DataType::Int64 => true,
            _ => false,
        },
        DataType::UInt64 => match subelement_type {
            DataType::UInt8
            | DataType::Int16
            | DataType::UInt16
            | DataType::Int32
            | DataType::UInt64 => true,
            _ => false,
        },
        DataType::Single => match subelement_type {
            DataType::UInt8
            | DataType::Int16
            | DataType::UInt16
            | DataType::Int32
            | DataType::Single => true,
            _ => false,
        },
        DataType::Double => match subelement_type {
            DataType::UInt8
            | DataType::Int16
            | DataType::UInt16
            | DataType::Int32
            | DataType::Double => true,
            _ => false,
        },
        _ => false,
    }
}

fn parse_numeric_subelement(
    endianness: nom::number::Endianness,
) -> impl Fn(&[u8]) -> IResult<&[u8], NumericData> {
    move |i: &[u8]| {
        let (i, data_element_tag) = parse_data_element_tag(endianness)(i)?;
        let (i, numeric_data) = match data_element_tag.data_type {
            DataType::Int8 => map(
                count(i8, data_element_tag.data_byte_size as usize),
                NumericData::Int8,
            )(i)?,
            DataType::UInt8 => map(
                count(u8, data_element_tag.data_byte_size as usize),
                NumericData::UInt8,
            )(i)?,
            DataType::Int16 => map(
                count(
                    i16(endianness),
                    data_element_tag.data_byte_size as usize / 2,
                ),
                NumericData::Int16,
            )(i)?,
            DataType::UInt16 => map(
                count(
                    u16(endianness),
                    data_element_tag.data_byte_size as usize / 2,
                ),
                NumericData::UInt16,
            )(i)?,
            DataType::Int32 => map(
                count(
                    i32(endianness),
                    data_element_tag.data_byte_size as usize / 4,
                ),
                NumericData::Int32,
            )(i)?,
            DataType::UInt32 => map(
                count(
                    u32(endianness),
                    data_element_tag.data_byte_size as usize / 4,
                ),
                NumericData::UInt32,
            )(i)?,
            DataType::Int64 => map(
                count(
                    i64(endianness),
                    data_element_tag.data_byte_size as usize / 8,
                ),
                NumericData::Int64,
            )(i)?,
            DataType::UInt64 => map(
                count(
                    u64(endianness),
                    data_element_tag.data_byte_size as usize / 8,
                ),
                NumericData::UInt64,
            )(i)?,
            DataType::Single => map(
                count(
                    f32(endianness),
                    data_element_tag.data_byte_size as usize / 4,
                ),
                NumericData::Single,
            )(i)?,
            DataType::Double => map(
                count(
                    f64(endianness),
                    data_element_tag.data_byte_size as usize / 8,
                ),
                NumericData::Double,
            )(i)?,
            DataType::Compressed
            | DataType::Matrix
            | DataType::Utf8
            | DataType::Utf16
            | DataType::Utf32 => {
                return Err(nom::Err::Failure(error_position!(
                    i,
                    // TODO
                    nom::error::ErrorKind::Tag
                )));
            }
        };
        // Padding bytes
        let (i, _) = take(data_element_tag.padding_byte_size)(i)?;
        Ok((i, numeric_data))
    }
}

fn parse_compressed_data_element(
    endianness: nom::number::Endianness,
) -> impl Fn(&[u8]) -> IResult<&[u8], DataElement> {
    move |i: &[u8]| {
        let mut buf = Vec::new();
        Decoder::new(i)
            .map_err(|err| {
                eprintln!("{:?}", err);
                nom::Err::Failure(nom::error::Error {
                    input: i,
                    code: nom::error::ErrorKind::Tag,
                }) // TODO
            })?
            .read_to_end(&mut buf)
            .map_err(|err| {
                eprintln!("{:?}", err);
                nom::Err::Failure(nom::error::Error {
                    input: i,
                    code: nom::error::ErrorKind::Tag,
                }) // TODO
            })?;
        let (_remaining, data_element) = parse_next_data_element(endianness, None)(buf.as_slice())
            .map_err(|err| replace_err_slice(err, i))?;
        Ok((&[], data_element))
    }
}

pub type RowIndex = Vec<usize>;
pub type ColumnShift = Vec<usize>;

fn parse_numeric_matrix_subelements(
    endianness: nom::number::Endianness,
    header: ArrayHeader,
) -> impl FnOnce(&[u8]) -> IResult<&[u8], DataElement> {
    move |i: &[u8]| {
        let (i, real_part) = parse_numeric_subelement(endianness)(i)?;
        // Check that size and type of the real part are correct
        let num_required_elements = header.dimensions.iter().product::<i32>();
        let array_data_type = header.flags.class.numeric_data_type().unwrap();
        if !(real_part.len() == num_required_elements as usize
            && numeric_data_types_are_compatible(array_data_type, real_part.data_type()))
        {
            return Err(nom::Err::Failure(error_position!(
                i,
                // TODO
                nom::error::ErrorKind::Tag
            )));
        }
        let (i, imag_part) = cond(header.flags.complex, parse_numeric_subelement(endianness))(i)?;
        // Check that size and type of imaginary part are correct if present
        if let Some(imag_part) = &imag_part {
            if !(imag_part.len() == num_required_elements as usize
                && numeric_data_types_are_compatible(array_data_type, imag_part.data_type()))
            {
                return Err(nom::Err::Failure(error_position!(
                    i,
                    // TODO
                    nom::error::ErrorKind::Tag
                )));
            }
        }
        Ok((
            i,
            DataElement::NumericMatrix(Numeric {
                header,
                real_part,
                imag_part,
            }),
        ))
    }
}

fn parse_character_array(
    endianness: nom::number::Endianness,
    header: ArrayHeader,
) -> impl FnOnce(&[u8]) -> IResult<&[u8], DataElement> {
    move |i: &[u8]| {
        let (i, real_part) = parse_character_array_data(endianness, &header.dimensions)(i)?;
        let (i, imag_part) = cond(
            header.flags.complex,
            parse_character_array_data(endianness, &header.dimensions),
        )(i)?;

        Ok((
            i,
            DataElement::CharacterMatrix(Character {
                header,
                real_part,
                imag_part,
            }),
        ))
    }
}

fn parse_character_array_data(
    endianness: nom::number::Endianness,
    dimensions: &[i32],
) -> impl Fn(&[u8]) -> IResult<&[u8], CharacterData> + '_ {
    move |i| {
        let (i, data_element_tag) = parse_data_element_tag(endianness)(i)?;

        let cells = dimensions.iter().product::<i32>() as usize;

        let (i, buf) = take(data_element_tag.data_byte_size)(i)?;

        match data_element_tag.data_type {
            DataType::UInt16 => {
                assert!(data_element_tag.data_byte_size % 2 == 0);

                let (rem, str) = count(u16(endianness), cells)(buf)?;

                if !rem.is_empty() {
                    return Err(nom::Err::Failure(error_position!(
                        i,
                        // TODO
                        nom::error::ErrorKind::Tag
                    )));
                }

                Ok((i, CharacterData::NonUnicode(str)))
            }
            DataType::Utf8 => {
                let Ok(str) = String::from_utf8(buf.to_vec()) else {
                    return Err(nom::Err::Failure(error_position!(
                        i,
                        // TODO
                        nom::error::ErrorKind::Tag
                    )));
                };

                if str.chars().count() != cells {
                    return Err(nom::Err::Failure(error_position!(
                        i,
                        // TODO
                        nom::error::ErrorKind::Tag
                    )));
                }

                Ok((i, CharacterData::Unicode(str)))
            }
            DataType::Utf16 => {
                assert!(data_element_tag.data_byte_size % 2 == 0);

                let mut str = String::with_capacity(data_element_tag.data_byte_size as usize);
                let u16 = u16::<&[u8], nom::error::Error<&[u8]>>(endianness);
                let mut rem = buf;

                let mut iter = char::decode_utf16(std::iter::from_fn(|| {
                    let (r, ch) = u16(rem).ok()?;
                    rem = r;
                    Some(ch)
                }));

                for _ in 0..cells {
                    let Some(Ok(ch)) = iter.next() else {
                        return Err(nom::Err::Failure(error_position!(
                            i,
                            // TODO
                            nom::error::ErrorKind::Tag
                        )));
                    };

                    str.push(ch);
                }

                eof(rem)?;

                str.shrink_to_fit();
                Ok((i, CharacterData::Unicode(str)))
            }
            DataType::Utf32 => {
                assert!(data_element_tag.data_byte_size % 4 == 0);

                let mut str = String::with_capacity(data_element_tag.data_byte_size as usize);
                let u32 = u32(endianness);
                let mut rem = buf;

                for _ in 0..cells {
                    let (r, ch) = u32(rem)?;

                    let Some(ch) = char::from_u32(ch) else {
                        return Err(nom::Err::Failure(error_position!(
                            i,
                            // TODO
                            nom::error::ErrorKind::Tag
                        )));
                    };

                    rem = r;
                    str.push(ch);
                }

                eof(rem)?;

                str.shrink_to_fit();
                Ok((i, CharacterData::Unicode(str)))
            }
            _ => Err(nom::Err::Failure(error_position!(
                i,
                // TODO
                nom::error::ErrorKind::Tag
            ))),
        }
    }
}

fn parse_sparse_matrix_subelements(
    endianness: nom::number::Endianness,
    header: ArrayHeader,
) -> impl FnOnce(&[u8]) -> IResult<&[u8], DataElement> {
    move |i: &[u8]| {
        // Figure out the type of array
        let (i, row_index) = parse_row_index_array_subelement(endianness)(i)?;
        let (i, column_index) = parse_column_index_array_subelement(endianness)(i)?;
        let (i, real_part) = parse_numeric_subelement(endianness)(i)?;
        // Check that size of the real part is correct (can't check for type in sparse matrices)
        if !(real_part.len() == header.flags.nzmax) {
            return Err(nom::Err::Failure(error_position!(
                i,
                // TODO
                nom::error::ErrorKind::Tag
            )));
        }
        let (i, imag_part) = cond(header.flags.complex, parse_numeric_subelement(endianness))(i)?;
        // Check that size of the imaginary part is correct if present (can't check for type in sparse matrices)
        if let Some(imag_part) = &imag_part {
            if !(imag_part.len() == header.flags.nzmax as usize) {
                return Err(nom::Err::Failure(error_position!(
                    i,
                    // TODO
                    nom::error::ErrorKind::Tag
                )));
            }
        }
        Ok((
            i,
            DataElement::SparseMatrix(Sparse {
                header,
                row_index: row_index.iter().map(|&i| i as usize).collect(),
                column_index: column_index.iter().map(|&i| i as usize).collect(),
                real_part,
                imag_part,
            }),
        ))
    }
}

fn parse_row_index_array_subelement(
    endianness: nom::number::Endianness,
) -> impl Fn(&[u8]) -> IResult<&[u8], RowIndex> {
    move |i: &[u8]| {
        let (i, data_element_tag) = parse_data_element_tag(endianness)(i)?;
        if !(data_element_tag.data_type == DataType::Int32 && data_element_tag.data_byte_size > 0) {
            return Err(nom::Err::Failure(error_position!(
                i,
                // TODO
                nom::error::ErrorKind::Tag
            )));
        }
        let (i, row_index) = count(
            i32(endianness),
            (data_element_tag.data_byte_size / 4) as usize,
        )(i)?;
        let (i, _) = take(data_element_tag.padding_byte_size)(i)?;
        Ok((i, row_index.iter().map(|&i| i as usize).collect()))
    }
}

fn parse_column_index_array_subelement(
    endianness: nom::number::Endianness,
) -> impl Fn(&[u8]) -> IResult<&[u8], ColumnShift> {
    move |i: &[u8]| {
        let (i, data_element_tag) = parse_data_element_tag(endianness)(i)?;
        if !(data_element_tag.data_type == DataType::Int32 && data_element_tag.data_byte_size > 0) {
            return Err(nom::Err::Failure(error_position!(
                i,
                // TODO
                nom::error::ErrorKind::Tag
            )));
        }
        let (i, column_index) = count(
            i32(endianness),
            (data_element_tag.data_byte_size / 4) as usize,
        )(i)?;
        let (i, _) = take(data_element_tag.padding_byte_size)(i)?;
        Ok((i, column_index.iter().map(|&i| i as usize).collect()))
    }
}

pub fn replace_err_slice<'old, 'new>(
    err: nom::Err<nom::error::Error<&'old [u8]>>,
    new_slice: &'new [u8],
) -> nom::Err<nom::error::Error<&'new [u8]>> {
    match err {
        nom::Err::Error(nom::error::Error { code, .. }) => nom::Err::Error(nom::error::Error {
            code,
            input: new_slice,
        }),
        nom::Err::Failure(nom::error::Error { code, .. }) => nom::Err::Failure(nom::error::Error {
            code,
            input: new_slice,
        }),
        nom::Err::Incomplete(needed) => nom::Err::Incomplete(needed),
    }
}

fn parse_array_header(
    endianness: nom::number::Endianness,
    supplied_name: Option<&str>,
) -> impl Fn(&[u8]) -> IResult<&[u8], ArrayHeader> + '_ {
    move |i| {
        let (i, flags) = parse_array_flags_subelement(endianness)(i)?;
        let (i, dimensions) = parse_dimensions_array_subelement(endianness)(i)?;
        let (i, name) = maybe_parse_array_name_subelement(endianness, supplied_name)(i)?;

        Ok((
            i,
            ArrayHeader {
                flags,
                dimensions,
                name,
            },
        ))
    }
}

fn parse_struct(
    endianness: nom::number::Endianness,
    header: ArrayHeader,
) -> impl FnOnce(&[u8]) -> IResult<&[u8], Structure> {
    move |i| {
        let (i, max_length) = parse_struct_field_name_length(endianness)(i)?;
        let (i, field_names) = parse_struct_names(endianness, max_length)(i)?;
        let (i, values) = parse_struct_fields(endianness, &field_names)(i)?;

        Ok((
            i,
            Structure {
                header,
                field_names,
                values,
            },
        ))
    }
}

fn parse_struct_field_name_length(
    endianness: nom::number::Endianness,
) -> impl Fn(&[u8]) -> IResult<&[u8], usize> {
    move |i| {
        let (i, numeric) = parse_numeric_subelement(endianness)(i)?;

        match numeric {
            NumericData::Int8(vec) => {
                assert!(vec.len() == 1);
                Ok((i, vec[0] as usize))
            }
            NumericData::UInt8(vec) => {
                assert!(vec.len() == 1);
                Ok((i, vec[0] as usize))
            }
            NumericData::Int16(vec) => {
                assert!(vec.len() == 1);
                Ok((i, vec[0] as usize))
            }
            NumericData::UInt16(vec) => {
                assert!(vec.len() == 1);
                Ok((i, vec[0] as usize))
            }
            NumericData::Int32(vec) => {
                assert!(vec.len() == 1);
                Ok((i, vec[0] as usize))
            }
            NumericData::UInt32(vec) => {
                assert!(vec.len() == 1);
                Ok((i, vec[0] as usize))
            }
            NumericData::Int64(vec) => {
                assert!(vec.len() == 1);
                Ok((i, vec[0] as usize))
            }
            NumericData::UInt64(vec) => {
                assert!(vec.len() == 1);
                Ok((i, vec[0] as usize))
            }
            NumericData::Single(_) | NumericData::Double(_) => todo!(),
        }
    }
}

fn parse_struct_names(
    endianness: nom::number::Endianness,
    max_length: usize,
) -> impl Fn(&[u8]) -> IResult<&[u8], Vec<String>> {
    move |i| {
        let (i, data_element_tag) = parse_data_element_tag(endianness)(i)?;

        if !(data_element_tag.data_type == DataType::Int8 && data_element_tag.data_byte_size > 0) {
            return Err(nom::Err::Failure(error_position!(
                i,
                // TODO
                nom::error::ErrorKind::Tag
            )));
        }

        let (i, data) = count(u8, data_element_tag.data_byte_size as usize)(i)?;
        let (i, _) = take(data_element_tag.padding_byte_size)(i)?;

        let value_count = data.len() / max_length;
        let mut result = Vec::with_capacity(value_count);

        for idx in 0..value_count {
            let buf = &data[max_length * idx..][..max_length];

            let Ok(v) = CStr::from_bytes_until_nul(&buf) else {
                return Err(nom::Err::Failure(error_position!(
                    i,
                    // TODO
                    nom::error::ErrorKind::Tag
                )));
            };

            let Ok(str) = v.to_str() else {
                return Err(nom::Err::Failure(error_position!(
                    i,
                    // TODO
                    nom::error::ErrorKind::Tag
                )));
            };

            result.push(str.to_string());
        }

        Ok((i, result))
    }
}

fn parse_struct_field(
    endianness: nom::number::Endianness,
    name: &str,
) -> impl Fn(&[u8]) -> IResult<&[u8], DataElement> + '_ {
    parse_next_data_element(endianness, Some(name))
}

fn parse_struct_fields(
    endianness: nom::number::Endianness,
    names: &[String],
) -> impl Fn(&[u8]) -> IResult<&[u8], Vec<DataElement>> + '_ {
    move |i| {
        let mut result = Vec::with_capacity(names.len());

        let mut i = i;

        for name in names {
            let (j, val) = parse_struct_field(endianness, name)(i)?;
            result.push(val);
            i = j;
        }

        Ok((i, result))
    }
}

fn parse_unsupported_data_element(
    _endianness: nom::number::Endianness,
) -> impl Fn(&[u8]) -> IResult<&[u8], DataElement> {
    |_i: &[u8]| Ok((&[], DataElement::Unsupported))
}

#[derive(Debug)]
pub struct ParseResult {
    pub header: Header,
    pub data_elements: Vec<DataElement>,
}

pub fn parse_all(i: &[u8]) -> IResult<&[u8], ParseResult> {
    let (i, header) = parse_header(i)?;
    let endianness = if header.is_little_endian {
        nom::number::Endianness::Little
    } else {
        nom::number::Endianness::Big
    };
    let (i, data_elements) = many0(complete(parse_next_data_element(endianness, None)))(i)?;
    Ok((
        i,
        ParseResult {
            header: header,
            data_elements: data_elements,
        },
    ))
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn sparse1() {
        let data = include_bytes!("../tests/sparse1.mat");

        let (_, parsed_data) = parse_all(data).unwrap();
        let parsed_matrix_data = parsed_data.data_elements[0].clone();
        if let DataElement::SparseMatrix(Sparse {
            header,
            row_index,
            column_index,
            real_part,
            imag_part,
        }) = parsed_matrix_data
        {
            assert_eq!(header.dimensions, vec![8, 8]);
            assert_eq!(row_index, vec![5, 7, 2, 0, 1, 3, 6]);
            assert_eq!(column_index, vec![0, 1, 2, 2, 3, 4, 5, 6, 7]);
            assert_eq!(
                real_part,
                NumericData::Double(vec![2.0, 7.0, 4.0, 9.0, 5.0, 8.0, 6.0])
            );
            assert_eq!(imag_part, None);
        } else {
            panic!("Error extracting DataElement::SparseMatrix");
        }
    }

    #[test]
    fn sparse2() {
        let data = include_bytes!("../tests/sparse2.mat");

        let (_, parsed_data) = parse_all(data).unwrap();
        let parsed_matrix_data = parsed_data.data_elements[0].clone();
        if let DataElement::SparseMatrix(Sparse {
            header,
            row_index,
            column_index,
            real_part,
            imag_part,
        }) = parsed_matrix_data
        {
            assert_eq!(header.dimensions, vec![8, 8]);
            assert_eq!(row_index, vec![5, 7, 2, 0, 1, 5, 3, 6]);
            assert_eq!(column_index, vec![0, 1, 2, 2, 3, 4, 6, 7, 8]);
            assert_eq!(
                real_part,
                NumericData::Double(vec![2.0, 7.0, 4.0, 9.0, 5.0, 6.0, 8.0, 6.0])
            );
            assert_eq!(
                imag_part,
                Some(NumericData::Double(vec![
                    4.0, 0.0, 3.0, 7.0, 0.0, 1.0, 0.0, 0.0
                ]))
            );
        } else {
            panic!("Error extracting DataElement::SparseMatrix");
        }
    }
}
