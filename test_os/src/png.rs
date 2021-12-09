use crate::println;
use alloc::vec::Vec;
use lazy_static::lazy_static;


// All png files must begin with bytes: [0x89, 'P', 'N', 'G', '\r', '\n', 0x1a, '\n'];
const SIGNATURE_LENGTH: usize = 8;
pub const PNG_SIGNATURE: [u8; SIGNATURE_LENGTH] = [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a];

const TYPE_OFFSET: usize = 4;   // length bytes
const TYPE_LENGTH: usize = 4;
const DATA_OFFSET: usize = TYPE_OFFSET + TYPE_LENGTH; // length bytes + type bytes
const CRC_LENGTH: usize = 4;

const IHDR_DATA_LENGTH: usize = 13;
const IHDR_TOTAL_LENGTH: usize = DATA_OFFSET + IHDR_DATA_LENGTH + CRC_LENGTH;

const IEND_TOTAL_LENGTH: usize = DATA_OFFSET + CRC_LENGTH;

const FIRST_CHUNK_AFTER_IHDR: usize = SIGNATURE_LENGTH + IHDR_TOTAL_LENGTH;

const GREYSCALE: u8 = 0;
const TRUECOLOR: u8 = 2;
const INDEXED_COLOR: u8 = 3;
const GREYSCALE_WITH_ALPHA: u8 = 4;
const TRUECOLOR_WITH_ALPHA: u8 = 6;

const PLTE_CHANNELS: usize = 3;

const DEFAULT_COMPRESSION_LEVEL: u8 = 3;

const FORCED_BIT_DEPTH: u8 = 8;


#[derive(Debug)]
pub enum ParseError {
    SIGNATURE,
    LENGTH,
    TYPE,
    ORDER,
    MISSING,
}

struct PNGInfo {
    width: usize,
    height: usize,
    bit_depth: u8,
    color_type: u8,
    compression_method: u8,
    filter_method: u8,
    interlace_method: u8,
}

struct ThumbnailGenerationInfo {
    width: usize,
    height: usize,
    ratio: f64,             // ratio of thumbnail size to original size
    x_pixel_offset: usize,  // x pixel offset into original image
    y_pixel_offset: usize,  // y pixel offset into original image
}


lazy_static! {
    static ref CRC_TABLE: [u32; 256] = {
        let mut crc_table: [u32; 256] = [0u32; 256];
        let mut c: u32;
        for n in 0..256 {
            c = n as u32;
            for _ in 0..8 {
                if c & 1 == 1 {
                    c = 0xedb88320u32 ^ (c >> 1);
                } else {
                    c >>= 1;
                }
            }
            crc_table[n] = c;
        }
        crc_table
    };
}


fn compute_crc(slice: &[u8]) -> u32 {
    let mut crc: u32 = 0xffffffffu32;
    for byte in slice {
        crc = CRC_TABLE[(crc as u8 ^ *byte) as usize] ^ (crc >> 8);
    }
    crc ^ 0xffffffffu32
}


fn channel_count(color_type: u8) -> usize {
    match color_type {
        GREYSCALE => 1,
        TRUECOLOR => 3,
        INDEXED_COLOR => 3,
        GREYSCALE_WITH_ALPHA => 2,
        TRUECOLOR_WITH_ALPHA => 4,
        _ => panic!("Invalid color type: {:?}", color_type),
    }
}


fn check_color_type_valid(color_type: u8) -> bool {
    match color_type {
        GREYSCALE => true,
        TRUECOLOR => true,
        INDEXED_COLOR => true,
        GREYSCALE_WITH_ALPHA => true,
        TRUECOLOR_WITH_ALPHA => true,
        _ => false,
    }
}


fn check_bit_depth_valid(depth: u8, color_type: u8) -> bool {
    let depth_options: Vec<u8> = match color_type {
        GREYSCALE => Vec::from([1, 2, 4, 8, 16]),
        TRUECOLOR => Vec::from([8, 16]),
        INDEXED_COLOR => Vec::from([1, 2, 4, 8]),
        GREYSCALE_WITH_ALPHA => Vec::from([8, 16]),
        TRUECOLOR_WITH_ALPHA => Vec::from([8, 16]),
        _ => panic!("Invalid color type: {:?}", color_type),
    };
    depth_options.contains(&depth)
}


fn check_interlace_method_valid(method: u8) -> bool {
    return (method & !1) == 0
}


fn check_png_info_valid(info: &PNGInfo) -> bool {
    (check_color_type_valid(info.color_type) == true)
    && (check_bit_depth_valid(info.bit_depth, info.color_type) == true)
    && (info.compression_method == 0)   // png only supports 0
    && (info.filter_method == 0)        // png only supports 0
    && (check_interlace_method_valid(info.interlace_method) == true)

    && (info.color_type & 1 == 0)           // For now, do not allow indexed-color
    && (info.bit_depth == FORCED_BIT_DEPTH) // For now, only accept bit depth of 8
}


fn compute_bytes_per_pixel(info: &PNGInfo) -> usize {
    let channels = channel_count(info.color_type);
    let bits_per_pixel = info.bit_depth as usize * channels;
    bits_per_pixel >> 3
}


fn decompress_data(data: Vec<u8>) -> Vec<u8> {
    return miniz_oxide::inflate::decompress_to_vec_zlib(data.as_slice()).expect("Failed to decompress!");
}


fn compress_data(data: Vec<u8>) -> Vec<u8> {
    return miniz_oxide::deflate::compress_to_vec_zlib(data.as_slice(), DEFAULT_COMPRESSION_LEVEL);
}


fn paeth_predictor(a: u8, b: u8, c: u8) -> u8 {
    let p: i32 = a as i32 + b as i32 - c as i32;
    let mut pa: i32 = p - a as i32;
    let mut pb: i32 = p - b as i32;
    let mut pc: i32 = p - c as i32;
    pa *= (pa >> 31) | 1;
    pb *= (pb >> 31) | 1;
    pc *= (pc >> 31) | 1;
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}


fn unfilter_data(info: &PNGInfo, data: Vec<u8>) -> Vec<u8> {
    // Unfilters and deserializes data, thus removing filter type byte from the
    // beginning of each scanline
    assert!(info.interlace_method == 0);
    let mut unfiltered: Vec<u8> = Vec::with_capacity(data.len() - info.height);
    let bytes_per_pixel: usize = compute_bytes_per_pixel(&info);
    let stride: usize = info.width * bytes_per_pixel;
    for row in 0..info.height {
        let orig_start: usize = row * (stride + 1) + 1; // first byte index into data for row
        let unf_start: usize = row * stride;            // first byte index into unfiltered for row
        let filter_type = data[orig_start - 1];         // filter type precedes first byte of row
        match filter_type {
            0 => {  // no change
                for col in 0..stride {
                    unfiltered.push(data[orig_start + col]);
                }
            },
            1 => {  // sub
                for col in 0..bytes_per_pixel {
                    unfiltered.push(data[orig_start + col]);
                }
                for col in bytes_per_pixel..stride {
                    let orig: u32 = data[orig_start + col] as u32;
                    unfiltered.push((orig + unfiltered[(unf_start + col) - bytes_per_pixel] as u32) as u8);
                }
            },
            2 => {  // up
                if row == 0 {
                    for col in 0..stride {
                        unfiltered.push(data[orig_start + col]);
                    }
                } else {
                    for col in 0..stride {
                        let orig: u32 = data[orig_start + col] as u32;
                        unfiltered.push((orig + unfiltered[unf_start - stride + col] as u32) as u8);
                    }
                }
            },
            3 => {  // average
                if row == 0 {
                    for col in 0..bytes_per_pixel {
                        unfiltered.push(data[orig_start + col]);
                    }
                    for col in bytes_per_pixel..stride {
                        let orig: u32 = data[orig_start + col] as u32;
                        let left: u32 = unfiltered[unf_start - bytes_per_pixel + col] as u32;
                        unfiltered.push((orig + (left >> 1)) as u8);
                    }
                } else {
                    for col in 0..bytes_per_pixel {
                        let orig: u32 = data[orig_start + col] as u32;
                        let up: u32 = unfiltered[unf_start - stride + col] as u32;
                        unfiltered.push((orig + (up >> 1)) as u8);
                    }
                    for col in bytes_per_pixel..stride {
                        let orig: u32 = data[orig_start + col] as u32;
                        let left: u32 = unfiltered[unf_start - bytes_per_pixel + col] as u32;
                        let up: u32 = unfiltered[unf_start - stride + col] as u32;
                        unfiltered.push((orig + ((left + up) >> 1)) as u8);
                    }
                }
            },
            4 => {  // Paeth predictor
                if row == 0 {
                    for col in 0..bytes_per_pixel {
                        unfiltered.push(data[orig_start + col]);
                    }
                    for col in bytes_per_pixel..stride {
                        let orig: u32 = data[orig_start + col] as u32;
                        let result: u32 = paeth_predictor(
                            unfiltered[unf_start - bytes_per_pixel + col],
                            0, 0) as u32;
                        unfiltered.push((orig + result) as u8);
                    }
                } else {
                    for col in 0..bytes_per_pixel {
                        let orig: u32 = data[orig_start + col] as u32;
                        let result: u32 = paeth_predictor(
                            0, unfiltered[unf_start - stride + col], 0) as u32;
                        unfiltered.push((orig + result) as u8);
                    }
                    for col in bytes_per_pixel..stride {
                        let orig: u32 = data[orig_start + col] as u32;
                        let result: u32 = paeth_predictor(
                            unfiltered[unf_start - bytes_per_pixel + col],
                            unfiltered[unf_start - stride + col],
                            unfiltered[unf_start - (stride + bytes_per_pixel) + col],
                            ) as u32;
                        unfiltered.push((orig + result) as u8);
                    }
                }
            },
            _ => panic!("Invalid filter type {:?} for row {:?}", filter_type, row),
        }
    }
    return unfiltered;
}


fn unfilter_interlaced_data(info: &PNGInfo, data: Vec<u8>) -> Vec<u8> {
    assert!(info.interlace_method == 1);
    let mut base_offset: usize = 8;
    let base_interval: usize = 8;
    let mut h_offset: usize;
    let mut v_offset: usize = 0;
    let mut h_interval: usize;
    let mut v_interval: usize = 8;
    let bytes_per_pixel: usize = compute_bytes_per_pixel(&info);
    let stride: usize = info.width * bytes_per_pixel;
    let total_bytes: usize = info.height * stride;
    let mut index: usize = 0;
    let mut unfiltered: Vec<u8> = Vec::with_capacity(total_bytes);
    for _ in 0..total_bytes {
        unfiltered.push(0u8);
    }
    // pass h_offset    v_offset    h_interval  v_interval
    // 0    0           0           8           8
    // 1    4           0           8           8
    // 2    0           4           4           8
    // 3    2           0           4           4
    // 4    0           2           2           4
    // 5    1           0           2           2
    // 6    0           1           1           2
    for pass in 0..7 {
        base_offset >>= pass & 1;
        h_offset = base_offset * (pass & 1); // if * is faster than &, <<, and ^
        // h_offset = offset & (offset << ((pass & 1) ^ 1));
        h_interval = base_interval >> (pass >> 1);
        if (v_offset >= info.height) || (h_offset >= info.width) {
            v_offset = h_offset;
            v_interval = h_interval;
            continue;
        }
        //let pass_height: usize = ((info.height - (v_offset + 1)) >> (pass >> 1)) + 1;  // division is slow
        let pass_height: usize = ((info.height - (v_offset + 1)) / v_interval) + 1;
        //let pass_width: usize = ((info.width - (h_offset + 1)) >> (pass >> 1)) + 1;   // division is slow
        let pass_width: usize = ((info.width - (h_offset + 1)) / h_interval) + 1;
        let mut row = v_offset;
        let row_interval: usize = v_interval * stride;  // byte interval between rows
        for _ in 0..pass_height {
            let filter_type = data[index];
            index += 1;
            let mut start = row * stride + h_offset * bytes_per_pixel;
            let col_interval = h_interval * bytes_per_pixel;    // byte interval between cols
            match filter_type {
                0 => {  // no change
                    for _ in 0..pass_width {
                        for byte_location in start..start+bytes_per_pixel {
                            unfiltered[byte_location] = data[index];
                            index += 1;
                        }
                        start += col_interval;
                    }
                }
                1 => {  // sub
                    for byte_location in start..start+bytes_per_pixel {
                        unfiltered[byte_location] = data[index];
                        index += 1;
                    }
                    start += col_interval;
                    for _ in 1..pass_width {
                        for byte_location in start..start+bytes_per_pixel {
                            unfiltered[byte_location] = (
                                data[index] as u32 +
                                unfiltered[byte_location - col_interval] as u32
                                ) as u8;
                            index += 1;
                        }
                        start += col_interval;
                    }
                },
                2 => {  // up
                    if row == v_offset {
                        for _ in 0..pass_width {
                            for byte_location in start..start+bytes_per_pixel {
                                unfiltered[byte_location] = data[index];
                                index += 1;
                            }
                            start += col_interval;
                        }
                    } else {
                        for _ in 0..pass_width {
                            for byte_location in start..start+bytes_per_pixel {
                                unfiltered[byte_location] = (
                                    data[index] as u32 +
                                    unfiltered[byte_location - row_interval] as u32
                                    ) as u8;
                                index += 1;
                            }
                            start += col_interval;
                        }
                    }
                },
                3 => {  // average
                    if row == v_offset {
                        for byte_location in start..start+bytes_per_pixel {
                            unfiltered[byte_location] = data[index];
                            index += 1;
                        }
                        start += col_interval;
                        for _ in 1..pass_width {
                            for byte_location in start..start+bytes_per_pixel {
                                unfiltered[byte_location] = (
                                    data[index] as u32 +
                                    (unfiltered[byte_location - col_interval] as u32 >> 1)
                                    ) as u8;
                                index += 1;
                            }
                            start += col_interval;
                        }
                    } else {
                        for byte_location in start..start+bytes_per_pixel {
                            unfiltered[byte_location] = (
                                data[index] as u32 +
                                (unfiltered[byte_location - row_interval] as u32 >> 1)
                                ) as u8;
                            index += 1;
                        }
                        start += col_interval;
                        for _ in 1..pass_width {
                            for byte_location in start..start+bytes_per_pixel {
                                unfiltered[byte_location] = (
                                    data[index] as u32 +
                                    (unfiltered[byte_location - col_interval] as u32 +
                                    unfiltered[byte_location - row_interval] as u32
                                    ) >> 1) as u8;
                                index += 1;
                            }
                            start += col_interval;
                        }
                    }
                },
                4 => {  // Paeth predictor
                    if row == v_offset {
                        for byte_location in start..start+bytes_per_pixel {
                            unfiltered[byte_location] = data[index];
                            index += 1;
                        }
                        start += col_interval;
                        for _ in 1..pass_width {
                            for byte_location in start..start+bytes_per_pixel {
                                unfiltered[byte_location] = (
                                    data[index] as u32 +
                                    paeth_predictor(unfiltered[byte_location - col_interval], 0, 0) as u32
                                    ) as u8;
                                index += 1;
                            }
                            start += col_interval;
                        }
                    } else {
                        for byte_location in start..start+bytes_per_pixel {
                            unfiltered[byte_location] = (
                                data[index] as u32 +
                                paeth_predictor(0, unfiltered[byte_location - row_interval], 0) as u32
                                ) as u8;
                            index += 1;
                        }
                        start += col_interval;
                        for _ in 1..pass_width {
                            for byte_location in start..start+bytes_per_pixel {
                                unfiltered[byte_location] = (
                                    data[index] as u32 +
                                    paeth_predictor(
                                        unfiltered[byte_location - col_interval],
                                        unfiltered[byte_location - row_interval],
                                        unfiltered[byte_location - (col_interval + row_interval)],
                                        ) as u32
                                    ) as u8;
                                index += 1;
                            }
                            start += col_interval
                        }
                    }
                },
                _ => panic!("Invalid filter type {:?} for row {:?} in pass {:?}",
                            filter_type, (row - v_offset) / v_interval, pass),
            }
            row += v_interval;
        }
        v_offset = h_offset;
        v_interval = h_interval;
    }
    return unfiltered;
}


fn filter_data(info: &PNGInfo, data: Vec<u8>) -> Vec<u8> {
    // Filters data and inserts filter type byte for each scanline
    assert!(info.interlace_method == 0);
    // If data is interlaced, then scanlines vary in length according to pass
    // number
    let mut filtered: Vec<u8> = Vec::with_capacity(data.len() + info.height);
    let bytes_per_pixel: usize = compute_bytes_per_pixel(&info);
    let stride: usize = info.width * bytes_per_pixel;
    for row in 0..info.height {
        // For now, always use filter type 0 -- no-op
        filtered.push(0);
        let row_start: usize = row * stride;
        for col in 0..stride {
            filtered.push(data[row_start + col]);
        }
    }
    return filtered;
}


fn get_size_from_bytes(number_slice: &[u8]) -> usize {
    ((number_slice[0] as usize) << 24) | ((number_slice[1] as usize) << 16)
        | ((number_slice[2] as usize) << 8) | (number_slice[3] as usize)
}


fn write_size_to_bytes(size: usize, data: &mut Vec<u8>) {
    data.push((size >> 24) as u8);
    data.push((size >> 16) as u8);
    data.push((size >> 8) as u8);
    data.push(size as u8);
}


/// Verify that the given raw data contains the necessary signature and IHDR
/// chunk, and return the information given by that IHDR chunk as a PNGInfo
/// struct wrapped in an Option.
///
/// If the signature or IHDR is invalid, returns None.
fn parse_ihdr(raw_data: &Vec<u8>) -> Result<PNGInfo, ParseError> {
    if &raw_data[0..SIGNATURE_LENGTH] != PNG_SIGNATURE {
        return Err(ParseError::SIGNATURE);
    }
    if raw_data.len() < FIRST_CHUNK_AFTER_IHDR {
        return Err(ParseError::LENGTH);
    }
    let length: usize = get_size_from_bytes(&raw_data[SIGNATURE_LENGTH..SIGNATURE_LENGTH+4]);
    if length != IHDR_DATA_LENGTH {
        return Err(ParseError::LENGTH);
    }
    if &raw_data[SIGNATURE_LENGTH+TYPE_OFFSET..SIGNATURE_LENGTH+DATA_OFFSET] != "IHDR".as_bytes() {
        return Err(ParseError::TYPE);
    }
    let offset: usize = SIGNATURE_LENGTH + DATA_OFFSET;
    Ok(PNGInfo {
        width: get_size_from_bytes(&raw_data[offset..offset+4]),
        height: get_size_from_bytes(&raw_data[offset+4..offset+8]),
        bit_depth: raw_data[offset + 8],
        color_type: raw_data[offset + 9],
        compression_method: raw_data[offset + 10],
        filter_method: raw_data[offset + 11],
        interlace_method: raw_data[offset + 12],
    })
}


/// Searches for and parses the PLTE chunk, if it exists, from the raw data.
/// Stops searching once it sees an IDAT chunk, since the PLTE chunk must
/// precede the first IDAT chunk.
///
/// Returns the data from the PLTE chunk as a slice wrapped in an Option, if
/// the PLTE chunk exists. If the chunk does not exist, returns None.
fn parse_plte(raw_data: &Vec<u8>) -> Result<Vec<u8>, ParseError> {
    let plte_data: Vec<u8>;
    let mut chunk_start: usize = FIRST_CHUNK_AFTER_IHDR;
    loop {
        if raw_data.len() < chunk_start + DATA_OFFSET + CRC_LENGTH {
            return Err(ParseError::LENGTH);
        }
        let length: usize = get_size_from_bytes(&raw_data[chunk_start..chunk_start+4]);
        if &raw_data[chunk_start+TYPE_OFFSET..chunk_start+DATA_OFFSET] == "IDAT".as_bytes()
            || &raw_data[chunk_start+TYPE_OFFSET..chunk_start+DATA_OFFSET] == "IEND".as_bytes() {
            return Err(ParseError::MISSING);
        }
        if &raw_data[chunk_start+TYPE_OFFSET..chunk_start+DATA_OFFSET] == "PLTE".as_bytes() {
            plte_data = (&raw_data[chunk_start+DATA_OFFSET..chunk_start+DATA_OFFSET+length]).to_vec();
            break;
        }
        chunk_start += DATA_OFFSET + length + CRC_LENGTH;
    }
    Ok(plte_data)
}


/// Searches for and parses the IDAT chunks from the raw data. By the PNG
/// specification, there must exist at least one IDAT chunk, and if there are
/// multiple IDAT chunks, they must be contiguous.
///
/// Concatenates all the IDAT data into one Vec<u8>. Returns that data Vec in
/// an Option wrapper, or returns None if the data is missing or there is some
/// other error.
fn parse_idat(raw_data: &Vec<u8>) -> Result<Vec<u8>, ParseError> {
    let mut idat_data: Vec<u8> = Vec::new();
    let mut chunk_start: usize = FIRST_CHUNK_AFTER_IHDR;
    let mut seen_idat: bool = false;
    loop {
        if raw_data.len() < chunk_start + DATA_OFFSET + CRC_LENGTH {
            return Err(ParseError::LENGTH);
        }
        let length: usize = get_size_from_bytes(&raw_data[chunk_start..chunk_start+4]);
        if &raw_data[chunk_start+TYPE_OFFSET..chunk_start+DATA_OFFSET] == "IDAT".as_bytes() {
            seen_idat = true;
            for byte in &raw_data[chunk_start+DATA_OFFSET..chunk_start+DATA_OFFSET+length] {
                idat_data.push(*byte);
            }
        } else if seen_idat {
            break;
        }
        chunk_start += DATA_OFFSET + length + CRC_LENGTH;
    }
    if idat_data.len() == 0 {
        return Err(ParseError::MISSING);
    }
    Ok(idat_data)
}


fn deindex_color(idat_data: Vec<u8>, plte_data: Vec<u8>) -> Vec<u8> {
    assert!(plte_data.len() % 3 == 0);
    let mut color_data: Vec<u8> = Vec::with_capacity(idat_data.len() * PLTE_CHANNELS);
    for plte_index in idat_data {
        let index: usize = plte_index as usize;
        for color_index in index*PLTE_CHANNELS..index*PLTE_CHANNELS+PLTE_CHANNELS {
            color_data.push(plte_data[color_index]);
        }
    }
    color_data
}


fn compute_orig_pixel_offset(orig_size: usize, new_size: usize, ratio: f64) -> usize {
    // Use when shrinking an image
    println!("Computing orig pixel offset when orig={:?}, new={:?}, ratio={:?}", orig_size, new_size, ratio);
    let scaled_new_size: f64 = new_size as f64 / ratio;
    println!("Scaled new size = {:?}", scaled_new_size);
    let leftover: f64 = orig_size as f64 - scaled_new_size;
    println!("Leftover pixels = {:?}", leftover);
    let offset: f64 = leftover / 2.0;
    println!("Offset = {:?}", offset);
    println!("Offset as usize = {:?}", offset as usize);
    return offset as usize;
}


fn compute_new_pixel_offset(orig_size: usize, new_size: usize, ratio: f64) -> usize {
    // Use when stretching an image
    println!("Computing new pixel offset when orig={:?}, new={:?}, ratio={:?}", orig_size, new_size, ratio);
    let scaled_orig_size: f64 = orig_size as f64 * ratio;
    println!("Scaled orig size = {:?}", scaled_orig_size);
    let leftover: f64 = scaled_orig_size - new_size as f64;
    println!("Leftover pixels = {:?}", leftover);
    let offset: f64 = leftover / 2.0;
    println!("Offset = {:?}", offset);
    println!("Offset as usize = {:?}", offset as usize);
    return offset as usize;
}


fn compute_thumbnail_generation_info(orig_info: &PNGInfo,
                                     max_width: usize,
                                     max_height: usize,
                                     zoom_to_fill: bool
                                     ) -> ThumbnailGenerationInfo {
    let mut generation_info: ThumbnailGenerationInfo = ThumbnailGenerationInfo {
        width: 0, height: 0, ratio: 0.0, x_pixel_offset: 0, y_pixel_offset: 0
    };
    let h_ratio: f64 = max_width as f64 / orig_info.width as f64;
    let v_ratio: f64 = max_height as f64 / orig_info.height as f64;
    if zoom_to_fill {
        generation_info.width = max_width;
        generation_info.height = max_height;
        if h_ratio > v_ratio {  // scale to fit max_width
            generation_info.ratio = h_ratio;
            generation_info.x_pixel_offset = 0;
            generation_info.y_pixel_offset = if h_ratio > 1.0 {
                compute_new_pixel_offset(orig_info.height, max_height, h_ratio)
            } else {
                compute_orig_pixel_offset(orig_info.height, max_height, h_ratio)
            };
        } else {    // scale to fit max_height
            generation_info.ratio = v_ratio;
            generation_info.x_pixel_offset = if v_ratio > 1.0 {
                compute_new_pixel_offset(orig_info.width, max_width, v_ratio)
            } else {
                compute_orig_pixel_offset(orig_info.width, max_width, v_ratio)
            };
            generation_info.y_pixel_offset = 0;
        }
    } else {
        generation_info.x_pixel_offset = 0;
        generation_info.y_pixel_offset = 0;
        if h_ratio < v_ratio {  // scale to fit max_width
            generation_info.ratio = h_ratio;
            generation_info.width = max_width;
            generation_info.height = (orig_info.height as f64 * h_ratio) as usize;
        } else {    // scale to fit max_height
            generation_info.ratio = v_ratio;
            generation_info.width = (orig_info.width as f64 * v_ratio) as usize;
            generation_info.height = max_height;
        }
    }
    return generation_info;
}


fn shrink_image(orig_info: &PNGInfo, orig_data: Vec<u8>,
                new_width: usize, new_height: usize, ratio: f64,
                x_pixel_offset: usize, y_pixel_offset: usize) -> Vec<u8> {
    let bytes_per_pixel = compute_bytes_per_pixel(&orig_info);
    let new_pixels: usize = new_width * new_height;
    let new_bytes: usize = new_pixels * bytes_per_pixel;
    println!("Shrinking image to {:?}x{:?} ({:?} bytes)", new_height, new_width, new_bytes);
    let mut new_data: Vec<u8> = Vec::with_capacity(new_bytes);
    let mut sums: Vec<u32> = Vec::with_capacity(new_bytes);
    let mut counts: Vec<u32> = Vec::with_capacity(new_bytes);
    for _ in 0..new_bytes {
        sums.push(0u32);
    }
    for _ in 0..new_pixels {
        counts.push(0u32);
    }
    let bytes_per_orig_row: usize = orig_info.width * bytes_per_pixel;
    let x_byte_offset: usize = x_pixel_offset * bytes_per_pixel;
    let y_byte_offset: usize = y_pixel_offset * bytes_per_orig_row;
    let orig_row_limit: usize = (new_height as f64 / ratio) as usize;
    let orig_col_limit: usize = (new_width as f64 / ratio) as usize;
    for row in 0..orig_row_limit {
        let orig_row_start_byte: usize = row * bytes_per_orig_row + y_byte_offset + x_byte_offset;
        let new_row_start_index: usize = (row as f64 * ratio) as usize * new_width;
        for col in 0..orig_col_limit {
            let orig_col_start_byte: usize = col * bytes_per_pixel + orig_row_start_byte;
            let new_col_index: usize = (col as f64 * ratio) as usize;
            let new_index: usize = new_row_start_index + new_col_index;
            let new_col_start_byte: usize = new_index * bytes_per_pixel;
            for i in 0..bytes_per_pixel {
                sums[new_col_start_byte + i] += orig_data[orig_col_start_byte + i] as u32;
            }
            counts[new_index] += 1;
        }
    }
    for byte_index in 0..new_bytes {
        new_data.push((sums[byte_index] / counts[byte_index / bytes_per_pixel]) as u8);
        // might be faster to use nested loop through bytes_per_pixel per column, to avoid second
        // division
    }
    return new_data;
}


fn stretch_image(orig_info: &PNGInfo, orig_data: Vec<u8>,
                 new_width: usize, new_height: usize, ratio: f64,
                 x_pixel_offset: usize, y_pixel_offset: usize) -> Vec<u8> {
    let bytes_per_pixel = compute_bytes_per_pixel(&orig_info);
    let new_pixels: usize = new_width * new_height;
    let new_bytes: usize = new_pixels * bytes_per_pixel;
    let mut new_data: Vec<u8> = Vec::with_capacity(new_bytes);
    for _ in 0..new_bytes {
        new_data.push(0u8);
    }
    println!("Stretching image to {:?}x{:?} ({:?} bytes)", new_height, new_width, new_bytes);
    let bytes_per_orig_row: usize = orig_info.width * bytes_per_pixel;
    let bytes_per_new_row: usize = new_width * bytes_per_pixel;
    for row in 0..new_height {
        let new_row_start_byte: usize = row * bytes_per_new_row;
        let orig_row: usize = ((row + y_pixel_offset) as f64 / ratio) as usize;
        let orig_row_start_byte: usize = orig_row * bytes_per_orig_row; // excluding the x byte offset
        for col in 0..new_width {
            let orig_col: usize = ((col + x_pixel_offset) as f64 / ratio) as usize;
            let orig_col_start_byte = orig_col * bytes_per_pixel + orig_row_start_byte;
            let new_col_start_byte: usize = col * bytes_per_pixel + new_row_start_byte;
            for i in 0..bytes_per_pixel {
                new_data[new_col_start_byte + i] = orig_data[orig_col_start_byte + i];
            }
        }
    }
    return new_data;
}


fn write_png_signature(data: &mut Vec<u8>) {
    for byte in &PNG_SIGNATURE {
        data.push(*byte);
    }
}


fn write_info_as_ihdr(info: &PNGInfo, data: &mut Vec<u8>) {
    write_size_to_bytes(IHDR_DATA_LENGTH, data);
    let slice_start: usize = data.len();
    for byte in "IHDR".as_bytes() {
        data.push(*byte);
    }
    write_size_to_bytes(info.width, data);
    write_size_to_bytes(info.height, data);
    data.push(info.bit_depth);
    data.push(info.color_type);
    data.push(info.compression_method);
    data.push(info.filter_method);
    data.push(info.interlace_method);
    let slice_end: usize = data.len();
    let slice: &[u8] = &data[slice_start..slice_end];
    write_size_to_bytes(compute_crc(slice) as usize, data);
}


fn write_data_as_idat(compressed_data: &Vec<u8>, png_data: &mut Vec<u8>) {
    write_size_to_bytes(compressed_data.len(), png_data);
    let slice_start: usize = png_data.len();
    for byte in "IDAT".as_bytes() {
        png_data.push(*byte);
    }
    for byte in compressed_data {
        png_data.push(*byte);
    }
    let slice_end: usize = png_data.len();
    let slice: &[u8] = &png_data[slice_start..slice_end];
    write_size_to_bytes(compute_crc(slice) as usize, png_data);
}


fn write_palette_as_plte(plte_data: &Vec<u8>, png_data: &mut Vec<u8>) {
    write_size_to_bytes(plte_data.len(), png_data);
    let slice_start: usize = png_data.len();
    for byte in "PLTE".as_bytes() {
        png_data.push(*byte);
    }
    for byte in plte_data {
        png_data.push(*byte);
    }
    let slice_end: usize = png_data.len();
    let slice: &[u8] = &png_data[slice_start..slice_end];
    write_size_to_bytes(compute_crc(slice) as usize, png_data);
}


fn write_iend(data: &mut Vec<u8>) {
    write_size_to_bytes(0, data);
    let slice_start: usize = data.len();
    for byte in "IEND".as_bytes() {
        data.push(*byte);
    }
    let slice_end: usize = data.len();
    let slice: &[u8] = &data[slice_start..slice_end];
    write_size_to_bytes(compute_crc(slice) as usize, data);
}


fn construct_png(thumbnail_info: PNGInfo, compressed_data: Vec<u8>) -> Vec<u8> {
    let total_size: usize = SIGNATURE_LENGTH + IHDR_TOTAL_LENGTH + DATA_OFFSET
        + compressed_data.len() + CRC_LENGTH + IEND_TOTAL_LENGTH;
    let mut png_data: Vec<u8> = Vec::with_capacity(total_size);
    write_png_signature(&mut png_data);
    write_info_as_ihdr(&thumbnail_info, &mut png_data);
    write_data_as_idat(&compressed_data, &mut png_data);
    write_iend(&mut png_data);
    return png_data;
}


fn construct_indexed_png(thumbnail_info: PNGInfo, compressed_data: Vec<u8>, plte_data: Vec<u8>) -> Vec<u8> {
    let total_size: usize = SIGNATURE_LENGTH + IHDR_TOTAL_LENGTH + DATA_OFFSET
        + compressed_data.len() + CRC_LENGTH + DATA_OFFSET + plte_data.len()
        + CRC_LENGTH + IEND_TOTAL_LENGTH;
    let mut png_data: Vec<u8> = Vec::with_capacity(total_size);
    write_png_signature(&mut png_data);
    write_info_as_ihdr(&thumbnail_info, &mut png_data);
    write_palette_as_plte(&plte_data, &mut png_data);
    write_data_as_idat(&compressed_data, &mut png_data);
    write_iend(&mut png_data);
    return png_data;
}


/// Generates a thumbnail for the image represented by the given raw bytes.
///
/// raw_bytes:      the unaltered bytes of the png file
/// max_width:      the maximum width allowed for the thumbnail
/// max_height:     the maximum height allowed for the thumbnail
/// zoom_to_fill:   if true then fits the less constrained dimension to the
///                 corresponding maximum size, and crops the more constrained
///                 dimension to fit the its corresponding maximum size;
///                 otherwise, zooms to fit the original aspect ratio within
///                 the given maximum dimensions
///
/// Average colors are used to compute the thumbnail. If the image is interlaced,
/// then the image is first deinterlaced as part of the unfiltering process.
/// The variable zoom_to_fill determines whether the more or less constrained
/// dimension is stretched to its corresponding maximum. If zoom_to_fill is true,
/// then the less constrained dimension is used, resulting in a thumbnail with
/// size maximum_width x maximum_height; if zoom_to_fill is false, then the more
/// constrained dimension is used, resulting in a thumbnail that is zoomed to
/// fit, rather than fill.
///
/// Disregards all ancillary chunks (those besides IHDR, PLTE, IDAT, and IEND).
///
/// Returns the thumbnail image as a byte vector ready to be written.
/// If an error occurs, returns the original raw_bytes, since a thumbnail
/// cannot be computed.
pub fn generate_thumbnail(raw_bytes: Vec<u8>, max_width: usize,
                          max_height: usize, zoom_to_fill: bool
                          )-> Result<Vec<u8>, ParseError> {
    let mut png_info: PNGInfo;
    match parse_ihdr(&raw_bytes) {
        Ok(info) => png_info = info,
        Err(e) => return Err(e),   // Can't parse as PNG, so return original
    }
    assert!(check_png_info_valid(&png_info) == true);

    let plte_data: Vec<u8>;
    if png_info.color_type == INDEXED_COLOR {
        match parse_plte(&raw_bytes) {
            Ok(data) => plte_data = data,
            Err(e) => return Err(e),    // Error or missing required PLTE chunk, so return original
        }
    } else { plte_data = Vec::with_capacity(0); }
    let idat_data: Vec<u8>;
    match parse_idat(&raw_bytes) {
        Ok(data) => idat_data = data,
        Err(e) => return Err(e),    // Error or missing required IDAT chunk, so return original
    }

    let decompressed_data = decompress_data(idat_data);
    println!("Decompressed data from IDAT blocks:");

    let unfiltered_data: Vec<u8>;
    if png_info.interlace_method == 1 {
        unfiltered_data = unfilter_interlaced_data(&png_info, decompressed_data);
        png_info.interlace_method = 0;
    } else {
        unfiltered_data = unfilter_data(&png_info, decompressed_data);
    };
    println!("Unfiltered the data:");

    let color_data: Vec<u8>;
    if png_info.color_type == INDEXED_COLOR {
        color_data = deindex_color(unfiltered_data, plte_data);
        png_info.color_type = TRUECOLOR;
    } else {
        color_data = unfiltered_data;
    }

    let generation_info: ThumbnailGenerationInfo =
        compute_thumbnail_generation_info(&png_info, max_width, max_height,
                                          zoom_to_fill);
    let thumbnail_color_data: Vec<u8> = if generation_info.ratio < 1.0 {
        shrink_image(&png_info,
                     color_data,
                     generation_info.width,
                     generation_info.height,
                     generation_info.ratio,
                     generation_info.x_pixel_offset,
                     generation_info.y_pixel_offset)
    } else {    // if image scale is the same (need to handle crop) or larger
        stretch_image(&png_info,
                     color_data,
                     generation_info.width,
                     generation_info.height,
                     generation_info.ratio,
                     generation_info.x_pixel_offset,
                     generation_info.y_pixel_offset)
    };
    let thumbnail_info: PNGInfo = PNGInfo {
        width: (generation_info.width),
        height: (generation_info.height),
        ..png_info
    };
    println!("Scaled original image by {:?}", generation_info.ratio);

    let filtered_data: Vec<u8> = filter_data(&thumbnail_info, thumbnail_color_data);
    let compressed_data: Vec<u8> = compress_data(filtered_data);
    let chunked_data: Vec<u8> = construct_png(thumbnail_info, compressed_data);
    return Ok(chunked_data);
}
