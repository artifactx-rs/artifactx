use std::io::Write;
use std::path::Path;

use quick_xml::events::{BytesDecl, BytesEnd, BytesStart, BytesText, Event};
use quick_xml::Writer;

use crate::createrepo_rs::compression::{
    bzip2_compress, gzip_compress, xz_compress, zstd_compress,
};
use crate::createrepo_rs::types::{ChangelogEntry, CompressionType, Package};
use crate::createrepo_rs::xml::error::XmlError;

const METADATA_NS: &str = "http://linux.duke.edu/metadata/other";

pub fn dump_other_xml(packages: &[Package], pretty: bool) -> Result<Vec<u8>, XmlError> {
    let estimated = packages.len() * 128;
    let mut writer = if pretty {
        Writer::new_with_indent(Vec::with_capacity(estimated), b' ', 2)
    } else {
        Writer::new(Vec::with_capacity(estimated))
    };

    writer.write_event(Event::Decl(BytesDecl::new("1.0", Some("UTF-8"), None)))?;

    let mut other_start = BytesStart::new("otherdata");
    other_start.push_attribute(("xmlns", METADATA_NS));
    let package_count = packages.len().to_string();
    other_start.push_attribute(("packages", package_count.as_str()));
    writer.write_event(Event::Start(other_start))?;

    for package in packages {
        write_package_element(&mut writer, package)?;
    }

    writer.write_event(Event::End(BytesEnd::new("otherdata")))?;

    Ok(writer.into_inner())
}

pub fn dump_other(
    packages: &[Package],
    output: &Path,
    compression: CompressionType,
    pretty: bool,
) -> Result<(), XmlError> {
    let xml_content = dump_other_xml(packages, pretty)?;

    if compression == CompressionType::None {
        std::fs::write(output, xml_content)?;
    } else {
        let compressed = compress_bytes(&xml_content, compression)?;
        std::fs::write(output, compressed)?;
    }

    Ok(())
}

fn write_package_element<W: Write>(
    writer: &mut Writer<W>,
    package: &Package,
) -> Result<(), XmlError> {
    let mut pkg_start = BytesStart::new("package");
    pkg_start.push_attribute(("pkgid", package.pkgid.as_str()));
    pkg_start.push_attribute(("name", package.name.as_str()));
    pkg_start.push_attribute(("arch", package.arch.as_str()));
    writer.write_event(Event::Start(pkg_start))?;

    let _ = write_version_element(writer, package);
    let _ = write_changelog_elements(writer, &package.changelogs);

    writer.write_event(Event::End(BytesEnd::new("package")))?;
    Ok(())
}

fn write_version_element<W: Write>(
    writer: &mut Writer<W>,
    package: &Package,
) -> Result<(), XmlError> {
    let mut version_start = BytesStart::new("version");
    let epoch_val = package.epoch.unwrap_or(0);
    version_start.push_attribute(("epoch", epoch_val.to_string().as_str()));
    version_start.push_attribute(("ver", package.version.as_str()));
    version_start.push_attribute(("rel", package.release.as_str()));
    writer.write_event(Event::Empty(version_start))?;
    Ok(())
}

fn write_changelog_elements<W: Write>(
    writer: &mut Writer<W>,
    changelogs: &[ChangelogEntry],
) -> Result<(), XmlError> {
    for entry in changelogs {
        let mut changelog_start = BytesStart::new("changelog");
        changelog_start.push_attribute(("author", entry.author.as_str()));
        changelog_start.push_attribute(("date", entry.date.to_string().as_str()));
        writer.write_event(Event::Start(changelog_start))?;
        writer.write_event(Event::Text(BytesText::new(&entry.content)))?;
        writer.write_event(Event::End(BytesEnd::new("changelog")))?;
    }
    Ok(())
}

fn compress_bytes(content: &[u8], compression: CompressionType) -> Result<Vec<u8>, XmlError> {
    match compression {
        CompressionType::Gzip => gzip_compress(content, 6).map_err(XmlError::IoError),
        CompressionType::Bzip2 => bzip2_compress(content, 6).map_err(XmlError::IoError),
        CompressionType::Xz => xz_compress(content, 6).map_err(XmlError::IoError),
        CompressionType::Zstd => zstd_compress(content, 6).map_err(XmlError::IoError),
        CompressionType::None => Ok(content.to_vec()),
    }
}
