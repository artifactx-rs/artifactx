use bzip2::read::BzDecoder;
use bzip2::write::BzEncoder;
use bzip2::Compression;
use std::io::{Read, Write};

pub fn decompress(data: &[u8]) -> Result<Vec<u8>, std::io::Error> {
    let mut decoder = BzDecoder::new(data);
    let mut result = Vec::new();
    decoder.read_to_end(&mut result)?;
    Ok(result)
}

pub fn compress(data: &[u8], level: i32) -> Result<Vec<u8>, std::io::Error> {
    let mut encoder = BzEncoder::new(Vec::new(), Compression::new(level as u32));
    encoder.write_all(data)?;
    encoder.finish()
}
