use bytes::Bytes;
use eyre::Result;

const MAX_BLOB_DATA_SIZE: usize = (4 * 31 + 3) * 1024 - 4;
const ENCODING_VERSION: u8 = 0;
const VERSION_OFFSET: usize = 1;
const ROUNDS: usize = 1024;

/// Encodes a blob of data into a byte array
pub fn decode_blob_data(blob: &[u8]) -> Result<Bytes> {
    let mut output = vec![0; MAX_BLOB_DATA_SIZE];

    if blob[VERSION_OFFSET] != ENCODING_VERSION {
        eyre::bail!(
            "Blob decoding: Invalid encoding version: want {}, got {}",
            ENCODING_VERSION,
            blob[VERSION_OFFSET]
        );
    }

    // decode the 3-byte big-endian length value into a 4-byte integer
    let output_len = u32::from_be_bytes([0, blob[2], blob[3], blob[4]]) as usize;
    if output_len > MAX_BLOB_DATA_SIZE {
        eyre::bail!(
            "Blob decoding: Invalid length: {} exceeds maximum {}",
            output_len,
            MAX_BLOB_DATA_SIZE
        );
    }

    output[0..27].copy_from_slice(&blob[5..32]);

    let mut output_pos = 28;
    let mut input_pos = 32;

    // buffer for the 4 6-bit chunks
    let mut encoded_byte = [0; 4];

    encoded_byte[0] = blob[0];
    for byte in encoded_byte.iter_mut().skip(1) {
        *byte = decode_field_element(&mut output_pos, &mut input_pos, blob, &mut output)?;
    }
    reassemble_bytes(&mut output_pos, encoded_byte, &mut output);

    for _ in 1..ROUNDS {
        if output_pos >= output_len {
            break;
        }

        for byte in encoded_byte.iter_mut() {
            *byte = decode_field_element(&mut output_pos, &mut input_pos, blob, &mut output)?;
        }
        reassemble_bytes(&mut output_pos, encoded_byte, &mut output);
    }

    for output_byte in output.iter().take(MAX_BLOB_DATA_SIZE).skip(output_len) {
        if output_byte != &0 {
            eyre::bail!(
                "Blob decoding: Extraneous data in field element {}",
                output_pos / 32
            );
        }
    }

    output.truncate(output_len);

    for byte in blob.iter().skip(input_pos) {
        if byte != &0 {
            eyre::bail!(
                "Blob decoding: Extraneous data in input position {}",
                input_pos
            );
        }
    }

    Ok(output.into())
}

fn decode_field_element(
    output_pos: &mut usize,
    input_pos: &mut usize,
    blob: &[u8],
    output: &mut [u8],
) -> Result<u8> {
    let result = blob[*input_pos];

    // two highest order bits of the first byte of each field element should always be 0
    if result & 0b1100_0000 != 0 {
        eyre::bail!("Blob decoding: Invalid field element");
    }

    output[*output_pos..*output_pos + 31].copy_from_slice(&blob[*input_pos + 1..*input_pos + 32]);

    *output_pos += 32;
    *input_pos += 32;

    Ok(result)
}

fn reassemble_bytes(output_pos: &mut usize, encoded_byte: [u8; 4], output: &mut [u8]) {
    *output_pos -= 1;

    let x = (encoded_byte[0] & 0b0011_1111) | ((encoded_byte[1] & 0b0011_0000) << 2);
    let y = (encoded_byte[1] & 0b0000_1111) | ((encoded_byte[3] & 0b0000_1111) << 4);
    let z = (encoded_byte[2] & 0b0011_1111) | ((encoded_byte[3] & 0b0011_0000) << 2);

    output[*output_pos - 32] = z;
    output[*output_pos - (32 * 2)] = y;
    output[*output_pos - (32 * 3)] = x;
}
