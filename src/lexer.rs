//! Text to tokens.
//!
//! Besides splitting the text, the lexer decides which line breaks end a
//! statement, makes a `?` or `!` suffix part of a lowercase name, decodes
//! string escapes, and lexes the expression inside each `#{…}` of a
//! double-quoted string into tokens of its own. Every span is a byte range
//! of the file, including those of tokens inside an interpolation.

use crate::diag::Diagnostic;
use crate::source::{FileId, Span};

/// The largest magnitude of an integer literal: `2^63`, which is in range
/// only when a `-` directly before it makes it negative.
pub const INT_LIMIT: u64 = 1 << 63;

#[derive(Clone, Debug, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
    /// Whether whitespace, a comment or a line break comes between the
    /// previous token and this one.
    pub space_before: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum TokenKind {
    /// The magnitude of an integer literal, at most [`INT_LIMIT`].
    Int(u64),
    Float(f64),
    /// A string literal with its escapes decoded. Adjacent text is one
    /// piece, and an empty string has no pieces.
    Str(Vec<StrPiece>),
    /// A name that begins with a lowercase letter or `_`, with its `?` or
    /// `!` suffix. A keyword directly after a `.` is also an `Ident`.
    Ident(String),
    /// A name that begins with an uppercase letter.
    Const(String),
    Kw(Kw),
    LParen,
    RParen,
    LBracket,
    RBracket,
    LBrace,
    RBrace,
    Comma,
    Dot,
    Colon,
    ColonColon,
    Arrow,
    Pipe,
    Question,
    Assign,
    PlusAssign,
    MinusAssign,
    StarAssign,
    SlashAssign,
    PercentAssign,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Pow,
    /// `-@`, the name of the unary minus method.
    MinusAt,
    Bang,
    EqEq,
    NotEq,
    Lt,
    Le,
    Gt,
    Ge,
    AndAnd,
    OrOr,
    /// A line break that ends a statement. Its span is empty and sits at
    /// the end of the token before it.
    Newline,
    /// The `}` that closes an interpolation: the last token of a
    /// [`StrPiece::Interp`].
    InterpEnd,
    /// The last token of a file. Its span is empty and sits at the end of
    /// the token before it.
    Eof,
}

#[derive(Clone, Debug, PartialEq)]
pub enum StrPiece {
    Lit(String),
    /// The tokens between `#{` and `}`, ending in [`TokenKind::InterpEnd`].
    Interp(Vec<Token>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kw {
    Break,
    Case,
    Component,
    Def,
    Do,
    Each,
    Else,
    Elsif,
    Emit,
    End,
    Enum,
    Event,
    False,
    Gpu,
    If,
    Loop,
    Next,
    On,
    Print,
    Puts,
    Require,
    Resource,
    Return,
    Self_,
    Spawn,
    Struct,
    System,
    Tag,
    Then,
    True,
    Unless,
    Until,
    When,
    While,
}

impl Kw {
    pub fn from_name(name: &str) -> Option<Kw> {
        Some(match name {
            "break" => Kw::Break,
            "case" => Kw::Case,
            "component" => Kw::Component,
            "def" => Kw::Def,
            "do" => Kw::Do,
            "each" => Kw::Each,
            "else" => Kw::Else,
            "elsif" => Kw::Elsif,
            "emit" => Kw::Emit,
            "end" => Kw::End,
            "enum" => Kw::Enum,
            "event" => Kw::Event,
            "false" => Kw::False,
            "gpu" => Kw::Gpu,
            "if" => Kw::If,
            "loop" => Kw::Loop,
            "next" => Kw::Next,
            "on" => Kw::On,
            "print" => Kw::Print,
            "puts" => Kw::Puts,
            "require" => Kw::Require,
            "resource" => Kw::Resource,
            "return" => Kw::Return,
            "self" => Kw::Self_,
            "spawn" => Kw::Spawn,
            "struct" => Kw::Struct,
            "system" => Kw::System,
            "tag" => Kw::Tag,
            "then" => Kw::Then,
            "true" => Kw::True,
            "unless" => Kw::Unless,
            "until" => Kw::Until,
            "when" => Kw::When,
            "while" => Kw::While,
            _ => return None,
        })
    }
}

impl TokenKind {
    /// Whether a line break directly after this token continues the
    /// statement: binary operators, `,`, `=` and the compound assignments.
    fn continues_line(&self) -> bool {
        matches!(
            self,
            TokenKind::Comma
                | TokenKind::Assign
                | TokenKind::PlusAssign
                | TokenKind::MinusAssign
                | TokenKind::StarAssign
                | TokenKind::SlashAssign
                | TokenKind::PercentAssign
                | TokenKind::Plus
                | TokenKind::Minus
                | TokenKind::Star
                | TokenKind::Slash
                | TokenKind::Percent
                | TokenKind::Pow
                | TokenKind::EqEq
                | TokenKind::NotEq
                | TokenKind::Lt
                | TokenKind::Le
                | TokenKind::Gt
                | TokenKind::Ge
                | TokenKind::AndAnd
                | TokenKind::OrOr
        )
    }
}

/// Splits one file into tokens, ending in [`TokenKind::Eof`]. Reports the
/// first error.
pub fn lex(file: FileId, text: &str) -> Result<Vec<Token>, Diagnostic> {
    let mut lexer = Lexer {
        file,
        text,
        bytes: text.as_bytes(),
        pos: 0,
        unterminated: false,
    };
    lexer.tokens(None)
}

/// The error for an integer literal outside the range of `Int`.
pub fn int_too_large(span: Span) -> Diagnostic {
    Diagnostic::new("integer literal is too large", span).label(format!(
        "an `Int` holds {} to {}",
        i64::MIN,
        i64::MAX
    ))
}

struct Lexer<'a> {
    file: FileId,
    text: &'a str,
    bytes: &'a [u8],
    pos: usize,
    /// Set when a string with no closing quote has been reported.
    unterminated: bool,
}

fn is_word_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

impl Lexer<'_> {
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn peek_at(&self, ahead: usize) -> Option<u8> {
        self.bytes.get(self.pos + ahead).copied()
    }

    /// The character at the current position.
    fn char(&self) -> Option<char> {
        self.text[self.pos..].chars().next()
    }

    fn span(&self, start: usize, end: usize) -> Span {
        Span::new(self.file, start as u32, end as u32)
    }

    /// Consumes `len` bytes as the token `kind`.
    fn op(&mut self, len: usize, kind: TokenKind) -> TokenKind {
        self.pos += len;
        kind
    }

    /// Lexes up to the end of the file, or, inside an interpolation that
    /// opened at byte `interpolation`, up to its closing `}`.
    fn tokens(&mut self, interpolation: Option<usize>) -> Result<Vec<Token>, Diagnostic> {
        let mut out: Vec<Token> = Vec::new();
        // Open `(` and `[`, inside which line breaks are ignored.
        let mut nesting = 0u32;
        // Open `{` inside an interpolation.
        let mut braces = 0u32;
        let mut space = interpolation.is_none();
        loop {
            space |= self.skip_blanks();
            let start = self.pos;
            let byte = match (self.peek(), interpolation) {
                (None | Some(b'\n'), Some(open)) => {
                    return Err(self.unclosed_interpolation(open));
                }
                (None, None) => {
                    let at = out.last().map_or(0, |token| token.span.end);
                    out.push(Token {
                        kind: TokenKind::Eof,
                        span: Span::new(self.file, at, at),
                        space_before: space,
                    });
                    return Ok(out);
                }
                (Some(b'\n'), None) => {
                    self.pos += 1;
                    space = true;
                    if nesting == 0 && self.newline_ends_statement(&out) {
                        let at = out.last().map_or(0, |token| token.span.end);
                        out.push(Token {
                            kind: TokenKind::Newline,
                            span: Span::new(self.file, at, at),
                            space_before: false,
                        });
                    }
                    continue;
                }
                (Some(byte), _) => byte,
            };
            let kind = match byte {
                b'0'..=b'9' => self.number()?,
                b'"' | b'\'' => {
                    let string = if byte == b'"' {
                        self.double_quoted()
                    } else {
                        self.single_quoted()
                    };
                    match (string, interpolation) {
                        (Err(_), Some(open)) if self.unterminated => {
                            return Err(self.unclosed_interpolation(open));
                        }
                        (string, _) => string?,
                    }
                }
                b'a'..=b'z' | b'A'..=b'Z' | b'_' => {
                    let after_dot = out.last().is_some_and(|t| t.kind == TokenKind::Dot);
                    self.word(after_dot)
                }
                b'(' => {
                    nesting += 1;
                    self.op(1, TokenKind::LParen)
                }
                b')' => {
                    nesting = nesting.saturating_sub(1);
                    self.op(1, TokenKind::RParen)
                }
                b'[' => {
                    nesting += 1;
                    self.op(1, TokenKind::LBracket)
                }
                b']' => {
                    nesting = nesting.saturating_sub(1);
                    self.op(1, TokenKind::RBracket)
                }
                b'{' => {
                    braces += 1;
                    self.op(1, TokenKind::LBrace)
                }
                b'}' if interpolation.is_some() && braces == 0 => {
                    self.pos += 1;
                    out.push(Token {
                        kind: TokenKind::InterpEnd,
                        span: self.span(start, self.pos),
                        space_before: space,
                    });
                    return Ok(out);
                }
                b'}' => {
                    braces = braces.saturating_sub(1);
                    self.op(1, TokenKind::RBrace)
                }
                b',' => self.op(1, TokenKind::Comma),
                b'.' => self.op(1, TokenKind::Dot),
                b'?' => self.op(1, TokenKind::Question),
                b':' => match self.peek_at(1) {
                    Some(b':') => self.op(2, TokenKind::ColonColon),
                    _ => self.op(1, TokenKind::Colon),
                },
                b'-' => match self.peek_at(1) {
                    Some(b'>') => self.op(2, TokenKind::Arrow),
                    Some(b'=') => self.op(2, TokenKind::MinusAssign),
                    Some(b'@') => self.op(2, TokenKind::MinusAt),
                    _ => self.op(1, TokenKind::Minus),
                },
                b'+' => match self.peek_at(1) {
                    Some(b'=') => self.op(2, TokenKind::PlusAssign),
                    _ => self.op(1, TokenKind::Plus),
                },
                b'*' => match self.peek_at(1) {
                    Some(b'*') => self.op(2, TokenKind::Pow),
                    Some(b'=') => self.op(2, TokenKind::StarAssign),
                    _ => self.op(1, TokenKind::Star),
                },
                b'/' => match self.peek_at(1) {
                    Some(b'=') => self.op(2, TokenKind::SlashAssign),
                    _ => self.op(1, TokenKind::Slash),
                },
                b'%' => match self.peek_at(1) {
                    Some(b'=') => self.op(2, TokenKind::PercentAssign),
                    _ => self.op(1, TokenKind::Percent),
                },
                b'=' => match self.peek_at(1) {
                    Some(b'=') => self.op(2, TokenKind::EqEq),
                    _ => self.op(1, TokenKind::Assign),
                },
                b'!' => match self.peek_at(1) {
                    Some(b'=') => self.op(2, TokenKind::NotEq),
                    _ => self.op(1, TokenKind::Bang),
                },
                b'<' => match self.peek_at(1) {
                    Some(b'=') => self.op(2, TokenKind::Le),
                    _ => self.op(1, TokenKind::Lt),
                },
                b'>' => match self.peek_at(1) {
                    Some(b'=') => self.op(2, TokenKind::Ge),
                    _ => self.op(1, TokenKind::Gt),
                },
                b'|' => match self.peek_at(1) {
                    Some(b'|') => self.op(2, TokenKind::OrOr),
                    _ => self.op(1, TokenKind::Pipe),
                },
                b'&' if self.peek_at(1) == Some(b'&') => self.op(2, TokenKind::AndAnd),
                _ => return Err(self.unexpected_character()),
            };
            out.push(Token {
                kind,
                span: self.span(start, self.pos),
                space_before: space,
            });
            space = false;
        }
    }

    /// Skips spaces, tabs, carriage returns and comments. Returns whether
    /// there were any.
    fn skip_blanks(&mut self) -> bool {
        let start = self.pos;
        loop {
            match self.peek() {
                Some(b' ' | b'\t' | b'\r') => self.pos += 1,
                Some(b'#') => {
                    while !matches!(self.peek(), None | Some(b'\n')) {
                        self.pos += 1;
                    }
                }
                _ => return self.pos > start,
            }
        }
    }

    /// Whether the line break just consumed ends a statement, given the
    /// tokens before it and the text after it.
    fn newline_ends_statement(&self, before: &[Token]) -> bool {
        let Some(last) = before.last() else {
            return false;
        };
        if last.kind == TokenKind::Newline {
            return false;
        }
        // An operator directly after `def` is a method name.
        let names_method =
            before.len() >= 2 && before[before.len() - 2].kind == TokenKind::Kw(Kw::Def);
        if last.kind.continues_line() && !names_method {
            return false;
        }
        // A `.` at the start of the next line of code continues a method
        // chain.
        let mut at = self.pos;
        loop {
            match self.bytes.get(at) {
                Some(b' ' | b'\t' | b'\r' | b'\n') => at += 1,
                Some(b'#') => {
                    while !matches!(self.bytes.get(at), None | Some(b'\n')) {
                        at += 1;
                    }
                }
                Some(b'.') => return false,
                _ => return true,
            }
        }
    }

    /// Consumes digits of `radix` separated by single underscores. Returns
    /// false when there is no digit or an underscore is out of place.
    fn digits(&mut self, radix: u32) -> bool {
        let mut any = false;
        let mut after_underscore = false;
        loop {
            match self.peek() {
                Some(byte) if (byte as char).is_digit(radix) => {
                    self.pos += 1;
                    any = true;
                    after_underscore = false;
                }
                Some(b'_') if any && !after_underscore => {
                    self.pos += 1;
                    after_underscore = true;
                }
                _ => return any && !after_underscore,
            }
        }
    }

    fn number(&mut self) -> Result<TokenKind, Diagnostic> {
        let start = self.pos;
        let radix = match (self.bytes[start], self.peek_at(1)) {
            (b'0', Some(b'x')) => 16,
            (b'0', Some(b'b')) => 2,
            _ => 10,
        };
        let mut well_formed;
        let mut float = false;
        let mut padded = false;
        if radix == 10 {
            well_formed = self.digits(10);
            padded = self.bytes[start] == b'0' && self.pos - start > 1;
            // A `.` begins a fraction only when a digit follows: `1.abs`
            // is a method call on `1`.
            if self.peek() == Some(b'.') && self.peek_at(1).is_some_and(|b| b.is_ascii_digit()) {
                self.pos += 1;
                well_formed &= self.digits(10);
                float = true;
            }
            if matches!(self.peek(), Some(b'e' | b'E')) {
                let sign = usize::from(matches!(self.peek_at(1), Some(b'+' | b'-')));
                if self.peek_at(1 + sign).is_some_and(|b| b.is_ascii_digit()) {
                    self.pos += 1 + sign;
                    well_formed &= self.digits(10);
                    float = true;
                }
            }
        } else {
            self.pos += 2;
            well_formed = self.digits(radix);
        }
        // Letters, digits or `_` directly after the literal are part of
        // the mistake.
        if self.peek().is_some_and(is_word_byte) {
            while self.peek().is_some_and(is_word_byte) {
                self.pos += 1;
            }
            well_formed = false;
        }
        let span = self.span(start, self.pos);
        if !well_formed {
            return Err(Diagnostic::new("invalid number literal", span)
                .label("this is not a valid `Int` or `Float`"));
        }
        if padded {
            return Err(Diagnostic::new("invalid number literal", span)
                .label("a number can't begin with extra zeros"));
        }
        let digits: String = self.text[start..self.pos]
            .chars()
            .filter(|&c| c != '_')
            .collect();
        if float {
            return match digits.parse::<f64>() {
                Ok(value) if value.is_finite() => Ok(TokenKind::Float(value)),
                _ => Err(Diagnostic::new("float literal is too large", span)
                    .label("this is outside the range of `Float`")),
            };
        }
        let value = if radix == 10 {
            digits.parse::<u64>()
        } else {
            u64::from_str_radix(&digits[2..], radix)
        };
        match value {
            Ok(value) if value <= INT_LIMIT => Ok(TokenKind::Int(value)),
            _ => Err(int_too_large(span)),
        }
    }

    /// Lexes a name or keyword. After a `.` every name is an identifier.
    fn word(&mut self, after_dot: bool) -> TokenKind {
        let start = self.pos;
        while self.peek().is_some_and(is_word_byte) {
            self.pos += 1;
        }
        if self.bytes[start].is_ascii_uppercase() {
            return TokenKind::Const(self.text[start..self.pos].to_string());
        }
        // `a!=b` is `a != b`.
        let suffix = match self.peek() {
            Some(b'?') => true,
            Some(b'!') => self.peek_at(1) != Some(b'='),
            _ => false,
        };
        if suffix {
            self.pos += 1;
        }
        let name = &self.text[start..self.pos];
        match Kw::from_name(name) {
            Some(kw) if !after_dot => TokenKind::Kw(kw),
            _ => TokenKind::Ident(name.to_string()),
        }
    }

    fn double_quoted(&mut self) -> Result<TokenKind, Diagnostic> {
        let open = self.pos;
        self.pos += 1;
        let mut pieces = Vec::new();
        let mut text = String::new();
        loop {
            match self.char() {
                None | Some('\n') => return Err(self.unterminated_string(open)),
                Some('"') => {
                    self.pos += 1;
                    break;
                }
                Some('\\') => {
                    let start = self.pos;
                    self.pos += 1;
                    let escape = match self.char() {
                        None | Some('\n') => return Err(self.unterminated_string(open)),
                        Some(escape) => escape,
                    };
                    self.pos += escape.len_utf8();
                    text.push(match escape {
                        'n' => '\n',
                        't' => '\t',
                        'r' => '\r',
                        '0' => '\0',
                        'e' => '\x1b',
                        '\\' | '"' | '#' => escape,
                        _ => {
                            return Err(Diagnostic::new(
                                format!("unknown escape `\\{escape}`"),
                                self.span(start, self.pos),
                            )
                            .label(r#"the escapes are \n, \t, \r, \0, \e, \\, \" and \#"#));
                        }
                    });
                }
                Some('#') if self.peek_at(1) == Some(b'{') => {
                    if !text.is_empty() {
                        pieces.push(StrPiece::Lit(std::mem::take(&mut text)));
                    }
                    let start = self.pos;
                    self.pos += 2;
                    pieces.push(StrPiece::Interp(self.tokens(Some(start))?));
                }
                Some(c) => {
                    text.push(c);
                    self.pos += c.len_utf8();
                }
            }
        }
        if !text.is_empty() {
            pieces.push(StrPiece::Lit(text));
        }
        Ok(TokenKind::Str(pieces))
    }

    fn single_quoted(&mut self) -> Result<TokenKind, Diagnostic> {
        let open = self.pos;
        self.pos += 1;
        let mut text = String::new();
        loop {
            match self.char() {
                None | Some('\n') => return Err(self.unterminated_string(open)),
                Some('\'') => {
                    self.pos += 1;
                    break;
                }
                Some('\\') if matches!(self.peek_at(1), Some(b'\'' | b'\\')) => {
                    text.push(self.bytes[self.pos + 1] as char);
                    self.pos += 2;
                }
                Some(c) => {
                    text.push(c);
                    self.pos += c.len_utf8();
                }
            }
        }
        if text.is_empty() {
            Ok(TokenKind::Str(Vec::new()))
        } else {
            Ok(TokenKind::Str(vec![StrPiece::Lit(text)]))
        }
    }

    /// The error for a string whose line ends before its closing quote.
    /// `open` is the position of the opening quote.
    fn unterminated_string(&mut self, open: usize) -> Diagnostic {
        self.unterminated = true;
        let quote = self.bytes[open] as char;
        Diagnostic::new("unterminated string", self.span(open, open + 1))
            .label(format!("the closing `{quote}` is missing"))
    }

    /// The error for a `#{` at byte `open` with no `}` on its line.
    fn unclosed_interpolation(&self, open: usize) -> Diagnostic {
        Diagnostic::new("this `#{` is never closed", self.span(open, open + 2))
            .label("expected a matching `}`")
    }

    fn unexpected_character(&self) -> Diagnostic {
        let c = self.char().unwrap_or(char::REPLACEMENT_CHARACTER);
        let span = self.span(self.pos, self.pos + c.len_utf8());
        let shown = if c.is_control() {
            format!("U+{:04X}", c as u32)
        } else {
            format!("`{c}`")
        };
        let diagnostic = Diagnostic::new(format!("unexpected character {shown}"), span);
        match c {
            '&' => diagnostic.label("did you mean `&&`?"),
            ';' => diagnostic.label("put each statement on its own line"),
            _ => diagnostic,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::TokenKind::*;
    use super::*;

    const FILE: FileId = FileId(0);

    fn kinds(text: &str) -> Vec<TokenKind> {
        lex(FILE, text)
            .unwrap_or_else(|error| panic!("{text:?}: {}", error.message))
            .into_iter()
            .map(|token| token.kind)
            .collect()
    }

    fn error(text: &str) -> (String, u32, u32) {
        let diagnostic = lex(FILE, text).expect_err(text);
        (
            diagnostic.message,
            diagnostic.span.start,
            diagnostic.span.end,
        )
    }

    fn ident(name: &str) -> TokenKind {
        Ident(name.to_string())
    }

    fn constant(name: &str) -> TokenKind {
        Const(name.to_string())
    }

    fn lit(text: &str) -> TokenKind {
        Str(vec![StrPiece::Lit(text.to_string())])
    }

    #[test]
    fn integers() {
        assert_eq!(
            kinds("42 1_000_000 0xff 0b1010 0 0xFF_ff"),
            [
                Int(42),
                Int(1_000_000),
                Int(255),
                Int(10),
                Int(0),
                Int(0xffff),
                Eof
            ]
        );
        assert_eq!(
            kinds("9223372036854775807 9223372036854775808"),
            [Int(i64::MAX as u64), Int(INT_LIMIT), Eof]
        );
    }

    #[test]
    fn floats() {
        assert_eq!(
            kinds("3.25 1_000.5 1e-3 2.5e6 1E+2 0.5"),
            [
                Float(3.25),
                Float(1000.5),
                Float(1e-3),
                Float(2.5e6),
                Float(100.0),
                Float(0.5),
                Eof
            ]
        );
    }

    #[test]
    fn a_dot_without_a_digit_is_a_member_access() {
        assert_eq!(kinds("1.abs"), [Int(1), Dot, ident("abs"), Eof]);
        assert_eq!(kinds("3.times"), [Int(3), Dot, ident("times"), Eof]);
        assert_eq!(kinds("1.5.floor"), [Float(1.5), Dot, ident("floor"), Eof]);
        assert_eq!(kinds("2.e3"), [Int(2), Dot, ident("e3"), Eof]);
    }

    #[test]
    fn bad_numbers() {
        for text in [
            "12abc", "1__0", "1_", "0x", "0xfg", "0b102", "0b", "1e", "1.5e", "007", "0_1", "1.5x",
            "0X1F", "1_.5",
        ] {
            let (message, start, end) = error(text);
            assert_eq!(message, "invalid number literal", "{text}");
            assert_eq!((start, end), (0, text.len() as u32), "{text}");
        }
        assert_eq!(
            error("x = 9223372036854775809"),
            ("integer literal is too large".to_string(), 4, 23)
        );
        assert_eq!(
            error("0xffffffffffffffff").0,
            "integer literal is too large"
        );
        assert_eq!(error("1e999").0, "float literal is too large");
    }

    #[test]
    fn names_and_keywords() {
        assert_eq!(
            kinds("foo _bar Baz X1 end gpu self"),
            [
                ident("foo"),
                ident("_bar"),
                constant("Baz"),
                constant("X1"),
                Kw(super::Kw::End),
                Kw(super::Kw::Gpu),
                Kw(super::Kw::Self_),
                Eof
            ]
        );
    }

    #[test]
    fn every_keyword_is_reserved() {
        let words = "break case component def do each else elsif emit end enum event false gpu \
                     if loop next on print puts require resource return self spawn struct system \
                     tag then true unless until when while";
        let tokens = kinds(words);
        assert_eq!(tokens.len(), 35);
        for token in &tokens[..34] {
            assert!(matches!(token, Kw(_)), "{token:?}");
        }
    }

    #[test]
    fn question_and_bang_suffixes() {
        assert_eq!(
            kinds("alive? save! a!=b a != b"),
            [
                ident("alive?"),
                ident("save!"),
                ident("a"),
                NotEq,
                ident("b"),
                ident("a"),
                NotEq,
                ident("b"),
                Eof
            ]
        );
        assert_eq!(
            kinds("ok ? 1 : 2"),
            [ident("ok"), Question, Int(1), Colon, Int(2), Eof]
        );
        assert_eq!(
            kinds("e.alive? ? 1 : 2"),
            [
                ident("e"),
                Dot,
                ident("alive?"),
                Question,
                Int(1),
                Colon,
                Int(2),
                Eof
            ]
        );
        // A suffix makes a different name, not a keyword.
        assert_eq!(
            kinds("end? Foo?"),
            [ident("end?"), constant("Foo"), Question, Eof]
        );
        assert_eq!(kinds("!done"), [Bang, ident("done"), Eof]);
    }

    #[test]
    fn keywords_after_a_dot_are_names() {
        assert_eq!(
            kinds("xs.each e.spawn x.end"),
            [
                ident("xs"),
                Dot,
                ident("each"),
                ident("e"),
                Dot,
                ident("spawn"),
                ident("x"),
                Dot,
                ident("end"),
                Eof
            ]
        );
    }

    #[test]
    fn punctuation() {
        assert_eq!(
            kinds(
                "( ) [ ] , . : :: -> | ? = += -= *= /= %= + - * / % ** -@ ! == != < <= > >= && ||"
            ),
            [
                LParen,
                RParen,
                LBracket,
                RBracket,
                Comma,
                Dot,
                Colon,
                ColonColon,
                Arrow,
                Pipe,
                Question,
                Assign,
                PlusAssign,
                MinusAssign,
                StarAssign,
                SlashAssign,
                PercentAssign,
                Plus,
                Minus,
                Star,
                Slash,
                Percent,
                Pow,
                MinusAt,
                Bang,
                EqEq,
                NotEq,
                Lt,
                Le,
                Gt,
                Ge,
                AndAnd,
                OrOr,
                Eof
            ]
        );
        assert_eq!(
            kinds("Team::Red a**b |a, b|"),
            [
                constant("Team"),
                ColonColon,
                constant("Red"),
                ident("a"),
                Pow,
                ident("b"),
                Pipe,
                ident("a"),
                Comma,
                ident("b"),
                Pipe,
                Eof
            ]
        );
    }

    #[test]
    fn space_before() {
        let tokens = lex(FILE, "spawn(a) spawn (a)\n  b # c\nd").unwrap();
        let spaces: Vec<bool> = tokens.iter().map(|token| token.space_before).collect();
        assert_eq!(
            spaces,
            [
                true, false, false, false, // spawn ( a )
                true, true, false, false, // spawn ( a )
                false, true, // newline b
                false, true,  // newline d
                false  // end of file
            ]
        );
    }

    #[test]
    fn spans_are_byte_offsets() {
        let error = lex(FILE, "né = 'é' + x").unwrap_err();
        assert_eq!(error.message, "unexpected character `é`");
        assert_eq!((error.span.start, error.span.end), (1, 3));

        let tokens = lex(FILE, "a = 'é' + x").unwrap();
        let spans: Vec<(u32, u32)> = tokens.iter().map(|t| (t.span.start, t.span.end)).collect();
        assert_eq!(spans, [(0, 1), (2, 3), (4, 8), (9, 10), (11, 12), (12, 12)]);
    }

    #[test]
    fn newlines_end_statements() {
        assert_eq!(
            kinds("a\nb\n"),
            [ident("a"), Newline, ident("b"), Newline, Eof]
        );
        // Blank lines and comments produce nothing.
        assert_eq!(
            kinds("\n\n# top\na # one\n\n  # two\n\nb"),
            [ident("a"), Newline, ident("b"), Eof]
        );
        assert_eq!(kinds("a\r\nb"), [ident("a"), Newline, ident("b"), Eof]);
    }

    #[test]
    fn a_newline_token_sits_after_the_previous_token() {
        let tokens = lex(FILE, "ab  # note\ncd").unwrap();
        assert_eq!(tokens[1].kind, Newline);
        assert_eq!((tokens[1].span.start, tokens[1].span.end), (2, 2));
        assert_eq!((tokens[3].span.start, tokens[3].span.end), (13, 13));
    }

    #[test]
    fn newlines_inside_brackets_are_ignored() {
        assert_eq!(
            kinds("f(\n  a\n)\n[\n1\n]"),
            [
                ident("f"),
                LParen,
                ident("a"),
                RParen,
                Newline,
                LBracket,
                Int(1),
                RBracket,
                Eof
            ]
        );
    }

    #[test]
    fn newlines_after_operators_are_ignored() {
        for op in [
            "+", "-", "*", "/", "%", "**", "==", "!=", "<", "<=", ">", ">=", "&&", "||", ",", "=",
            "+=", "-=", "*=", "/=", "%=",
        ] {
            let tokens = kinds(&format!("a {op}\n  b"));
            assert_eq!(tokens.len(), 4, "{op}");
            assert!(!tokens.contains(&Newline), "{op}");
        }
        for other in ["!", "?", ":", "|", "->", "."] {
            let tokens = kinds(&format!("a {other}\n  b"));
            assert_eq!(tokens[2], Newline, "{other}");
        }
    }

    #[test]
    fn an_operator_method_name_does_not_continue_the_line() {
        assert_eq!(
            kinds("def -\nx"),
            [Kw(super::Kw::Def), Minus, Newline, ident("x"), Eof]
        );
        assert_eq!(
            kinds("def -@\nx"),
            [Kw(super::Kw::Def), MinusAt, Newline, ident("x"), Eof]
        );
    }

    #[test]
    fn a_leading_dot_continues_the_statement() {
        assert_eq!(
            kinds("a\n  .b\n\n  # note\n  .c\nd"),
            [
                ident("a"),
                Dot,
                ident("b"),
                Dot,
                ident("c"),
                Newline,
                ident("d"),
                Eof
            ]
        );
    }

    #[test]
    fn double_quoted_strings() {
        assert_eq!(kinds(r#""hello""#), [lit("hello"), Eof]);
        assert_eq!(kinds(r#""""#), [Str(vec![]), Eof]);
        assert_eq!(
            kinds(r#""a\n\t\r\0\e\\\"\#{x}""#),
            [lit("a\n\t\r\0\x1b\\\"#{x}"), Eof]
        );
        assert_eq!(
            kinds(r##""# not a comment""##),
            [lit("# not a comment"), Eof]
        );
        assert_eq!(kinds(r#""a { b } c""#), [lit("a { b } c"), Eof]);
    }

    #[test]
    fn single_quoted_strings() {
        assert_eq!(kinds(r"'raw'"), [lit("raw"), Eof]);
        assert_eq!(kinds(r"''"), [Str(vec![]), Eof]);
        assert_eq!(
            kinds(r##"'a\n #{x} \' \\ "q"'"##),
            [lit(r##"a\n #{x} ' \ "q""##), Eof]
        );
    }

    fn pieces(text: &str) -> Vec<StrPiece> {
        match lex(FILE, text).unwrap().remove(0).kind {
            Str(pieces) => pieces,
            other => panic!("not a string: {other:?}"),
        }
    }

    fn interp_kinds(piece: &StrPiece) -> Vec<TokenKind> {
        match piece {
            StrPiece::Interp(tokens) => tokens.iter().map(|token| token.kind.clone()).collect(),
            StrPiece::Lit(text) => panic!("not an interpolation: {text:?}"),
        }
    }

    #[test]
    fn interpolation() {
        let parts = pieces(r#""x = #{x + 1}!""#);
        assert_eq!(parts.len(), 3);
        assert_eq!(parts[0], StrPiece::Lit("x = ".to_string()));
        assert_eq!(
            interp_kinds(&parts[1]),
            [ident("x"), Plus, Int(1), InterpEnd]
        );
        assert_eq!(parts[2], StrPiece::Lit("!".to_string()));

        let parts = pieces(r##""#{a}#{b}""##);
        assert_eq!(parts.len(), 2);
    }

    #[test]
    fn interpolation_spans_point_into_the_file() {
        let StrPiece::Interp(tokens) = &pieces(r#""é#{pos.x}""#)[1] else {
            panic!("not an interpolation");
        };
        let spans: Vec<(u32, u32)> = tokens.iter().map(|t| (t.span.start, t.span.end)).collect();
        assert_eq!(spans, [(5, 8), (8, 9), (9, 10), (10, 11)]);
    }

    #[test]
    fn nested_interpolation() {
        let parts = pieces(r#""a#{ "b#{c}" }d""#);
        assert_eq!(parts.len(), 3);
        let StrPiece::Interp(tokens) = &parts[1] else {
            panic!("not an interpolation");
        };
        assert_eq!(tokens.len(), 2);
        let Str(inner) = &tokens[0].kind else {
            panic!("not a string");
        };
        assert_eq!(inner[0], StrPiece::Lit("b".to_string()));
        assert_eq!(interp_kinds(&inner[1]), [ident("c"), InterpEnd]);

        let parts = pieces(r##""#{h["}"]}""##);
        assert_eq!(
            interp_kinds(&parts[0]),
            [ident("h"), LBracket, lit("}"), RBracket, InterpEnd]
        );
    }

    #[test]
    fn braces_nest_inside_interpolation() {
        let parts = pieces(r##""#{ {a} }""##);
        assert_eq!(
            interp_kinds(&parts[0]),
            [LBrace, ident("a"), RBrace, InterpEnd]
        );
    }

    #[test]
    fn string_errors() {
        assert_eq!(
            error("x = \"abc\ny"),
            ("unterminated string".to_string(), 4, 5)
        );
        assert_eq!(error("'abc"), ("unterminated string".to_string(), 0, 1));
        assert_eq!(error("\"abc\\"), ("unterminated string".to_string(), 0, 1));
        assert_eq!(
            error(r#"puts "a\qb""#),
            ("unknown escape `\\q`".to_string(), 7, 9)
        );
        assert_eq!(
            error(r#""a #{x""#),
            ("this `#{` is never closed".to_string(), 3, 5)
        );
        assert_eq!(
            error("\"#{x\n}\""),
            ("this `#{` is never closed".to_string(), 1, 3)
        );
    }

    #[test]
    fn unexpected_characters() {
        assert_eq!(
            error("a & b"),
            ("unexpected character `&`".to_string(), 2, 3)
        );
        assert_eq!(
            error("a; b"),
            ("unexpected character `;`".to_string(), 1, 2)
        );
        assert_eq!(
            error("a\u{1}"),
            ("unexpected character U+0001".to_string(), 1, 2)
        );
    }

    #[test]
    fn end_of_file_sits_after_the_last_token() {
        let tokens = lex(FILE, "ab \n\n").unwrap();
        assert_eq!(tokens.last().unwrap().span, Span::new(FILE, 2, 2));
        assert_eq!(kinds(""), [Eof]);
        assert_eq!(kinds("# only a comment\n"), [Eof]);
    }
}
