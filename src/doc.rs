use std::env;
use std::fs::File;
use std::path::PathBuf;
use std::io::prelude::*;
use std::io::BufReader;
use regex::Regex;
use toml;

#[derive(PartialEq)]
enum Code {
    Rust,
    Other,
    Doc,
}

struct CrateInfo {
    name: String,
    license: Option<String>,
}

/// Given the current directory, start from there, and go up, and up, until a Cargo.toml file has
/// been found. If a Cargo.toml folder has been found, then we have found the project dir. If not,
/// nothing is found, and we return None.
pub fn project_root_dir() -> Option<PathBuf> {
    let mut currpath = env::current_dir().unwrap();

    fn _is_file(p: &PathBuf) -> bool {
        use std::fs;

        match fs::metadata(p) {
            Ok(v) => v.file_type().is_file(),
            // Errs only if not enough fs permissions, or no fs entry
            Err(..) => return false,
        }
    }

    while currpath.parent().is_some() {
        currpath.push("Cargo.toml");
        if _is_file(&currpath) {
            currpath.pop(); // found, remove toml, return project root
            return Some(currpath);
        }
        currpath.pop(); // remove toml filename
        currpath.pop(); // next dir
    }

    None
}

/// Generates readme data from `source` file
pub fn generate_readme<T: Read>(source: &mut T,
                                template: &mut Option<T>,
                                add_title: bool,
                                add_license: bool,
                                indent_headings: bool)
                                -> Result<String, String>
{
    let doc_data = extract(source, indent_headings);
    let mut readme = fold_data(doc_data);

    let crate_info = try!(get_crate_info());
    if add_license && crate_info.license.is_none() {
        return Err("There is no license in Cargo.toml".to_owned());
    }

    match template.as_mut() {
        Some(template) => process_template(template, readme, crate_info, add_title, add_license),
        None => {
            if add_title {
                readme = prepend_title(readme, &crate_info.name);
            }

            if add_license {
                readme = append_license(readme, &crate_info.license.unwrap());
            }

            Ok(readme)
        }
    }
}

/// Extracts the doc comments as a Vec of lines
///
/// Doc tests are automatically transformed into '```rust'.
/// Lines that would not show in rust docs are not returned.
fn extract<T: Read>(source: &mut T, indent_headings: bool) -> Vec<String> {
    let reader = BufReader::new(source);

    // Is this code block rust?
    let re_code_rust = Regex::new(r"^//! ```(no_run|ignore|should_panic)?$").unwrap();
    // Is this code block a language other than rust?
    let re_code_other = Regex::new(r"//! ```\w+").unwrap();

    let mut section = Code::Doc;

    reader.lines()
          .filter_map(|line| {
              let mut line = line.unwrap();
              if line.starts_with("//!") {

                  if section == Code::Doc && re_code_rust.is_match(&line) {
                      section = Code::Rust;

                      return Some("```rust".to_owned());
                  } else if section == Code::Doc && re_code_other.is_match(&line) {
                      section = Code::Other;
                  } else if section != Code::Doc && line == "//! ```" {
                      section = Code::Doc;

                      return Some("```".to_owned());
                  }

                  // If line is hidden in documentation, it is also hidden in README
                  if section == Code::Rust && line.starts_with("//! # ") {
                      return None;
                  }

                  // Remove leading '//!' before returning the line
                  if line.trim() == "//!" {
                      line = String::new();
                  } else {
                      line = line[4..].to_owned();
                      // If we should indent headings, only do this outside code blocks
                      if indent_headings && section == Code::Doc && line.starts_with("#") {
                          line.insert(0, '#');
                      }
                  }

                  Some(line)
              } else {
                  return None;
              }
          })
          .collect()
}

/// Renders the template
///
/// This is not a real template engine, it just processes a few substitutions.
fn process_template<T: Read>(template: &mut T,
                             mut readme: String,
                             crate_info: CrateInfo,
                             add_title: bool,
                             add_license: bool)
                             -> Result<String, String> {

    let mut template = try!(get_template(template));
    template = template.trim_right_matches("\n").to_owned();

    if add_title && !template.contains("{{crate}}") {
        readme = prepend_title(readme, &crate_info.name);
    } else {
        template = template.replace("{{crate}}", &crate_info.name);
    }

    if add_license && !template.contains("{{license}}") {
        readme = append_license(readme, &crate_info.license.unwrap());
    } else if template.contains("{{license}}") {
        if crate_info.license.is_none() {
            return Err("`{{license}}` found in template but there is no license in Cargo.toml".to_owned());
        }
        template = template.replace("{{license}}", &crate_info.license.unwrap())
    }

    if !template.contains("{{readme}}") {
        return Err("Missing `{{readme}}` in template".to_owned());
    }

    let result = template.replace("{{readme}}", &readme);
    Ok(result)
}

/// Try to get crate name and license from Cargo.toml
fn get_crate_info() -> Result<CrateInfo, String> {
    let current_dir = match project_root_dir() {
        Some(v) => v,
        None => return Err("Not in a rust project".into()),
    };

    let mut cargo_toml = match File::open(current_dir.join("Cargo.toml")) {
        Ok(file) => file,
        Err(_) => return Err(format!("Cargo.toml not found in '{}'",
                                     current_dir.to_string_lossy())),
    };

    let mut buf = String::new();
    match cargo_toml.read_to_string(&mut buf) {
        Err(e) => return Err(format!("{}", e)),
        Ok(_) => {}
    }

    let table = toml::Parser::new(&buf).parse().unwrap();

    // Crate name is required, right?
    let crate_name = table["package"].lookup("name").unwrap().as_str().unwrap().to_owned();
    let license = table["package"].lookup("license").map(|v| v.as_str().unwrap().to_owned());

    Ok(CrateInfo {
        name: crate_name,
        license: license,
    })
}

/// Transforms the Vec of lines into a single String
fn fold_data(data: Vec<String>) -> String {
    if data.len() < 1 {
        String::new()
    } else if data.len() < 2 {
        data[0].to_owned()
    } else {
        data[1..].into_iter().fold(data[0].to_owned(), |acc, line| format!("{}\n{}", acc, line))
    }
}

fn get_template<T: Read>(template: &mut T) -> Result<String, String> {
    let mut template_string = String::new();
    match template.read_to_string(&mut template_string) {
        Err(e) => return Err(format!("Error: {}", e)),
        _ => {}
    }

    Ok(template_string)
}

fn prepend_title(readme: String, crate_name: &str) -> String {
    let mut new_readme = format!("# {}\n\n", crate_name);
    new_readme.push_str(&readme);

    new_readme
}

fn append_license(readme: String, license: &str) -> String {
    let mut new_readme = String::new();
    new_readme.push_str(&format!("{}\n\nLicense: {}", &readme, &license));

    new_readme
}

#[cfg(test)]
mod tests {
    use std::cmp;
    use std::io::{self, Read};

    struct StringIO {
        data: String,
        pos: usize,
    }

    impl Read for StringIO {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            let mut bytes_read = 0;

            let bytes = self.data.as_bytes();
            let buf_len = cmp::min(buf.len(), bytes.len());

            let mut i = 0;
            while i < buf_len {
                println!("{}", i);
                let byte = bytes[self.pos];
                buf[i] = byte;
                bytes_read += 1;
                self.pos += 1;
                i += 1;
            }

            Ok(bytes_read)
        }
    }

    #[test]
    fn extract_indent_headings() {
        let doc_string = r#"
//! first line
//! ```
//! let rust_code = "will show";
//! # let binding = "won't show"
//! ```
//! # heading
//! ```no_run
//! let no_run = true;
//! ```
//! ```ignore
//! let ignore = true;
//! ```should_panic
//! let should_panic = true;
//! ```
//! # heading
//! ```C
//! int i = 0; // no rust code
//! ```
use std::any::Any;

fn main() {}"#;

        let mut string_io = StringIO { data: doc_string.to_owned(), pos: 0 };
        let doc_data = super::extract(&mut string_io, true);

        let expected = vec![
            "first line".to_owned(),
            "```rust".to_owned(),
            "let rust_code = \"will show\"".to_owned(),
            "```".to_owned(),
            "## heading".to_owned(),
            "```rust".to_owned(),
            "let no_run = true;".to_owned(),
            "```".to_owned(),
            "```rust".to_owned(),
            "let ignore = true;".to_owned(),
            "```".to_owned(),
            "```rust".to_owned(),
            "let should_panic = true".to_owned(),
            "```".to_owned(),
            "## heading".to_owned(),
            "```C".to_owned(),
            "int i = 0; // no rust code".to_owned(),
            "```".to_owned()
        ];

        assert_eq!(doc_data, expected);
    }
}
