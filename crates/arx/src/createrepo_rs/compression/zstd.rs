pub fn decompress(data: &[u8]) -> Result<Vec<u8>, std::io::Error> {
    zstd::decode_all(data).map_err(std::io::Error::other)
}

pub fn compress(data: &[u8], level: i32) -> Result<Vec<u8>, std::io::Error> {
    let encoded = zstd::encode_all(data, level).map_err(std::io::Error::other)?;
    Ok(encoded)
}
