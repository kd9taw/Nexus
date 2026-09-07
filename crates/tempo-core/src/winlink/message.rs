//! B2 message assemble/parse — the message body carried inside an FBB transfer.
//!
//! A B2 message is a header block (MID, date, sender, recipients, subject, body and attachment
//! lengths) followed by the body and each attachment laid end to end, with the header's declared
//! lengths — not any delimiter — deciding where each part stops. That makes the lengths
//! load-bearing: a body that disagrees with its declared length is a malformed message, not a
//! message to guess at.
//!
//! Bodies and attachment names are bytes. Winlink carries binary attachments and non-UTF-8 text
//! from the field, and this layer neither validates nor transcodes them.
//!
//! Not yet implemented — the module exists so the rest of `winlink` can name it.
