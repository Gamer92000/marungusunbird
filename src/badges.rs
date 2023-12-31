use std::error::Error;
use std::fmt::Display;
use std::io;
use std::io::{Cursor, Read};

use serde::{Deserialize, Serialize};

pub struct BadgesFile {
    pub badges: Vec<Badge>,
    pub last_change: u64,
}

#[derive(Serialize, Deserialize)]
pub struct Badge {
    pub uuid: String,
    pub name: String,
    pub icon_url: String,
    pub description: String,
    pub time: u64,
}

#[derive(Debug)]
pub enum ParseError {
    Io(io::Error),
    Utf8(std::string::FromUtf8Error),
}

impl BadgesFile {
    pub fn parse(bytes: &[u8]) -> Result<Self, ParseError> {
        let len = bytes.len() as u64;
        let mut cursor = Cursor::new(bytes);
        let mut badges = Vec::new();

        cursor.read_var_int()?; // offset for ???
        let _ = cursor.read_var_int()?; // ???

        cursor.read_var_int()?; // offset for last_change
        let last_change = cursor.read_var_int()?;

        while cursor.position() < len {
            badges.push(Badge::read(&mut cursor)?);
        }

        Ok(Self {
            badges,
            last_change,
        })
    }
}

impl Badge {
    fn read(cursor: &mut Cursor<&[u8]>) -> Result<Self, ParseError> {
        let _ = cursor.read_var_int()?; // always 26 (0x1A)

        let content_len = cursor.read_var_int()?;
        let content_start = cursor.position();

        cursor.read_var_int()?; // offset for uuid
        let uuid = cursor.read_string()?;

        cursor.read_var_int()?; // offset for name
        let name = cursor.read_string()?;

        cursor.read_var_int()?; // offset for icon_url
        let icon_url = cursor.read_string()?;

        cursor.read_var_int()?; // offset for description
        let description = cursor.read_string()?;

        cursor.read_var_int()?; // offset for time
        let time = cursor.read_var_int()?;

        cursor.read_var_int()?; // offset for ???
        let _ = cursor.read_var_int()?; // ranges from 1 to 3

        // ensure we read the correct amount of bytes
        cursor.set_position(content_start + content_len);

        Ok(Self {
            uuid,
            name,
            icon_url,
            description,
            time,
        })
    }
}

trait ReadExt: Read {
    const SEGMENT_BITS: u8 = 0x7F;
    const CONTINUE_BIT: u8 = 0x80;

    fn read_u8(&mut self) -> Result<u8, ParseError> {
        let mut buffer = [0; 1];
        self.read_exact(&mut buffer)?;
        Ok(buffer[0])
    }

    fn read_var_int(&mut self) -> Result<u64, ParseError> {
        let mut result = 0;
        let mut shift = 0;

        loop {
            let byte = self.read_u8()?;

            result |= ((byte & Self::SEGMENT_BITS) as u64) << shift;

            if byte & Self::CONTINUE_BIT == 0 {
                break;
            }

            shift += 7;
        }

        Ok(result)
    }

    fn read_string(&mut self) -> Result<String, ParseError> {
        let length = self.read_var_int()?;
        let mut buffer = vec![0; length as usize];

        self.read_exact(&mut buffer)?;

        Ok(String::from_utf8(buffer)?)
    }
}

impl<R: Read + ?Sized> ReadExt for R {}

impl From<io::Error> for ParseError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<std::string::FromUtf8Error> for ParseError {
    fn from(error: std::string::FromUtf8Error) -> Self {
        Self::Utf8(error)
    }
}

impl Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseError::Io(error) => write!(f, "{}", error),
            ParseError::Utf8(error) => write!(f, "{}", error),
        }
    }
}

impl Error for ParseError {}
