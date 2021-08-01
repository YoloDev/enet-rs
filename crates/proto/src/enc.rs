mod decoder;
mod encoder;

pub use decoder::{EnetDecoder, EnetDecoderError};
pub use encoder::{EnetEncoder, EnetEncoderError};

const DELIMETER: &str = "\r\n\r\n";
