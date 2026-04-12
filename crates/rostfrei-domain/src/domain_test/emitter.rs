use std::io::{self, Write};

use super::{DomainTestDescriptor, projection};

const PREFIX: &str = "ROSTFREI_DOMAIN_TEST_METADATA_V1\t";

pub fn emit_domain_test_metadata(descriptor: DomainTestDescriptor) -> io::Result<()> {
    let stdout = io::stdout();
    let mut stdout = stdout.lock();
    write_metadata(&mut stdout, descriptor)
}

pub(super) fn write_metadata(
    writer: &mut impl Write,
    descriptor: DomainTestDescriptor,
) -> io::Result<()> {
    let frame = format!("\n{PREFIX}{}\n", projection::compact(descriptor));
    writer.write_all(frame.as_bytes())?;
    writer.flush()
}
