use std::io::Write;
use std::path::Path;

use quick_xml::events::{BytesDecl, BytesEnd, BytesStart, BytesText, Event};
use quick_xml::Writer;

use crate::createrepo_rs::compression::{
    bzip2_compress, gzip_compress, xz_compress, zstd_compress,
};
use crate::createrepo_rs::types::{
    ChecksumType, CompressionType, Dependency, Package, PackageFile,
};
use crate::createrepo_rs::xml::error::XmlError;

const METADATA_NS: &str = "http://linux.duke.edu/metadata/common";
const RPM_NS: &str = "http://linux.duke.edu/metadata/rpm";

/// Dump primary XML metadata to a byte vector.
/// Returns the uncompressed XML content.
pub fn dump_primary_xml(packages: &[Package], pretty: bool) -> Result<Vec<u8>, XmlError> {
    let estimated = packages.len() * 512;
    let mut writer = if pretty {
        Writer::new_with_indent(Vec::with_capacity(estimated), b' ', 2)
    } else {
        Writer::new(Vec::with_capacity(estimated))
    };

    writer.write_event(Event::Decl(BytesDecl::new("1.0", Some("UTF-8"), None)))?;

    let mut metadata_start = BytesStart::new("metadata");
    metadata_start.push_attribute(("xmlns", METADATA_NS));
    metadata_start.push_attribute(("xmlns:rpm", RPM_NS));
    let package_count = packages.len().to_string();
    metadata_start.push_attribute(("packages", package_count.as_str()));
    writer.write_event(Event::Start(metadata_start))?;

    for package in packages {
        write_package_element(&mut writer, package)?;
    }

    writer.write_event(Event::End(BytesEnd::new("metadata")))?;

    Ok(writer.into_inner())
}

pub fn dump_primary(
    packages: &[Package],
    output: &Path,
    compression: CompressionType,
    pretty: bool,
) -> Result<(), XmlError> {
    let xml_content = dump_primary_xml(packages, pretty)?;

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
    pkg_start.push_attribute(("type", "rpm"));
    writer.write_event(Event::Start(pkg_start))?;

    write_text_element(writer, "name", &package.name)?;
    write_text_element(writer, "arch", &package.arch)?;
    let _ = write_version_element(writer, package);
    let _ = write_checksum_element(writer, package);
    write_text_element(writer, "summary", package.summary.as_deref().unwrap_or(""))?;
    write_text_element(
        writer,
        "description",
        package.description.as_deref().unwrap_or(""),
    )?;
    write_text_element(
        writer,
        "packager",
        package.packager.as_deref().unwrap_or(""),
    )?;
    write_text_element(writer, "url", package.url.as_deref().unwrap_or(""))?;

    let _ = write_time_element(writer, package);
    let _ = write_size_element(writer, package);
    let _ = write_location_element(writer, package);
    let _ = write_format_element(writer, package);

    writer.write_event(Event::End(BytesEnd::new("package")))?;
    Ok(())
}

fn write_version_element<W: Write>(
    writer: &mut Writer<W>,
    package: &Package,
) -> Result<(), XmlError> {
    let mut version_start = BytesStart::new("version");
    // Always output epoch (C version always includes it)
    let epoch_val = package.epoch.unwrap_or(0);
    version_start.push_attribute(("epoch", epoch_val.to_string().as_str()));
    version_start.push_attribute(("ver", package.version.as_str()));
    version_start.push_attribute(("rel", package.release.as_str()));
    writer.write_event(Event::Empty(version_start))?;
    Ok(())
}

fn write_checksum_element<W: Write>(
    writer: &mut Writer<W>,
    package: &Package,
) -> Result<(), XmlError> {
    let mut checksum_start = BytesStart::new("checksum");
    checksum_start.push_attribute(("type", checksum_type_str(package.checksum_type)));
    checksum_start.push_attribute(("pkgid", "YES"));
    writer.write_event(Event::Start(checksum_start))?;
    writer.write_event(Event::Text(BytesText::new(&package.pkgid)))?;
    writer.write_event(Event::End(BytesEnd::new("checksum")))?;
    Ok(())
}

fn write_time_element<W: Write>(writer: &mut Writer<W>, package: &Package) -> Result<(), XmlError> {
    let mut time_start = BytesStart::new("time");
    time_start.push_attribute(("file", package.time_file.to_string().as_str()));
    time_start.push_attribute(("build", package.time_build.to_string().as_str()));
    writer.write_event(Event::Empty(time_start))?;
    Ok(())
}

fn write_size_element<W: Write>(writer: &mut Writer<W>, package: &Package) -> Result<(), XmlError> {
    let mut size_start = BytesStart::new("size");
    size_start.push_attribute(("package", package.size_package.to_string().as_str()));
    size_start.push_attribute(("installed", package.size_installed.to_string().as_str()));
    size_start.push_attribute(("archive", package.size_archive.to_string().as_str()));
    writer.write_event(Event::Empty(size_start))?;
    Ok(())
}

fn write_location_element<W: Write>(
    writer: &mut Writer<W>,
    package: &Package,
) -> Result<(), XmlError> {
    let mut location_start = BytesStart::new("location");
    location_start.push_attribute(("href", package.location.as_str()));
    writer.write_event(Event::Empty(location_start))?;
    Ok(())
}

fn write_format_element<W: Write>(
    writer: &mut Writer<W>,
    package: &Package,
) -> Result<(), XmlError> {
    let format_start = BytesStart::new("format");
    writer.write_event(Event::Start(format_start))?;

    write_text_element(
        writer,
        "rpm:license",
        package.license.as_deref().unwrap_or(""),
    )?;
    write_text_element(
        writer,
        "rpm:vendor",
        package.vendor.as_deref().unwrap_or(""),
    )?;
    write_text_element(writer, "rpm:group", package.group.as_deref().unwrap_or(""))?;
    write_text_element(
        writer,
        "rpm:buildhost",
        package.buildhost.as_deref().unwrap_or(""),
    )?;
    write_text_element(
        writer,
        "rpm:sourcerpm",
        package.sourcerpm.as_deref().unwrap_or(""),
    )?;

    let _ = write_header_range_element(writer, package);
    let _ = write_pco_elements(writer, "rpm:provides", &package.provides);
    let _ = write_pco_elements(writer, "rpm:requires", &package.requires);
    let _ = write_pco_elements(writer, "rpm:conflicts", &package.conflicts);
    let _ = write_pco_elements(writer, "rpm:obsoletes", &package.obsoletes);
    let _ = write_pco_elements(writer, "rpm:suggests", &package.suggests);
    let _ = write_pco_elements(writer, "rpm:enhances", &package.enhances);
    let _ = write_pco_elements(writer, "rpm:recommends", &package.recommends);
    let _ = write_pco_elements(writer, "rpm:supplements", &package.supplements);

    let _ = write_file_elements(writer, &package.files);

    writer.write_event(Event::End(BytesEnd::new("format")))?;
    Ok(())
}

fn write_header_range_element<W: Write>(
    writer: &mut Writer<W>,
    package: &Package,
) -> Result<(), XmlError> {
    let mut header_start = BytesStart::new("rpm:header-range");
    if let Some(start) = package.header_start {
        header_start.push_attribute(("start", start.to_string().as_str()));
    }
    if let Some(end) = package.header_end {
        header_start.push_attribute(("end", end.to_string().as_str()));
    }
    writer.write_event(Event::Empty(header_start))?;
    Ok(())
}

fn write_pco_elements<W: Write>(
    writer: &mut Writer<W>,
    element_name: &str,
    dependencies: &[Dependency],
) -> Result<(), XmlError> {
    if dependencies.is_empty() {
        return Ok(());
    }

    let pco_start = BytesStart::new(element_name);
    writer.write_event(Event::Start(pco_start))?;

    for dep in dependencies {
        let mut entry_start = BytesStart::new("rpm:entry");
        entry_start.push_attribute(("name", dep.name.as_str()));

        if !dep.flags.is_empty() {
            entry_start.push_attribute(("flags", dep.flags.as_str()));
        }

        // Always output epoch (C version always includes it)
        let epoch_val = dep.epoch.unwrap_or(0);
        entry_start.push_attribute(("epoch", epoch_val.to_string().as_str()));

        if let Some(ref version) = dep.version {
            entry_start.push_attribute(("ver", version.as_str()));
        }
        if let Some(ref release) = dep.release {
            entry_start.push_attribute(("rel", release.as_str()));
        }

        if dep.pre {
            entry_start.push_attribute(("pre", "1"));
        }

        writer.write_event(Event::Empty(entry_start))?;
    }

    writer.write_event(Event::End(BytesEnd::new(element_name)))?;
    Ok(())
}

fn write_file_elements<W: Write>(
    writer: &mut Writer<W>,
    files: &[PackageFile],
) -> Result<(), XmlError> {
    for file in files {
        let mut file_start = BytesStart::new("file");
        let file_type = file.file_type.as_str();
        if !file_type.is_empty() && file_type != "file" {
            file_start.push_attribute(("type", file_type));
        }
        writer.write_event(Event::Start(file_start))?;
        writer.write_event(Event::Text(BytesText::new(&file.path)))?;
        writer.write_event(Event::End(BytesEnd::new("file")))?;
    }
    Ok(())
}

fn write_text_element<W: Write>(
    writer: &mut Writer<W>,
    name: &str,
    text: &str,
) -> Result<(), XmlError> {
    writer.write_event(Event::Start(BytesStart::new(name)))?;
    writer.write_event(Event::Text(BytesText::new(text)))?;
    writer.write_event(Event::End(BytesEnd::new(name)))?;
    Ok(())
}

const fn checksum_type_str(ct: ChecksumType) -> &'static str {
    match ct {
        ChecksumType::Md5 => "md5",
        ChecksumType::Sha1 => "sha1",
        ChecksumType::Sha224 => "sha224",
        ChecksumType::Sha256 => "sha256",
        ChecksumType::Sha384 => "sha384",
        ChecksumType::Sha512 => "sha512",
        ChecksumType::Unknown => "sha256",
    }
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
