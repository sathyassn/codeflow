//! Cobertura XML coverage parser.

use std::path::Path;

use crate::testing::coverage::FileCoverage;
use crate::testing::error::TestingError;

/// Parse a Cobertura XML coverage file.
///
/// # Errors
///
/// Returns `TestingError::CoverageParseError` on I/O or XML parse failure.
pub fn parse_cobertura(path: &Path) -> Result<Vec<FileCoverage>, TestingError> {
    let content = std::fs::read_to_string(path).map_err(|e| TestingError::CoverageParseError {
        path: path.to_path_buf(),
        message: format!("failed to read file: {e}"),
    })?;

    parse_cobertura_str(&content, path)
}

/// Parse Cobertura XML from a string.
pub(crate) fn parse_cobertura_str(
    xml: &str,
    source_path: &Path,
) -> Result<Vec<FileCoverage>, TestingError> {
    use quick_xml::events::Event;
    use quick_xml::reader::Reader;

    let mut reader = Reader::from_str(xml);
    let mut results = Vec::new();
    let mut current_file: Option<String> = None;
    let mut lines_found = 0u64;
    let mut lines_hit = 0u64;

    let mut scanned = 0;
    let mut line = 1;
    loop {
        let offset = usize::try_from(reader.buffer_position())
            .unwrap_or(xml.len())
            .min(xml.len());
        line += xml.as_bytes()[scanned..offset]
            .split(|&byte| byte == b'\n')
            .count()
            - 1;
        scanned = offset;
        let invalid = |message: String| TestingError::CoverageParseError {
            path: source_path.to_path_buf(),
            message: format!("line {line}: {message}"),
        };
        match reader.read_event() {
            Ok(Event::Start(ref e) | Event::Empty(ref e)) => {
                match e.local_name().as_ref() {
                    b"class" | b"file" => {
                        // Flush previous file if any
                        if let Some(ref file_path) = current_file {
                            let percent = FileCoverage::compute_percent(lines_found, lines_hit);
                            results.push(FileCoverage {
                                path: file_path.clone(),
                                lines_found,
                                lines_hit,
                                percent,
                            });
                        }

                        let mut filename = String::new();
                        for attr in e.attributes() {
                            let attr = attr.map_err(|error| invalid(error.to_string()))?;
                            if attr.key.local_name().as_ref() == b"filename" {
                                filename = attr
                                    .decoded_and_normalized_value(
                                        quick_xml::XmlVersion::Implicit1_0,
                                        reader.decoder(),
                                    )
                                    .map_err(|error| invalid(error.to_string()))?
                                    .into_owned();
                            }
                        }

                        if !filename.is_empty() {
                            current_file = Some(filename);
                            lines_found = 0;
                            lines_hit = 0;
                        }
                    }
                    b"line" if current_file.is_some() => {
                        lines_found += 1;
                        for attr in e.attributes() {
                            let attr = attr.map_err(|error| invalid(error.to_string()))?;
                            if attr.key.local_name().as_ref() == b"hits" {
                                let value = attr
                                    .decoded_and_normalized_value(
                                        quick_xml::XmlVersion::Implicit1_0,
                                        reader.decoder(),
                                    )
                                    .map_err(|error| invalid(error.to_string()))?;
                                let hits = value
                                    .parse::<u64>()
                                    .map_err(|error| invalid(error.to_string()))?;
                                if hits > 0 {
                                    lines_hit += 1;
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                return Err(TestingError::CoverageParseError {
                    path: source_path.to_path_buf(),
                    message: format!("line {line}: XML parse error: {e}"),
                });
            }
            _ => {}
        }
    }

    // Flush last file
    if let Some(file_path) = current_file {
        let percent = FileCoverage::compute_percent(lines_found, lines_hit);
        results.push(FileCoverage {
            path: file_path,
            lines_found,
            lines_hit,
            percent,
        });
    }

    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn path(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    #[test]
    fn test_parse_cobertura_basic() {
        let xml = r#"<?xml version="1.0"?>
<coverage>
  <packages>
    <package>
      <classes>
        <class filename="src/main.rs">
          <lines>
            <line number="1" hits="1"/>
            <line number="2" hits="0"/>
            <line number="3" hits="1"/>
          </lines>
        </class>
      </classes>
    </package>
  </packages>
</coverage>"#;
        let result = parse_cobertura_str(xml, &path("cov.xml")).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].path, "src/main.rs");
        assert_eq!(result[0].lines_found, 3);
        assert_eq!(result[0].lines_hit, 2);
    }

    #[test]
    fn test_parse_cobertura_multiple_files() {
        let xml = r#"<?xml version="1.0"?>
<coverage>
  <packages>
    <package>
      <classes>
        <class filename="a.js">
          <lines><line number="1" hits="1"/></lines>
        </class>
        <class filename="b.js">
          <lines><line number="1" hits="0"/></lines>
        </class>
      </classes>
    </package>
  </packages>
</coverage>"#;
        let result = parse_cobertura_str(xml, &path("cov.xml")).unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].path, "a.js");
        assert_eq!(result[1].path, "b.js");
    }

    #[test]
    fn test_parse_cobertura_empty() {
        let xml = r#"<?xml version="1.0"?><coverage></coverage>"#;
        let result = parse_cobertura_str(xml, &path("cov.xml")).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_parse_cobertura_invalid_xml() {
        let result = parse_cobertura_str("<<< bad xml", &path("cov.xml"));
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_cobertura_file_not_found() {
        let result = parse_cobertura(&PathBuf::from("/nonexistent/cov.xml"));
        assert!(result.is_err());
    }
}

#[cfg(test)]
mod r22_regressions {
    use super::*;

    #[test]
    fn r22_cobertura_attribute_errors_name_file_and_line() {
        let path = Path::new("cov.xml");
        assert!(parse_cobertura_str("<coverage/>", path).unwrap().is_empty());
        for xml in [
            "<coverage>\n<class filename='a' filename='b'/></coverage>",
            "<coverage>\n<class filename=noquote/></coverage>",
            "<coverage>\n<class filename='a'><line hits='oops'/></class></coverage>",
        ] {
            let error = parse_cobertura_str(xml, path).unwrap_err().to_string();
            assert!(
                error.contains("cov.xml") && error.contains("line 2"),
                "{error}"
            );
        }
    }
}
