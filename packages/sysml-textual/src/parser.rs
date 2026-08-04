//! Hand-written recursive-descent parser for the minimal textual grammar (see the crate-level
//! doc comment). No parser-generator dependency, same clean-room approach `alf-lite` takes for
//! its own grammar.
//!
//! ```text
//! element := kind ident anchor? block?
//! anchor  := "/*" "#" id "*/"
//! block   := "{" element* "}"
//! ```

use sysml_core::{ElementId, NodeKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub line: usize,
    pub col: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParsedElement {
    pub kind: NodeKind,
    pub name: String,
    pub anchor_id: Option<ElementId>,
    pub children: Vec<ParsedElement>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParseError {
    pub message: String,
    pub span: Span,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}: {}", self.span.line, self.span.col, self.message)
    }
}

/// Parses a full document into its top-level elements. Stops at the first syntax error — parse
/// errors don't try to recover/collect multiple, unlike [`crate::diff`]'s validation errors,
/// which do collect all of them (a syntax error makes the rest of the document's structure
/// unreliable to keep parsing against).
pub fn parse(source: &str) -> Result<Vec<ParsedElement>, ParseError> {
    let mut cursor = Cursor::new(source);
    let mut elements = Vec::new();
    cursor.skip_ws();
    while !cursor.eof() {
        elements.push(parse_element(&mut cursor)?);
        cursor.skip_ws();
    }
    Ok(elements)
}

struct Cursor {
    chars: Vec<char>,
    pos: usize,
    line: usize,
    col: usize,
}

impl Cursor {
    fn new(source: &str) -> Self {
        Self {
            chars: source.chars().collect(),
            pos: 0,
            line: 1,
            col: 1,
        }
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn peek_at(&self, offset: usize) -> Option<char> {
        self.chars.get(self.pos + offset).copied()
    }

    fn advance(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.pos += 1;
        if c == '\n' {
            self.line += 1;
            self.col = 1;
        } else {
            self.col += 1;
        }
        Some(c)
    }

    fn mark(&self) -> Span {
        Span {
            start: self.pos,
            end: self.pos,
            line: self.line,
            col: self.col,
        }
    }

    fn span_from(&self, start: Span) -> Span {
        Span {
            start: start.start,
            end: self.pos,
            line: start.line,
            col: start.col,
        }
    }

    fn skip_ws(&mut self) {
        while let Some(c) = self.peek() {
            if c.is_whitespace() {
                self.advance();
            } else {
                break;
            }
        }
    }

    fn eof(&self) -> bool {
        self.pos >= self.chars.len()
    }
}

fn parse_element(cursor: &mut Cursor) -> Result<ParsedElement, ParseError> {
    let start = cursor.mark();
    let kind_word = parse_bare_word(cursor)?;
    let kind = crate::kind_from_keyword(&kind_word).ok_or_else(|| ParseError {
        message: format!("unknown element kind '{kind_word}'"),
        span: start,
    })?;

    cursor.skip_ws();
    let name = parse_identifier(cursor)?;

    cursor.skip_ws();
    let anchor_id = try_parse_anchor(cursor)?;

    cursor.skip_ws();
    let children = if cursor.peek() == Some('{') {
        parse_block(cursor)?
    } else {
        Vec::new()
    };

    Ok(ParsedElement {
        kind,
        name,
        anchor_id,
        children,
        span: cursor.span_from(start),
    })
}

fn parse_bare_word(cursor: &mut Cursor) -> Result<String, ParseError> {
    let start = cursor.mark();
    let mut s = String::new();
    while let Some(c) = cursor.peek() {
        if c.is_ascii_alphabetic() {
            s.push(c);
            cursor.advance();
        } else {
            break;
        }
    }
    if s.is_empty() {
        return Err(ParseError {
            message: "expected an element kind keyword (e.g. 'structure')".to_string(),
            span: start,
        });
    }
    Ok(s.to_ascii_lowercase())
}

fn parse_identifier(cursor: &mut Cursor) -> Result<String, ParseError> {
    let start = cursor.mark();
    if cursor.peek() == Some('"') {
        cursor.advance();
        let mut s = String::new();
        loop {
            match cursor.advance() {
                Some('"') => return Ok(s),
                Some('\\') => match cursor.advance() {
                    Some('"') => s.push('"'),
                    Some('\\') => s.push('\\'),
                    Some(other) => {
                        s.push('\\');
                        s.push(other);
                    }
                    None => {
                        return Err(ParseError {
                            message: "unterminated escape in quoted name".to_string(),
                            span: cursor.mark(),
                        })
                    }
                },
                Some(c) => s.push(c),
                None => {
                    return Err(ParseError {
                        message: "unterminated quoted name".to_string(),
                        span: start,
                    })
                }
            }
        }
    } else {
        let mut s = String::new();
        while let Some(c) = cursor.peek() {
            if c.is_alphanumeric() || c == '_' || c == '-' {
                s.push(c);
                cursor.advance();
            } else {
                break;
            }
        }
        if s.is_empty() {
            return Err(ParseError {
                message: "expected an element name (bare identifier or \"quoted name\")"
                    .to_string(),
                span: start,
            });
        }
        Ok(s)
    }
}

/// `/* #<id> */` — the identity anchor. Not present ⇒ `Ok(None)` without consuming anything;
/// present but malformed ⇒ an error (so a typo'd anchor doesn't get silently ignored, which would
/// otherwise make an existing element look brand-new and diff to a spurious `Create`).
fn try_parse_anchor(cursor: &mut Cursor) -> Result<Option<ElementId>, ParseError> {
    if cursor.peek() != Some('/') || cursor.peek_at(1) != Some('*') {
        return Ok(None);
    }
    let start = cursor.mark();
    cursor.advance();
    cursor.advance();
    cursor.skip_ws();
    if cursor.peek() != Some('#') {
        return Err(ParseError {
            message: "expected '#' to begin an identity anchor".to_string(),
            span: start,
        });
    }
    cursor.advance();
    cursor.skip_ws();
    let mut id = String::new();
    while let Some(c) = cursor.peek() {
        if c.is_whitespace() || c == '*' {
            break;
        }
        id.push(c);
        cursor.advance();
    }
    if id.is_empty() {
        return Err(ParseError {
            message: "empty identity anchor id".to_string(),
            span: start,
        });
    }
    cursor.skip_ws();
    if cursor.peek() != Some('*') || cursor.peek_at(1) != Some('/') {
        return Err(ParseError {
            message: "unterminated identity anchor comment, expected '*/'".to_string(),
            span: start,
        });
    }
    cursor.advance();
    cursor.advance();
    Ok(Some(id))
}

fn parse_block(cursor: &mut Cursor) -> Result<Vec<ParsedElement>, ParseError> {
    cursor.advance(); // consume '{'
    let mut children = Vec::new();
    loop {
        cursor.skip_ws();
        if cursor.peek() == Some('}') {
            cursor.advance();
            return Ok(children);
        }
        if cursor.eof() {
            return Err(ParseError {
                message: "unterminated block, expected '}'".to_string(),
                span: cursor.mark(),
            });
        }
        children.push(parse_element(cursor)?);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_flat_anchored_element() {
        let elements = parse("structure Combustor /* #Combustor */ {}").unwrap();
        assert_eq!(elements.len(), 1);
        assert_eq!(elements[0].kind, NodeKind::Structure);
        assert_eq!(elements[0].name, "Combustor");
        assert_eq!(elements[0].anchor_id, Some("Combustor".to_string()));
        assert!(elements[0].children.is_empty());
    }

    #[test]
    fn parses_nested_children() {
        let src = "structure Engine /* #Engine */ {\n  structure Fan /* #Fan */ {}\n}";
        let elements = parse(src).unwrap();
        assert_eq!(elements.len(), 1);
        assert_eq!(elements[0].children.len(), 1);
        assert_eq!(elements[0].children[0].name, "Fan");
    }

    #[test]
    fn parses_quoted_name_with_spaces() {
        let elements = parse("structure \"Fan & LP Compression\" /* #F1 */ {}").unwrap();
        assert_eq!(elements[0].name, "Fan & LP Compression");
    }

    #[test]
    fn parses_element_with_no_anchor_as_unanchored() {
        let elements = parse("structure NewPart {}").unwrap();
        assert_eq!(elements[0].anchor_id, None);
    }

    #[test]
    fn element_without_block_has_no_children() {
        let elements = parse("structure Combustor /* #Combustor */").unwrap();
        assert!(elements[0].children.is_empty());
    }

    #[test]
    fn rejects_unknown_kind() {
        let err = parse("widget Thing {}").unwrap_err();
        assert!(err.message.contains("unknown element kind"));
    }

    #[test]
    fn rejects_unterminated_block() {
        let err = parse("structure Engine { structure Fan {}").unwrap_err();
        assert!(err.message.contains("unterminated block"));
    }

    #[test]
    fn rejects_malformed_anchor() {
        let err = parse("structure Combustor /* nope */ {}").unwrap_err();
        assert!(err.message.contains("identity anchor"));
    }
}
