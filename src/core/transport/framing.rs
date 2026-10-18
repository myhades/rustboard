use super::*;
pub(super) fn write_v2_response<W: Write>(writer: &mut W, message: &Message) -> Result<()> {
    if let Message::Data {
        request_id,
        offset,
        eof,
        bytes,
    } = message
    {
        write_binary_data(writer, *request_id, *offset, *eof, bytes)
    } else {
        write_frame(writer, message)
    }
}
pub(super) fn write_binary_data<W: Write>(
    writer: &mut W,
    request_id: u64,
    offset: u64,
    eof: bool,
    bytes: &[u8],
) -> Result<()> {
    ensure!(bytes.len() <= READ_WINDOW, "data too large");
    let mut header = [0u8; 21];
    header[..4].copy_from_slice(&((bytes.len() as u32 + 17) | 0x8000_0000).to_be_bytes());
    header[4..12].copy_from_slice(&request_id.to_be_bytes());
    header[12..20].copy_from_slice(&offset.to_be_bytes());
    header[20] = u8::from(eof);
    let mut buffered = std::io::BufWriter::with_capacity(MAX_CHUNK, writer);
    buffered.write_all(&header)?;
    buffered.write_all(bytes)?;
    buffered.flush()?;
    Ok(())
}

pub(super) fn read_v2_data<R: Read>(reader: &mut R, limit: usize) -> Result<DataReply> {
    let mut header = [0u8; 21];
    reader.read_exact(&mut header[..4])?;
    let size = u32::from_be_bytes(header[..4].try_into().unwrap());
    ensure!(size & 0x8000_0000 != 0, "remote read rejected");
    let size = (size & 0x7fff_ffff) as usize;
    ensure!(
        size >= 17 && size - 17 <= limit && size - 17 <= READ_WINDOW,
        "invalid data length"
    );
    reader.read_exact(&mut header[4..])?;
    ensure!(header[20] <= 1, "invalid EOF");
    let mut bytes = vec![0; size - 17];
    reader.read_exact(&mut bytes)?;
    Ok(DataReply {
        request_id: u64::from_be_bytes(header[4..12].try_into().unwrap()),
        offset: u64::from_be_bytes(header[12..20].try_into().unwrap()),
        eof: header[20] == 1,
        bytes,
    })
}

pub(super) struct DataReply {
    pub(super) request_id: u64,
    pub(super) offset: u64,
    pub(super) bytes: Vec<u8>,
    pub(super) eof: bool,
}
