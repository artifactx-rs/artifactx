use quick_xml::events::Event;
use quick_xml::Reader;

#[derive(Debug, Clone, Default)]
pub struct RepomdRecord {
    pub type_: String,
    pub location: String,
    pub checksum: Option<String>,
    pub timestamp: Option<i64>,
    pub size: Option<i64>,
    pub open_size: Option<i64>,
    pub open_checksum: Option<String>,
}

pub fn parse_repomd(xml_data: &[u8]) -> Result<Vec<RepomdRecord>, String> {
    let mut reader = Reader::from_reader(xml_data);
    reader.config_mut().trim_text(true);

    let mut records = Vec::new();
    let mut current_record = RepomdRecord::default();
    let mut current_element = String::new();
    let mut in_data = false;

    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                if name == "data" {
                    in_data = true;
                    current_record = RepomdRecord::default();
                    for attr in e.attributes().flatten() {
                        let key = String::from_utf8_lossy(attr.key.as_ref()).to_string();
                        if key == "type" {
                            current_record.type_ = String::from_utf8_lossy(&attr.value).to_string();
                        }
                    }
                }
                current_element = name;
            }
            Ok(Event::Empty(e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                if name == "data" {
                    let mut record = RepomdRecord::default();
                    for attr in e.attributes().flatten() {
                        let key = String::from_utf8_lossy(attr.key.as_ref()).to_string();
                        if key == "type" {
                            record.type_ = String::from_utf8_lossy(&attr.value).to_string();
                        }
                    }
                    if !record.type_.is_empty() {
                        records.push(record);
                    }
                } else if name == "location" && in_data {
                    for attr in e.attributes().flatten() {
                        let key = String::from_utf8_lossy(attr.key.as_ref()).to_string();
                        if key == "href" {
                            current_record.location =
                                String::from_utf8_lossy(&attr.value).to_string();
                        }
                    }
                }
            }
            Ok(Event::Text(e)) if in_data => {
                let text = String::from_utf8_lossy(&e).to_string();
                match current_element.as_str() {
                    "checksum" => current_record.checksum = Some(text),
                    "timestamp" => current_record.timestamp = text.parse().ok(),
                    "size" => current_record.size = text.parse().ok(),
                    "open-size" => current_record.open_size = text.parse().ok(),
                    "open-checksum" => current_record.open_checksum = Some(text),
                    _ => {}
                }
            }
            Ok(Event::End(e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                if name == "data" && in_data {
                    if !current_record.type_.is_empty() {
                        records.push(current_record.clone());
                    }
                    in_data = false;
                }
                current_element.clear();
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(format!("XML parse error: {e}")),
            _ => {}
        }
    }

    Ok(records)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_repomd() {
        let xml = br#"<?xml version="1.0" encoding="UTF-8"?>
<repomd xmlns="http://linux.duke.edu/metadata/repomd" xmlns:rpm="http://linux.duke.edu/metadata/rpm">
  <data type="primary">
    <location href="repodata/primary.xml.gz"/>
    <checksum>abc123</checksum>
    <timestamp>1234567890</timestamp>
    <size>12345</size>
  </data>
</repomd>"#;

        let records = parse_repomd(xml).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].type_, "primary");
        assert_eq!(records[0].location, "repodata/primary.xml.gz");
        assert_eq!(records[0].checksum.as_deref(), Some("abc123"));
    }
}
