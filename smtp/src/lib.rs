use std::{
    borrow::Cow,
    io::{Read, Write},
};

pub fn read_line(mut stream: impl Read, buffer: &mut [u8]) -> Result<Cow<str>, String> {
    let bytes = stream.read(buffer).map_err(|e| format!("{}", e))?;
    Ok(String::from_utf8_lossy(&buffer[0..bytes]))
}

pub fn write(mut stream: impl Write, string: &str) -> Result<usize, String> {
    println!("Writing: {}", string);
    let res = stream
        .write(string.as_bytes())
        .map_err(|e| format!("Failed to write: {}", e))?;
    if res != string.len() {
        return Err(format!("Failed to write, only wrote {} bytes", res));
    }
    Ok(res)
}

pub fn write_line(mut stream: impl Write, string: &str) -> Result<usize, String> {
    let res = write(&mut stream, string)?;

    let endline = stream
        .write(b"\r\n")
        .map_err(|e| format!("Failed to write: {}", e))?;
    if endline != 2 {
        return Err(format!("Failed to CRLF, only wrote {} bytes", endline));
    }
    Ok(res)
}

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        let result = 2 + 2;
        assert_eq!(result, 4);
    }
}
