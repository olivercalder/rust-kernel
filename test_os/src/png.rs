use crate::{print, println, vga_buffer};
use alloc::vec::Vec;

// All png files must begin with bytes: [0x89, 'P', 'N', 'G', '\r', '\n', 0x1a, '\n'];
const SIGNATURE_LENGTH: usize = 8;
pub const PNG_SIGNATURE: [u8; SIGNATURE_LENGTH] = [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a];

const TYPE_OFFSET: usize = 4;   // length bytes
const DATA_OFFSET: usize = TYPE_OFFSET + 4; // length bytes + type bytes
const CRC_LENGTH: usize = 4;

const IHDR_DATA_LENGTH: usize = 13;
const FIRST_CHUNK_AFTER_IHDR: usize = SIGNATURE_LENGTH + DATA_OFFSET + IHDR_DATA_LENGTH + CRC_LENGTH;

const GREYSCALE: u8 = 0;
const TRUECOLOR: u8 = 2;
const INDEXED_COLOR: u8 = 3;
const GREYSCALE_WITH_ALPHA: u8 = 4;
const TRUECOLOR_WITH_ALPHA: u8 = 6;

const PLTE_CHANNELS: usize = 3;

const DEFAULT_COMPRESSION_LEVEL: u8 = 3;

struct Chunk {
    length: u32,
    type_arr: [u8; 4],
    data: Vec<u8>,
    crc: [u8; 4],
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

    && (info.color_type & 1 == 0)   // For now, do not allow indexed-color
    && (info.bit_depth == 8)        // For now, only accept bit depth of 8
}

fn compute_bytes_per_pixel(info: &PNGInfo) -> usize {
    let channels = channel_count(info.color_type);
    let bits_per_pixel = info.bit_depth as usize * channels;
    bits_per_pixel >> 3
}

fn compute_total_data_bytes(info: &PNGInfo) -> usize {
    info.height * (1 + compute_bytes_per_pixel(&info) * info.width)
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
    // If interlace method is 1, then scanlines vary in width by pass, so need
    // to compute the width of the current scanline according to the current
    // pass number
    let mut unfiltered: Vec<u8> = Vec::with_capacity(data.len() - info.height);
    let bytes_per_pixel: usize = compute_bytes_per_pixel(&info);
    let stride: usize = info.width * bytes_per_pixel;
    for row in 0..info.height {
        let filter_type = data[row * (stride + 1)];
        match filter_type {
            0 => {  // no change
                for col in 0..stride {
                    unfiltered.push(data[row * (stride + 1) + 1 + col]);
                }
            },
            1 => {  // sub
                for col in 0..bytes_per_pixel {
                    unfiltered.push(data[row * (stride + 1) + 1 + col]);
                }
                for col in bytes_per_pixel..stride {
                    let orig: u32 = data[row * (stride + 1) + 1 + col] as u32;
                    unfiltered.push((orig + unfiltered[row * stride + col - bytes_per_pixel] as u32) as u8);
                }
            },
            2 => {  // up
                if row == 0 {
                    for col in 0..stride {
                        unfiltered.push(data[row * (stride + 1) + 1 + col]);
                    }
                } else {
                    for col in 0..stride {
                        let orig: u32 = data[row * (stride + 1) + 1 + col] as u32;
                        unfiltered.push((orig + unfiltered[(row - 1) * stride + col] as u32) as u8);
                    }
                }
            },
            3 => {  // average
                if row == 0 {
                    for col in 0..bytes_per_pixel {
                        unfiltered.push(data[row * (stride + 1) + 1 + col]);
                    }
                    for col in bytes_per_pixel..stride {
                        let orig: u32 = data[row * (stride + 1) + 1 + col] as u32;
                        let left: u32 = unfiltered[row * stride + col - bytes_per_pixel] as u32;
                        unfiltered.push((orig + (left >> 1)) as u8);
                    }
                } else {
                    for col in 0..bytes_per_pixel {
                        let orig: u32 = data[row * (stride + 1) + 1 + col] as u32;
                        let up: u32 = unfiltered[(row - 1) * stride + col] as u32;
                        unfiltered.push((orig + (up >> 1)) as u8);
                    }
                    for col in bytes_per_pixel..stride {
                        let orig: u32 = data[row * (stride + 1) + 1 + col] as u32;
                        let left: u32 = unfiltered[row * stride + col - bytes_per_pixel] as u32;
                        let up: u32 = unfiltered[(row - 1) * stride + col] as u32;
                        unfiltered.push((orig + ((left + up) >> 1)) as u8);
                    }
                }
            },
            4 => {  // Paeth predictor
                if row == 0 {
                    for col in 0..bytes_per_pixel {
                        unfiltered.push(data[row * (stride + 1) + 1 + col]);
                    }
                    for col in bytes_per_pixel..stride {
                        let orig: u32 = data[row * (stride + 1) + 1 + col] as u32;
                        let result: u32 = paeth_predictor(
                            unfiltered[row * stride + col - bytes_per_pixel],
                            0, 0) as u32;
                        unfiltered.push((orig + result) as u8);
                    }
                } else {
                    for col in 0..bytes_per_pixel {
                        let orig: u32 = data[row * (stride + 1) + 1 + col] as u32;
                        let result: u32 = paeth_predictor(
                            0, unfiltered[(row - 1) * stride + col], 0) as u32;
                        unfiltered.push((orig + result) as u8);
                    }
                    for col in bytes_per_pixel..stride {
                        let orig: u32 = data[row * (stride + 1) + 1 + col] as u32;
                        let result: u32 = paeth_predictor(
                            unfiltered[row * stride + col - bytes_per_pixel],
                            unfiltered[(row - 1) * stride + col],
                            unfiltered[(row - 1) * stride + col - bytes_per_pixel],
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
    let mut index: usize = 0;
    let mut unfiltered: Vec<u8> = Vec::with_capacity(info.height * info.width * bytes_per_pixel);
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
        let pass_height: usize = ((info.height - (v_offset + 1)) >> (pass >> 1)) + 1;  // division is slow
        // let pass_height: usize = ((info.height - (v_offset + 1)) / v_interval) + 1;
        let pass_width: usize = ((info.width - (h_offset + 1)) >> (pass >> 1)) + 1;   // division is slow
        // let pass_width: usize = ((info.width - (h_offset + 1)) / h_interval) + 1;
        let mut row = v_offset;
        let row_interval: usize = v_interval * stride;  // byte interval between rows
        for _ in 0..pass_height {
            let filter_type = data[index];
            index += 1;
            //let mut pixel_col = h_offset;
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
    for row in 0..info.height {
        // For now, always use filter type 0 -- no-op
        filtered.push(0);
        for col in 0..info.width {
            filtered.push(data[row * info.width + col]);
        }
    }
    return filtered;
}

fn get_size_from_bytes(number_slice: &[u8]) -> usize {
    ((number_slice[0] as usize) << 24) | ((number_slice[1] as usize) << 16)
        | ((number_slice[2] as usize) << 8) | (number_slice[3] as usize)
}

/// Verify that the given raw data contains the necessary signature and IHDR
/// chunk, and return the information given by that IHDR chunk as a PNGInfo
/// struct wrapped in an Option.
///
/// If the signature or IHDR is invalid, returns None.
fn parse_ihdr(raw_data: &Vec<u8>) -> Option<PNGInfo> {
    if &raw_data[0..SIGNATURE_LENGTH] != PNG_SIGNATURE {
        return None;
    }
    if raw_data.len() < FIRST_CHUNK_AFTER_IHDR {
        return None;
    }
    let length: usize = get_size_from_bytes(&raw_data[SIGNATURE_LENGTH..SIGNATURE_LENGTH+4]);
    if length != IHDR_DATA_LENGTH {
        return None;
    }
    if &raw_data[SIGNATURE_LENGTH+TYPE_OFFSET..SIGNATURE_LENGTH+DATA_OFFSET] != "IHDR".as_bytes() {
        return None;
    }
    let offset: usize = SIGNATURE_LENGTH + DATA_OFFSET;
    Some(PNGInfo {
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
fn parse_plte(raw_data: &Vec<u8>) -> Option<Vec<u8>> {
    let plte_data: Vec<u8>;
    let mut chunk_start: usize = FIRST_CHUNK_AFTER_IHDR;
    loop {
        if raw_data.len() < chunk_start + DATA_OFFSET + CRC_LENGTH {
            return None;
        }
        let length: usize = get_size_from_bytes(&raw_data[chunk_start..chunk_start+4]);
        if &raw_data[chunk_start+TYPE_OFFSET..chunk_start+DATA_OFFSET] == "IDAT".as_bytes()
            || &raw_data[chunk_start+TYPE_OFFSET..chunk_start+DATA_OFFSET] == "IEND".as_bytes() {
            return None;
        }
        if &raw_data[chunk_start+TYPE_OFFSET..chunk_start+DATA_OFFSET] == "PLTE".as_bytes() {
            plte_data = (&raw_data[chunk_start+DATA_OFFSET..chunk_start+DATA_OFFSET+length]).to_vec();
            break;
        }
        chunk_start += DATA_OFFSET + length + CRC_LENGTH;
    }
    Some(plte_data)
}

/// Searches for and parses the IDAT chunks from the raw data. By the PNG
/// specification, there must exist at least one IDAT chunk, and if there are
/// multiple IDAT chunks, they must be contiguous.
///
/// Concatenates all the IDAT data into one Vec<u8>. Returns that data Vec in
/// an Option wrapper, or returns None if the data is missing or there is some
/// other error.
fn parse_idat(raw_data: &Vec<u8>) -> Option<Vec<u8>> {
    let mut idat_data: Vec<u8> = Vec::new();
    let mut chunk_start: usize = FIRST_CHUNK_AFTER_IHDR;
    let mut seen_idat: bool = false;
    loop {
        if raw_data.len() < chunk_start + DATA_OFFSET + CRC_LENGTH {
            return None;
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
    Some(idat_data)
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

fn compute_max_passes_to_fit(info: &PNGInfo, max_width: usize, max_height: usize) -> usize {
    // TODO
    return 8;
}

fn generate_indexed_thumbnail_using_passes(orig_info: PNGInfo, idat_data: Vec<u8>,
                                           plte_data: Vec<u8>, passes: usize) -> Vec<u8> {
    // TODO
    return Vec::new();
}

fn generate_thumbnail_using_passes(orig_info: PNGInfo, color_data: Vec<u8>, passes: usize) -> Vec<u8> {
    // TODO
    return Vec::new();
}

fn generate_thumbnail_using_averages(orig_info: PNGInfo, color_data: Vec<u8>,
                                     max_width: usize, max_height: usize,
                                     zoom_to_fill: bool) -> Vec<u8> {
    // TODO
    return Vec::new();
}

/// Generates a thumbnail for the image represented by the given raw bytes.
///
/// raw_bytes:      the unaltered bytes of the png file
/// max_width:      the maximum width allowed for the thumbnail
/// max_height:     the maximum height allowed for the thumbnail
/// use_interlace:  if true, the original image is interlaced, and a combination
///                 of passes results in an image which falls within the given
///                 maximum dimensions, then use those passes to generate the
///                 thumbnail (skipping much of the computation)
/// zoom_to_fill:   if true and either use_interface is false or interlacing
///                 cannot be used to generate a thumbnail, then fits the less
///                 constrained dimension to the corresponding maximum size, and
///                 crops the more constrained dimension to fit the its
///                 corresponding maximum size; otherwise, zooms to fit the
///                 original aspect ratio within the given maximum dimensions
///
/// If use_interlace is false (or the first pass already exceeds the given
/// maximum dimensions), the image is deinterlaced and average colors are used
/// to compute the thumbnail. In this case, zoom_to_fill determines whether the
/// more or lessconstrained dimension is stretched to its corresponding maximum.
/// If zoom_to_fill is true, then the less constrained dimension is used,
/// resulting in a thumbnail with size maximum_width x maximum_height; if
/// zoom_to_fill is false, then the more constrained dimension is used,
/// resulting in a thumbnail that is zoomed to fit, rather than fill.
///
/// Disregards all ancillary chunks (those besides IHDR, PLTE, IDAT, and IEND).
///
/// Returns the thumbnail image as a byte vector ready to be written.
/// If an error occurs, returns the original raw_bytes, since a thumbnail
/// cannot be computed.
pub fn generate_thumbnail(raw_bytes: Vec<u8>, max_width: usize,
                          max_height: usize, use_interlace: bool,
                          zoom_to_fill: bool) -> Vec<u8> {
    let png_info: PNGInfo;
    let mut plte_data: Vec<u8> = Vec::with_capacity(0);
    let idat_data: Vec<u8>;
    match parse_ihdr(&raw_bytes) {
        Some(info) => png_info = info,
        None => return raw_bytes,   // Can't parse as PNG, so return original
    }
    assert!(check_png_info_valid(&png_info) == true);

    let pass_count = if use_interlace && png_info.interlace_method == 1 {
        compute_max_passes_to_fit(&png_info, max_width, max_height)
    } else {
        8
    };
    // pass_count < 8 if interlaced passes should be used to generate the
    // thumbnail rather than averaging pixel colors

    if png_info.color_type == INDEXED_COLOR {
        let palette_data: Vec<u8>;
        match parse_plte(&raw_bytes) {
            Some(data) => plte_data = data,
            None => return raw_bytes,   // Error or missing required PLTE chunk, so return original
        }
    }
    match parse_idat(&raw_bytes) {
        Some(data) => idat_data = data,
        None => return raw_bytes,   // Error or missing required IDAT chunk, so return original
    }

    let decompressed_data = decompress_data(idat_data);
    println!("Decompressed data from IDAT blocks:");
    let expected_size = compute_total_data_bytes(&png_info);
    println!("IDAT decompressed data size equals expected size? {:?}", expected_size == decompressed_data.len());

    if pass_count < 8 {
        if png_info.color_type == INDEXED_COLOR {
            return generate_indexed_thumbnail_using_passes(png_info, decompressed_data, plte_data, pass_count);
        } else {
            return generate_thumbnail_using_passes(png_info, decompressed_data, pass_count);
        }
    }

    let unfiltered_data: Vec<u8> = if png_info.interlace_method == 1 {
        unfilter_interlaced_data(&png_info, decompressed_data)
    } else {
        unfilter_data(&png_info, decompressed_data)
    };
    println!("Unfiltered the data:");

    let color_data: Vec<u8> = if png_info.color_type == INDEXED_COLOR {
        deindex_color(unfiltered_data, plte_data)
    } else {
        unfiltered_data
    };
    println!("{:?}", color_data);
    return color_data;

    /*
    let raw_data = match png_info.interlace_method {
        0 => unfiltered_data,
        1 => deinterlace_data(&png_info, unfiltered_data),
        _ => panic!("Invalid interlace method"),
    };

    let thumbnail_data = generate_thumbnail(&png_info, raw_data);
    // don't bother interlacing output

    */
}
