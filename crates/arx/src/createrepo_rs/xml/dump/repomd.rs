use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use quick_xml::events::{BytesDecl, BytesEnd, BytesStart, BytesText, Event};
use quick_xml::Writer;

use crate::createrepo_rs::types::{Repomd, RepomdRecord};
use crate::createrepo_rs::xml::error::XmlError;

const REPOMD_NS: &str = "http://linux.duke.edu/metadata/repo";
const RPM_NS: &str = "http://linux.duke.edu/metadata/rpm";

pub fn dump_repomd(repomd: &Repomd, output: &Path, pretty: bool) -> Result<(), XmlError> {
    let file = File::create(output)?;
    let mut writer = if pretty {
        Writer::new_with_indent(BufWriter::new(file), b' ', 2)
    } else {
        Writer::new(BufWriter::new(file))
    };

    writer.write_event(Event::Decl(BytesDecl::new("1.0", Some("UTF-8"), None)))?;

    let mut repomd_start = BytesStart::new("repomd");
    repomd_start.push_attribute(("xmlns", REPOMD_NS));
    repomd_start.push_attribute(("xmlns:rpm", RPM_NS));
    writer.write_event(Event::Start(repomd_start))?;

    write_revision_element(&mut writer, repomd.revision.as_deref())?;

    for record in &repomd.records {
        write_record_element(&mut writer, record)?;
    }

    if !repomd.distro_tags.is_empty()
        || !repomd.content_tags.is_empty()
        || !repomd.repo_tags.is_empty()
    {
        write_tags_element(&mut writer, repomd)?;
    }

    writer.write_event(Event::End(BytesEnd::new("repomd")))?;

    let mut buf_writer = writer.into_inner();
    buf_writer.flush()?;
    Ok(())
}

fn write_revision_element<W: Write>(
    writer: &mut Writer<BufWriter<W>>,
    revision: Option<&str>,
) -> Result<(), XmlError> {
    let revision_value = if let Some(rev) = revision {
        rev.to_string()
    } else {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_else(|_| std::time::Duration::from_secs(0))
            .as_secs()
            .to_string()
    };

    writer.write_event(Event::Start(BytesStart::new("revision")))?;
    writer.write_event(Event::Text(BytesText::new(&revision_value)))?;
    writer.write_event(Event::End(BytesEnd::new("revision")))?;
    Ok(())
}

fn write_record_element<W: Write>(
    writer: &mut Writer<BufWriter<W>>,
    record: &RepomdRecord,
) -> Result<(), XmlError> {
    let mut data_start = BytesStart::new("data");
    data_start.push_attribute(("type", record.record_type.as_str()));
    writer.write_event(Event::Start(data_start))?;

    if let Some(ref checksum) = record.checksum {
        let checksum_type = record.checksum_type.as_deref().unwrap_or("sha256");
        let mut checksum_start = BytesStart::new("checksum");
        checksum_start.push_attribute(("type", checksum_type));
        writer.write_event(Event::Start(checksum_start))?;
        writer.write_event(Event::Text(BytesText::new(checksum)))?;
        writer.write_event(Event::End(BytesEnd::new("checksum")))?;
    }

    if let Some(ref open_checksum) = record.open_checksum {
        let checksum_type = record.checksum_type.as_deref().unwrap_or("sha256");
        let mut open_checksum_start = BytesStart::new("open-checksum");
        open_checksum_start.push_attribute(("type", checksum_type));
        writer.write_event(Event::Start(open_checksum_start))?;
        writer.write_event(Event::Text(BytesText::new(open_checksum)))?;
        writer.write_event(Event::End(BytesEnd::new("open-checksum")))?;
    }

    let mut location_start = BytesStart::new("location");
    location_start.push_attribute(("href", record.location.as_str()));
    writer.write_event(Event::Empty(location_start))?;

    if let Some(timestamp) = record.timestamp {
        writer.write_event(Event::Start(BytesStart::new("timestamp")))?;
        writer.write_event(Event::Text(BytesText::new(&timestamp.to_string())))?;
        writer.write_event(Event::End(BytesEnd::new("timestamp")))?;
    }

    if let Some(size) = record.size {
        writer.write_event(Event::Start(BytesStart::new("size")))?;
        writer.write_event(Event::Text(BytesText::new(&size.to_string())))?;
        writer.write_event(Event::End(BytesEnd::new("size")))?;
    }

    if let Some(open_size) = record.open_size {
        if open_size != -1 {
            writer.write_event(Event::Start(BytesStart::new("open-size")))?;
            writer.write_event(Event::Text(BytesText::new(&open_size.to_string())))?;
            writer.write_event(Event::End(BytesEnd::new("open-size")))?;
        }
    }

    writer.write_event(Event::End(BytesEnd::new("data")))?;
    Ok(())
}

fn write_tags_element<W: Write>(
    writer: &mut Writer<BufWriter<W>>,
    repomd: &Repomd,
) -> Result<(), XmlError> {
    let tags_start = BytesStart::new("tags");
    writer.write_event(Event::Start(tags_start))?;

    for tag in &repomd.content_tags {
        let elem = BytesStart::new("content");
        writer.write_event(Event::Start(elem))?;
        writer.write_event(Event::Text(BytesText::new(&tag.tag)))?;
        writer.write_event(Event::End(BytesEnd::new("content")))?;
    }

    for tag in &repomd.repo_tags {
        let elem = BytesStart::new("repo");
        writer.write_event(Event::Start(elem))?;
        writer.write_event(Event::Text(BytesText::new(&tag.tag)))?;
        writer.write_event(Event::End(BytesEnd::new("repo")))?;
    }

    for tag in &repomd.distro_tags {
        let mut elem = BytesStart::new("distro");
        if let Some(ref cpeid) = tag.cpeid {
            elem.push_attribute(("cpeid", cpeid.as_str()));
        }
        writer.write_event(Event::Start(elem))?;
        writer.write_event(Event::Text(BytesText::new(&tag.tag)))?;
        writer.write_event(Event::End(BytesEnd::new("distro")))?;
    }

    writer.write_event(Event::End(BytesEnd::new("tags")))?;
    Ok(())
}
