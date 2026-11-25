//! Tokens to syntax tree.
//!
//! A recursive-descent parser that follows the grammar in
//! `docs/language.md`, one method per rule. It stops at the first syntax
//! error.

use crate::ast::{
    Arg, AssignOp, BinaryOp, Block, CaseArm, ConstDecl, DoBlock, EnumDecl, Expr, ExprKind,
    FieldDecl, FuncDecl, Ident, Item, Param, Query, RecordDecl, RecordKind, Require, Stmt,
    StmtKind, StrPart, SystemDecl, TagDecl, TypeExpr, TypeExprKind, UnaryOp,
};
use crate::diag::Diagnostic;
use crate::lexer::{self, Kw, StrPiece, Token, TokenKind as T};
use crate::source::{FileId, Span};

/// Parses one file into its items. Reports the first syntax error.
pub fn parse(file: FileId, text: &str) -> Result<Vec<Item>, Diagnostic> {
    let tokens = lexer::lex(file, text)?;
    Parser::new(file, text, &tokens).program()
}

type Parse<V> = Result<V, Diagnostic>;

// Binary operator levels, loosest first.
const OR: u8 = 0;
const AND: u8 = 1;
const EQUALITY: u8 = 2;
const COMPARE: u8 = 3;
const ADDITIVE: u8 = 4;
const MULTIPLY: u8 = 5;

/// A block whose `end` has not been reached yet.
#[derive(Clone, Copy)]
struct Open {
    /// The keyword that opened it.
    word: &'static str,
    span: Span,
}

struct Parser<'a> {
    file: FileId,
    text: &'a str,
    /// Ends in `Eof` or, for an interpolation, `InterpEnd`.
    tokens: &'a [Token],
    pos: usize,
    /// Where the last consumed token other than a newline ended.
    prev_end: u32,
    /// The blocks being parsed, outermost first.
    open: Vec<Open>,
    /// Blocks inside the outermost open one whose `end` was not lined up
    /// with them, in the order they were closed. When the end of the file
    /// arrives too early, the first of these is the likeliest block to be
    /// missing its `end`.
    misaligned: Vec<Open>,
    /// How many `(` and `[` of expressions are open.
    enclosed: u32,
    /// Whether the condition of an `if`, `unless`, `while` or `until` is
    /// being parsed.
    in_condition: bool,
}

/// The label of the error for a `do` block after something that is not a
/// loop.
const DO_BLOCK_TAKERS: &str =
    "only `times`, `upto`, `downto`, `each` and `each_with_index` take one";

fn is_assign_op(kind: &T) -> bool {
    matches!(
        kind,
        T::Assign
            | T::PlusAssign
            | T::MinusAssign
            | T::StarAssign
            | T::SlashAssign
            | T::PercentAssign
    )
}

fn is_declaration(kw: Kw) -> bool {
    matches!(
        kw,
        Kw::Struct
            | Kw::Component
            | Kw::Resource
            | Kw::Event
            | Kw::Enum
            | Kw::Tag
            | Kw::Def
            | Kw::System
            | Kw::Require
    )
}

impl<'a> Parser<'a> {
    fn new(file: FileId, text: &'a str, tokens: &'a [Token]) -> Parser<'a> {
        Parser {
            file,
            text,
            tokens,
            pos: 0,
            prev_end: tokens[0].span.start,
            open: Vec::new(),
            misaligned: Vec::new(),
            enclosed: 0,
            in_condition: false,
        }
    }

    // Tokens

    fn peek(&self) -> &'a Token {
        &self.tokens[self.pos]
    }

    fn peek_at(&self, ahead: usize) -> &'a Token {
        &self.tokens[(self.pos + ahead).min(self.tokens.len() - 1)]
    }

    fn at(&self, kind: T) -> bool {
        self.peek().kind == kind
    }

    fn at_kw(&self, kw: Kw) -> bool {
        self.peek().kind == T::Kw(kw)
    }

    /// Whether the next token closes a block or ends the input.
    fn at_closer(&self) -> bool {
        matches!(
            self.peek().kind,
            T::Kw(Kw::End | Kw::Else | Kw::Elsif | Kw::When) | T::Eof | T::InterpEnd
        )
    }

    /// Whether nothing more of a simple statement can follow: the next
    /// token ends the line, closes a block or begins a modifier.
    fn at_statement_end(&self) -> bool {
        self.at_closer() || matches!(self.peek().kind, T::Newline | T::Kw(Kw::If | Kw::Unless))
    }

    /// Consumes one token. The last token of the input is never passed.
    fn advance(&mut self) -> &'a Token {
        let token = &self.tokens[self.pos];
        if self.pos + 1 < self.tokens.len() {
            self.pos += 1;
        }
        if token.kind != T::Newline {
            self.prev_end = token.span.end;
        }
        token
    }

    fn eat(&mut self, kind: T) -> bool {
        let found = self.at(kind);
        if found {
            self.advance();
        }
        found
    }

    fn expect(&mut self, kind: T, label: &str) -> Parse<&'a Token> {
        if self.at(kind) {
            Ok(self.advance())
        } else {
            Err(self.unexpected(label))
        }
    }

    /// Consumes the bracket `kind` that closes the one at `open`.
    fn close(&mut self, open: Span, kind: T, label: &str) -> Parse<&'a Token> {
        if self.at(T::Eof) {
            let (left, right) = if kind == T::RParen {
                ('(', ')')
            } else {
                ('[', ']')
            };
            return Err(
                Diagnostic::new(format!("this `{left}` is never closed"), open)
                    .label(format!("expected a matching `{right}`")),
            );
        }
        self.expect(kind, label)
    }

    /// Opens a `(` or `[` of an expression, whose span it returns.
    fn open_bracket(&mut self) -> Span {
        self.enclosed += 1;
        self.advance().span
    }

    /// Consumes the bracket `kind` that closes the one at `open`, after an
    /// expression.
    fn close_bracket(&mut self, open: Span, kind: T, label: &str) -> Parse<&'a Token> {
        if is_assign_op(&self.peek().kind) {
            return Err(self.assignment_in_expression());
        }
        self.enclosed = self.enclosed.saturating_sub(1);
        self.close(open, kind, label)
    }

    fn skip_newlines(&mut self) {
        while self.at(T::Newline) {
            self.advance();
        }
    }

    fn source(&self, span: Span) -> &'a str {
        &self.text[span.start as usize..span.end as usize]
    }

    /// From the start of `start` to the end of the last consumed token.
    fn span_from(&self, start: Span) -> Span {
        Span::new(self.file, start.start, self.prev_end.max(start.end))
    }

    // Errors

    /// An error at the next token, with `label` saying what was expected.
    fn unexpected(&self, label: impl Into<String>) -> Diagnostic {
        let token = self.peek();
        let message = match token.kind {
            T::Newline => "unexpected end of line".to_string(),
            T::Eof => "unexpected end of file".to_string(),
            T::Str(_) => "unexpected string".to_string(),
            _ => format!("unexpected `{}`", self.source(token.span)),
        };
        Diagnostic::new(message, token.span).label(label)
    }

    /// An error at the next token, which comes where an expression has
    /// just ended.
    fn unexpected_after_expr(&self, label: impl Into<String>) -> Diagnostic {
        if is_assign_op(&self.peek().kind) {
            self.assignment_in_expression()
        } else {
            self.unexpected(label)
        }
    }

    /// The error for an assignment operator inside an expression.
    fn assignment_in_expression(&self) -> Diagnostic {
        let token = self.peek();
        let label = if self.in_condition && token.kind == T::Assign {
            "to compare, write `==`; to assign, use a line of its own"
        } else {
            "assign on a line of its own, then use the variable"
        };
        Diagnostic::new("assignment is a statement, not an expression", token.span).label(label)
    }

    /// The error for a keyword where a name is being introduced.
    fn keyword_as_name(&self) -> Diagnostic {
        let token = self.peek();
        let message = format!(
            "`{}` is a keyword and can't be used as a name",
            self.source(token.span)
        );
        let label = if token.kind == T::Kw(Kw::Gpu) {
            "`gpu` goes before `system`: `gpu system name`"
        } else {
            "choose a different name"
        };
        Diagnostic::new(message, token.span).label(label)
    }

    /// Whether a statement begins by assigning to a keyword or declaring
    /// a variable named by one.
    fn assigns_to_keyword(&self) -> bool {
        let next = &self.peek_at(1).kind;
        matches!(self.peek().kind, T::Kw(kw) if kw != Kw::Self_)
            && (is_assign_op(next) || *next == T::Colon)
    }

    /// The error for an argument that follows the function or method at
    /// `callee` without parentheses, if the next token begins one on the
    /// same line.
    fn bare_argument(&self, callee: Span) -> Option<Diagnostic> {
        let first = self.peek();
        let begins_argument = matches!(
            first.kind,
            T::Int(_)
                | T::Float(_)
                | T::Str(_)
                | T::Ident(_)
                | T::Const(_)
                | T::Kw(Kw::True | Kw::False | Kw::Self_)
        );
        let between = Span::new(self.file, callee.end, first.span.start);
        if self.enclosed > 0 || !begins_argument || self.source(between).contains('\n') {
            return None;
        }
        // The arguments run to the end of the statement.
        let mut depth = 0u32;
        let mut end = first.span.end;
        for token in &self.tokens[self.pos..] {
            match token.kind {
                T::Newline | T::Eof | T::InterpEnd => break,
                T::LParen | T::LBracket => depth += 1,
                T::RParen | T::RBracket if depth > 0 => depth -= 1,
                T::RParen | T::RBracket => break,
                T::Kw(
                    Kw::If
                    | Kw::Unless
                    | Kw::Do
                    | Kw::Then
                    | Kw::End
                    | Kw::Else
                    | Kw::Elsif
                    | Kw::When,
                ) if depth == 0 => break,
                _ => {}
            }
            end = token.span.end;
        }
        let callee = self.source(callee);
        let arguments = self.source(Span::new(self.file, first.span.start, end));
        let label = if callee.contains('\n') || arguments.contains('\n') {
            "put the arguments in parentheses".to_string()
        } else {
            format!("write `{callee}({arguments})`")
        };
        Some(Diagnostic::new("a call with arguments needs parentheses", first.span).label(label))
    }

    /// Rejects a `do` that follows a function call, a constructor or a
    /// plain name.
    fn check_no_do_block(&self, expr: &Expr) -> Parse<()> {
        if !self.at_kw(Kw::Do) {
            return Ok(());
        }
        let (name, span) = match &expr.kind {
            ExprKind::Call { name, .. } | ExprKind::Construct { name, .. } => {
                (name.name.as_str(), name.span)
            }
            ExprKind::Var(name) | ExprKind::Const(name) => (name.as_str(), expr.span),
            _ => return Ok(()),
        };
        Err(
            Diagnostic::new(format!("`{name}` does not take a `do` block"), span)
                .label(DO_BLOCK_TAKERS),
        )
    }

    fn unclosed(open: Open) -> Diagnostic {
        Diagnostic::new(format!("this `{}` is never closed", open.word), open.span)
            .label("expected a matching `end`")
    }

    /// The error for a declaration or `require` inside a block.
    fn misplaced_declaration(&self) -> Diagnostic {
        if let Some(&open) = self.misaligned.first() {
            return Self::unclosed(open);
        }
        let token = self.peek();
        let message = match token.kind {
            T::Kw(Kw::Def) => {
                "`def` is only allowed at the top level and in a struct or component".to_string()
            }
            _ => format!(
                "`{}` is only allowed at the top level",
                self.source(token.span)
            ),
        };
        let diagnostic = Diagnostic::new(message, token.span).label("this is inside a block");
        match self.open.last() {
            Some(open) => diagnostic.note(
                format!("the enclosing `{}` starts here", open.word),
                open.span,
            ),
            None => diagnostic,
        }
    }

    // Blocks

    /// Consumes the keyword that opens a block and returns its span.
    fn opened(&mut self, word: &'static str) -> Span {
        let span = self.advance().span;
        self.open.push(Open { word, span });
        span
    }

    /// Consumes the `end` of the innermost open block and returns its span.
    fn end(&mut self) -> Parse<Span> {
        let Some(open) = self.open.pop() else {
            return Err(self.unexpected("expected `end`"));
        };
        match self.peek().kind {
            T::Kw(Kw::End) => {
                let end = self.advance().span;
                if self.open.is_empty() {
                    self.misaligned.clear();
                } else if self.aligned(open.span, end) {
                    self.misaligned
                        .retain(|inner| inner.span.start < open.span.start);
                } else {
                    self.misaligned.push(open);
                }
                Ok(end)
            }
            T::Eof | T::InterpEnd => Err(Self::unclosed(*self.misaligned.first().unwrap_or(&open))),
            _ => Err(self
                .unexpected("expected `end`")
                .note(format!("to close this `{}`", open.word), open.span)),
        }
    }

    /// Whether an `end` lines up with the keyword that opened its block: it
    /// is on the same line, has the indentation of the opener's line, is in
    /// the opener's column, or is not the first thing on its line.
    fn aligned(&self, open: Span, end: Span) -> bool {
        let (open_line, open_indent) = self.line_indent(open.start);
        let (end_line, end_indent) = self.line_indent(end.start);
        let end_column = end.start as usize - end_line;
        open_line == end_line
            || end_indent.len() != end_column
            || end_indent == open_indent
            || end_column == open.start as usize - open_line
    }

    /// The offset at which the line holding `offset` starts, and that
    /// line's leading whitespace.
    fn line_indent(&self, offset: u32) -> (usize, &'a str) {
        let start = self.text[..offset as usize]
            .rfind('\n')
            .map_or(0, |newline| newline + 1);
        let line = &self.text[start..];
        let width = line.len() - line.trim_start_matches([' ', '\t']).len();
        (start, &line[..width])
    }

    /// The end of a block's header: a newline, or the block closing on the
    /// same line.
    fn header_end(&mut self, label: &str) -> Parse<()> {
        if self.eat(T::Newline) || self.at_closer() {
            Ok(())
        } else {
            Err(self.unexpected(label))
        }
    }

    /// `then`, or the end of the line.
    fn then(&mut self) -> Parse<()> {
        if self.eat(T::Kw(Kw::Then)) {
            Ok(())
        } else if is_assign_op(&self.peek().kind) {
            Err(self.assignment_in_expression())
        } else {
            self.header_end("expected `then` or a newline")
        }
    }

    /// The condition of an `if`, `unless`, `while` or `until`.
    fn condition(&mut self) -> Parse<Expr> {
        let outer = std::mem::replace(&mut self.in_condition, true);
        let cond = self.expr()?;
        if is_assign_op(&self.peek().kind) {
            return Err(self.assignment_in_expression());
        }
        self.in_condition = outer;
        Ok(cond)
    }

    /// Statements up to a token that closes a block.
    fn block(&mut self) -> Parse<Block> {
        let at = self.prev_end;
        let in_condition = std::mem::replace(&mut self.in_condition, false);
        let enclosed = std::mem::replace(&mut self.enclosed, 0);
        let mut stmts = Vec::new();
        loop {
            self.skip_newlines();
            if self.assigns_to_keyword() {
                return Err(self.keyword_as_name());
            }
            if self.at_closer() {
                break;
            }
            stmts.push(self.statement()?);
            if !self.at(T::Newline) && !self.at_closer() {
                return Err(self.unexpected_after_expr("expected a newline"));
            }
        }
        self.in_condition = in_condition;
        self.enclosed = enclosed;
        let span = match (stmts.first(), stmts.last()) {
            (Some(first), Some(last)) => first.span.to(last.span),
            _ => Span::new(self.file, at, at),
        };
        Ok(Block { stmts, span })
    }

    /// `do |params| … end`
    fn do_block(&mut self) -> Parse<DoBlock> {
        if !self.at_kw(Kw::Do) {
            return Err(self.unexpected("expected `do`"));
        }
        let keyword = self.opened("do");
        let mut params = Vec::new();
        if self.eat(T::Pipe) {
            loop {
                params.push(self.ident("a block parameter")?);
                if !self.eat(T::Comma) {
                    break;
                }
            }
            self.expect(T::Pipe, "expected `,` or `|`")?;
        }
        let body = self.block()?;
        let end = self.end()?;
        Ok(DoBlock {
            params,
            body,
            span: keyword.to(end),
        })
    }

    // Names and types

    /// A lowercase name. `what` describes it: "a field name".
    fn ident(&mut self, what: &str) -> Parse<Ident> {
        let token = self.peek();
        match &token.kind {
            T::Ident(name) => {
                self.advance();
                Ok(Ident {
                    name: name.clone(),
                    span: token.span,
                })
            }
            T::Const(_) => Err(self.unexpected(format!(
                "expected {what}, which begins with a lowercase letter"
            ))),
            T::Kw(_) => Err(self.keyword_as_name()),
            _ => Err(self.unexpected(format!("expected {what}"))),
        }
    }

    /// A capitalized name. `what` describes it: "a type name".
    fn type_name(&mut self, what: &str) -> Parse<Ident> {
        let token = self.peek();
        match &token.kind {
            T::Const(name) => {
                self.advance();
                Ok(Ident {
                    name: name.clone(),
                    span: token.span,
                })
            }
            T::Ident(_) => Err(self.unexpected(format!(
                "expected {what}, which begins with an uppercase letter"
            ))),
            _ => Err(self.unexpected(format!("expected {what}"))),
        }
    }

    /// `Name` or `[type]`
    fn type_expr(&mut self) -> Parse<TypeExpr> {
        let token = self.peek();
        match &token.kind {
            T::Const(name) => {
                self.advance();
                Ok(TypeExpr {
                    kind: TypeExprKind::Name(name.clone()),
                    span: token.span,
                })
            }
            T::LBracket => {
                self.advance();
                let element = self.type_expr()?;
                self.close(token.span, T::RBracket, "expected `]`")?;
                Ok(TypeExpr {
                    kind: TypeExprKind::List(Box::new(element)),
                    span: self.span_from(token.span),
                })
            }
            T::Ident(_) => {
                Err(self.unexpected("expected a type, which begins with an uppercase letter"))
            }
            _ => Err(self.unexpected("expected a type")),
        }
    }

    // Items

    fn program(mut self) -> Parse<Vec<Item>> {
        let mut items = Vec::new();
        loop {
            self.skip_newlines();
            if self.assigns_to_keyword() {
                return Err(self.keyword_as_name());
            }
            match self.peek().kind {
                T::Eof | T::InterpEnd => return Ok(items),
                T::Kw(Kw::End) => {
                    return Err(self.unexpected("there is no open block to close"));
                }
                T::Kw(Kw::Else | Kw::Elsif) => {
                    return Err(self.unexpected("there is no `if` this belongs to"));
                }
                T::Kw(Kw::When) => {
                    return Err(self.unexpected("there is no `case` this belongs to"));
                }
                _ => {}
            }
            let item = self.item()?;
            if !self.at(T::Newline) && !self.at_closer() {
                return Err(match item {
                    Item::Stmt(_) | Item::Const(_) => {
                        self.unexpected_after_expr("expected a newline")
                    }
                    _ => self.unexpected("expected a newline"),
                });
            }
            items.push(item);
        }
    }

    fn item(&mut self) -> Parse<Item> {
        let token = self.peek();
        match (&token.kind, &self.peek_at(1).kind) {
            (T::Kw(Kw::Require), _) => self.require().map(Item::Require),
            (T::Kw(Kw::Struct), _) => self.record(RecordKind::Struct).map(Item::Record),
            (T::Kw(Kw::Component), _) => self.record(RecordKind::Component).map(Item::Record),
            (T::Kw(Kw::Resource), _) => self.record(RecordKind::Resource).map(Item::Record),
            (T::Kw(Kw::Event), _) => self.record(RecordKind::Event).map(Item::Record),
            (T::Kw(Kw::Enum), _) => self.enumeration().map(Item::Enum),
            (T::Kw(Kw::Tag), _) => self.tag().map(Item::Tag),
            (T::Kw(Kw::Def), _) => self.def().map(Item::Def),
            (T::Kw(Kw::System), _) => self.system(false).map(Item::System),
            (T::Kw(Kw::Gpu), T::Kw(Kw::System)) => {
                self.advance();
                self.system(true).map(Item::System)
            }
            (T::Const(name), T::Assign) => {
                let name = Ident {
                    name: name.clone(),
                    span: token.span,
                };
                self.advance();
                self.advance();
                let value = self.command_expr()?;
                self.check_no_do_block(&value)?;
                Ok(Item::Const(ConstDecl { name, value }))
            }
            _ => self.statement().map(Item::Stmt),
        }
    }

    /// `require "path"`
    fn require(&mut self) -> Parse<Require> {
        let keyword = self.advance().span;
        let token = self.peek();
        let T::Str(pieces) = &token.kind else {
            return Err(self.unexpected("expected a file name in quotes"));
        };
        let path = match pieces.as_slice() {
            [] => String::new(),
            [StrPiece::Lit(path)] => path.clone(),
            _ => {
                return Err(
                    Diagnostic::new("`require` takes a plain string", token.span)
                        .label("interpolation is not allowed here"),
                );
            }
        };
        self.advance();
        Ok(Require {
            path,
            span: self.span_from(keyword),
        })
    }

    /// `struct`, `component`, `resource` or `event`, through its `end`.
    fn record(&mut self, kind: RecordKind) -> Parse<RecordDecl> {
        let (word, methods_allowed) = match kind {
            RecordKind::Struct => ("struct", true),
            RecordKind::Component => ("component", true),
            RecordKind::Resource => ("resource", false),
            RecordKind::Event => ("event", false),
        };
        let keyword = self.opened(word);
        let name = self.type_name("a type name")?;
        let span = keyword.to(name.span);
        self.header_end("expected a newline")?;
        let mut fields = Vec::new();
        let mut methods = Vec::new();
        loop {
            self.skip_newlines();
            let token = self.peek();
            match (&token.kind, &self.peek_at(1).kind) {
                (T::Kw(_), T::Colon) => return Err(self.keyword_as_name()),
                (T::Kw(Kw::End) | T::Eof | T::InterpEnd, _) => break,
                (T::Kw(Kw::Def), _) if methods_allowed => methods.push(self.def()?),
                (T::Kw(Kw::Def), _) => {
                    let owner = if kind == RecordKind::Resource {
                        "a resource"
                    } else {
                        "an event"
                    };
                    return Err(
                        Diagnostic::new(format!("{owner} can't have methods"), token.span)
                            .label("only structs and components have methods"),
                    );
                }
                (T::Ident(_), _) => fields.push(self.field()?),
                (T::Const(_), T::Colon) => {
                    return Err(self.unexpected("field names begin with a lowercase letter"));
                }
                (T::Kw(kw), _) if is_declaration(*kw) => {
                    return Err(self.misplaced_declaration());
                }
                _ if methods_allowed => {
                    return Err(self.unexpected("expected a field, `def` or `end`"));
                }
                _ => return Err(self.unexpected("expected a field or `end`")),
            }
            if !self.at(T::Newline) && !self.at_closer() {
                return Err(self.unexpected("expected a newline"));
            }
        }
        self.end()?;
        Ok(RecordDecl {
            kind,
            name,
            fields,
            methods,
            span,
        })
    }

    /// `name: Type` or `name: Type = default`
    fn field(&mut self) -> Parse<FieldDecl> {
        let name = self.ident("a field name")?;
        self.expect(T::Colon, "expected `:` and the field's type")?;
        let ty = self.type_expr()?;
        let default = if self.eat(T::Assign) {
            Some(self.expr()?)
        } else {
            None
        };
        Ok(FieldDecl { name, ty, default })
    }

    fn enumeration(&mut self) -> Parse<EnumDecl> {
        let keyword = self.opened("enum");
        let name = self.type_name("a type name")?;
        let span = keyword.to(name.span);
        self.header_end("expected a newline")?;
        let mut variants = Vec::new();
        loop {
            self.skip_newlines();
            if matches!(self.peek().kind, T::Kw(Kw::End) | T::Eof | T::InterpEnd) {
                break;
            }
            if let T::Kw(kw) = self.peek().kind
                && is_declaration(kw)
            {
                return Err(self.misplaced_declaration());
            }
            loop {
                variants.push(self.type_name("a variant name")?);
                if !self.eat(T::Comma) {
                    break;
                }
            }
            if !self.at(T::Newline) && !self.at_closer() {
                return Err(self.unexpected("expected `,` or a newline"));
            }
        }
        self.end()?;
        Ok(EnumDecl {
            name,
            variants,
            span,
        })
    }

    fn tag(&mut self) -> Parse<TagDecl> {
        let keyword = self.advance().span;
        let name = self.type_name("a type name")?;
        Ok(TagDecl {
            span: keyword.to(name.span),
            name,
        })
    }

    /// `def name(params) -> Type`, through its `end`.
    fn def(&mut self) -> Parse<FuncDecl> {
        let keyword = self.opened("def");
        let token = self.peek();
        let name = match &token.kind {
            T::Ident(name) => name.as_str(),
            T::Plus => "+",
            T::Minus => "-",
            T::Star => "*",
            T::Slash => "/",
            T::Percent => "%",
            T::MinusAt => "-@",
            T::Const(_) => {
                return Err(self.unexpected("function names begin with a lowercase letter"));
            }
            T::Kw(_) => return Err(self.keyword_as_name()),
            T::EqEq | T::NotEq => {
                return Err(Diagnostic::new(
                    format!(
                        "`{}` is provided for every type and can't be redefined",
                        self.source(token.span)
                    ),
                    token.span,
                ));
            }
            T::Lt | T::Le | T::Gt | T::Ge | T::Pow | T::Bang | T::AndAnd | T::OrOr => {
                return Err(self.unexpected("a method can define `+`, `-`, `*`, `/`, `%` and `-@`"));
            }
            _ => return Err(self.unexpected("expected a function name")),
        };
        let name = Ident {
            name: name.to_string(),
            span: token.span,
        };
        self.advance();
        let mut params = Vec::new();
        if self.at(T::LParen) {
            let open = self.advance().span;
            if !self.at(T::RParen) {
                loop {
                    let name = self.ident("a parameter name")?;
                    self.expect(T::Colon, "expected `:` and the parameter's type")?;
                    let ty = self.type_expr()?;
                    params.push(Param { name, ty });
                    if !self.eat(T::Comma) {
                        break;
                    }
                }
            }
            self.close(open, T::RParen, "expected `,` or `)`")?;
        }
        let ret = if self.eat(T::Arrow) {
            Some(self.type_expr()?)
        } else {
            None
        };
        let span = self.span_from(keyword);
        self.header_end(if ret.is_some() {
            "expected a newline"
        } else {
            "expected `->` or a newline"
        })?;
        let body = self.block()?;
        self.end()?;
        Ok(FuncDecl {
            name,
            params,
            ret,
            body,
            span,
        })
    }

    fn system(&mut self, gpu: bool) -> Parse<SystemDecl> {
        let keyword = self.opened("system");
        let name = self.ident("a system name")?;
        let span = keyword.to(name.span);
        if self.at(T::LParen) {
            return Err(
                Diagnostic::new("a system takes no parameters", self.peek().span)
                    .label("systems read what they need from resources and queries"),
            );
        }
        self.header_end("expected a newline")?;
        let body = self.block()?;
        self.end()?;
        Ok(SystemDecl {
            gpu,
            name,
            body,
            span,
        })
    }

    // Statements

    fn statement(&mut self) -> Parse<Stmt> {
        let start = self.peek().span;
        let kind = match self.peek().kind {
            T::Kw(Kw::While) => self.while_loop("while", false)?,
            T::Kw(Kw::Until) => self.while_loop("until", true)?,
            T::Kw(Kw::Loop) => self.loop_do()?,
            T::Kw(Kw::Each) => self.each()?,
            T::Kw(Kw::On) => self.on()?,
            T::Kw(kw) if is_declaration(kw) => return Err(self.misplaced_declaration()),
            _ => return self.simple(),
        };
        Ok(Stmt {
            kind,
            span: self.span_from(start),
        })
    }

    /// A simple statement with its `if` or `unless` modifier, if any.
    fn simple(&mut self) -> Parse<Stmt> {
        let start = self.peek().span;
        let kind = self.simple_kind()?;
        let stmt = Stmt {
            kind,
            span: self.span_from(start),
        };
        let negate = match self.peek().kind {
            T::Kw(Kw::If) => false,
            T::Kw(Kw::Unless) => true,
            _ => return Ok(stmt),
        };
        self.advance();
        let cond = Box::new(self.condition()?);
        let span = self.span_from(start);
        let then = Block {
            span: stmt.span,
            stmts: vec![stmt],
        };
        let kind = ExprKind::If {
            cond,
            negate,
            then,
            else_: None,
        };
        Ok(Stmt {
            kind: StmtKind::Expr(Expr { kind, span }),
            span,
        })
    }

    fn simple_kind(&mut self) -> Parse<StmtKind> {
        let token = self.peek();
        match (&token.kind, &self.peek_at(1).kind) {
            (T::Ident(name), T::Colon) => {
                let name = Ident {
                    name: name.clone(),
                    span: token.span,
                };
                self.advance();
                self.advance();
                let ty = self.type_expr()?;
                self.expect(T::Assign, "expected `=` and a value")?;
                let value = self.command_expr()?;
                self.check_no_do_block(&value)?;
                Ok(StmtKind::VarDecl { name, ty, value })
            }
            (T::Kw(Kw::Return), _) => {
                self.advance();
                if self.at_statement_end() {
                    Ok(StmtKind::Return(None))
                } else {
                    Ok(StmtKind::Return(Some(self.expr()?)))
                }
            }
            (T::Kw(Kw::Break), _) => {
                self.advance();
                Ok(StmtKind::Break)
            }
            (T::Kw(Kw::Next), _) => {
                self.advance();
                Ok(StmtKind::Next)
            }
            (T::Kw(Kw::Puts), _) => {
                self.advance();
                Ok(StmtKind::Puts(self.command_args()?))
            }
            (T::Kw(Kw::Print), _) => {
                self.advance();
                let args = self.command_args()?;
                if args.is_empty() {
                    return Err(
                        Diagnostic::new("`print` needs at least one argument", token.span)
                            .label("`puts` alone writes an empty line"),
                    );
                }
                Ok(StmtKind::Print(args))
            }
            (T::Kw(Kw::Emit), _) => {
                self.advance();
                let mut args = self.command_args()?;
                if args.len() != 1 {
                    let label = if args.is_empty() {
                        "expected an event after `emit`"
                    } else {
                        "write one `emit` for each event"
                    };
                    let span = self.span_from(token.span);
                    return Err(
                        Diagnostic::new("`emit` takes exactly one event", span).label(label)
                    );
                }
                Ok(StmtKind::Emit(args.remove(0)))
            }
            _ => {
                let target = self.command_expr()?;
                self.check_no_do_block(&target)?;
                let Some(op) = self.assign_op() else {
                    return Ok(StmtKind::Expr(target));
                };
                Self::check_place(&target)?;
                self.advance();
                let value = self.command_expr()?;
                self.check_no_do_block(&value)?;
                Ok(StmtKind::Assign { target, op, value })
            }
        }
    }

    fn assign_op(&self) -> Option<AssignOp> {
        Some(match self.peek().kind {
            T::Assign => AssignOp::Set,
            T::PlusAssign => AssignOp::Add,
            T::MinusAssign => AssignOp::Sub,
            T::StarAssign => AssignOp::Mul,
            T::SlashAssign => AssignOp::Div,
            T::PercentAssign => AssignOp::Mod,
            _ => return None,
        })
    }

    /// Rejects an assignment target that can't be a place whatever the
    /// names in it mean.
    fn check_place(target: &Expr) -> Parse<()> {
        let label = match &target.kind {
            ExprKind::Var(_)
            | ExprKind::Const(_)
            | ExprKind::SelfRef
            | ExprKind::Index { .. }
            | ExprKind::Member {
                args: None,
                block: None,
                ..
            } => return Ok(()),
            ExprKind::Member { .. }
            | ExprKind::Call { .. }
            | ExprKind::Construct { .. }
            | ExprKind::Spawn(_) => "the result of a call is a value, not a place",
            ExprKind::Int(_)
            | ExprKind::Float(_)
            | ExprKind::Bool(_)
            | ExprKind::Str(_)
            | ExprKind::List(_) => "a literal is a value, not a place",
            _ => "only a variable, a field, a list element or a component can be assigned to",
        };
        Err(Diagnostic::new("can't assign to this expression", target.span).label(label))
    }

    /// The arguments of `puts`, `print` or `emit`: a parenthesized list when
    /// a `(` directly follows the keyword, otherwise the expressions up to
    /// the end of the statement.
    fn command_args(&mut self) -> Parse<Vec<Expr>> {
        let next = self.peek();
        if next.kind == T::LParen && !next.space_before {
            self.advance();
            return self.expr_list(next.span, T::RParen, "expected `,` or `)`");
        }
        let mut args = Vec::new();
        if self.at_statement_end() {
            return Ok(args);
        }
        loop {
            args.push(self.expr()?);
            if !self.eat(T::Comma) {
                return Ok(args);
            }
        }
    }

    /// An expression in a position where `spawn` may take its arguments
    /// without parentheses: at the start of a statement, or as the whole
    /// right-hand side of an assignment.
    fn command_expr(&mut self) -> Parse<Expr> {
        if !self.at_kw(Kw::Spawn) || !Self::starts_spawn_args(self.peek_at(1)) {
            return self.expr();
        }
        let keyword = self.advance().span;
        let mut args = Vec::new();
        loop {
            args.push(self.expr()?);
            if !self.eat(T::Comma) {
                break;
            }
        }
        Ok(Expr {
            kind: ExprKind::Spawn(args),
            span: self.span_from(keyword),
        })
    }

    /// Whether `token`, coming after `spawn`, begins an argument written
    /// without parentheses.
    fn starts_spawn_args(token: &Token) -> bool {
        match token.kind {
            T::Int(_)
            | T::Float(_)
            | T::Str(_)
            | T::Ident(_)
            | T::Const(_)
            | T::Kw(Kw::True | Kw::False | Kw::Self_ | Kw::Spawn) => true,
            T::LParen | T::LBracket => token.space_before,
            _ => false,
        }
    }

    /// `while cond … end` or `until cond … end`
    fn while_loop(&mut self, word: &'static str, negate: bool) -> Parse<StmtKind> {
        self.opened(word);
        let cond = self.condition()?;
        self.header_end("expected a newline")?;
        let body = self.block()?;
        self.end()?;
        Ok(StmtKind::While { cond, negate, body })
    }

    /// `loop do … end`
    fn loop_do(&mut self) -> Parse<StmtKind> {
        self.advance();
        let block = self.do_block()?;
        if let (Some(first), Some(last)) = (block.params.first(), block.params.last()) {
            return Err(Diagnostic::new(
                "`loop` takes no block parameters",
                first.span.to(last.span),
            )
            .label("remove these"));
        }
        Ok(StmtKind::Loop { body: block.body })
    }

    /// `each query do |params| … end`
    fn each(&mut self) -> Parse<StmtKind> {
        self.advance();
        let query = self.query()?;
        let block = self.do_block()?;
        Ok(StmtKind::Each {
            query,
            params: block.params,
            body: block.body,
        })
    }

    /// `Term, Term, with: Type, without: [Type, Type]`
    fn query(&mut self) -> Parse<Query> {
        let start = self.peek().span;
        let mut terms = Vec::new();
        let mut with = Vec::new();
        let mut without = Vec::new();
        let mut filtered = false;
        loop {
            let token = self.peek();
            match (&token.kind, &self.peek_at(1).kind) {
                (T::Ident(name), T::Colon) => {
                    let list = match name.as_str() {
                        "with" => &mut with,
                        "without" => &mut without,
                        _ => {
                            return Err(Diagnostic::new(
                                format!("unknown query filter `{name}`"),
                                token.span,
                            )
                            .label("expected `with:` or `without:`"));
                        }
                    };
                    filtered = true;
                    self.advance();
                    self.advance();
                    if self.at(T::LBracket) {
                        let open = self.advance().span;
                        loop {
                            list.push(self.type_name("a component or tag")?);
                            if !self.eat(T::Comma) {
                                break;
                            }
                        }
                        self.close(open, T::RBracket, "expected `,` or `]`")?;
                    } else {
                        list.push(self.type_name("a component, a tag or `[`")?);
                    }
                }
                (T::Const(_), _) if filtered => {
                    return Err(
                        Diagnostic::new("query terms come before the filters", token.span)
                            .label("move this before `with:` and `without:`"),
                    );
                }
                _ if filtered => {
                    return Err(self.unexpected("expected `with:` or `without:`"));
                }
                _ => terms.push(self.type_name("a component or `Entity`")?),
            }
            if !self.eat(T::Comma) {
                break;
            }
        }
        let span = self.span_from(start);
        if terms.is_empty() {
            return Err(Diagnostic::new("a query needs at least one term", span)
                .label("name a component or `Entity` before the filters"));
        }
        Ok(Query {
            terms,
            with,
            without,
            span,
        })
    }

    /// `on Event do |param| … end`
    fn on(&mut self) -> Parse<StmtKind> {
        self.advance();
        let event = self.type_name("an event type")?;
        let block = self.do_block()?;
        Ok(StmtKind::On {
            event,
            params: block.params,
            body: block.body,
        })
    }

    // Expressions

    fn expr(&mut self) -> Parse<Expr> {
        let cond = self.binary(OR)?;
        if !self.eat(T::Question) {
            return Ok(cond);
        }
        let then = self.expr()?;
        if !self.eat(T::Colon) {
            return Err(self.unexpected_after_expr("expected `:` and the other branch"));
        }
        let else_ = self.expr()?;
        let span = cond.span.to(else_.span);
        Ok(Expr {
            kind: ExprKind::Ternary {
                cond: Box::new(cond),
                then: Box::new(then),
                else_: Box::new(else_),
            },
            span,
        })
    }

    /// The binary operator at the next token, if it belongs to `level`.
    fn binary_op(&self, level: u8) -> Option<BinaryOp> {
        Some(match (level, &self.peek().kind) {
            (OR, T::OrOr) => BinaryOp::Or,
            (AND, T::AndAnd) => BinaryOp::And,
            (EQUALITY, T::EqEq) => BinaryOp::Eq,
            (EQUALITY, T::NotEq) => BinaryOp::Ne,
            (COMPARE, T::Lt) => BinaryOp::Lt,
            (COMPARE, T::Le) => BinaryOp::Le,
            (COMPARE, T::Gt) => BinaryOp::Gt,
            (COMPARE, T::Ge) => BinaryOp::Ge,
            (ADDITIVE, T::Plus) => BinaryOp::Add,
            (ADDITIVE, T::Minus) => BinaryOp::Sub,
            (MULTIPLY, T::Star) => BinaryOp::Mul,
            (MULTIPLY, T::Slash) => BinaryOp::Div,
            (MULTIPLY, T::Percent) => BinaryOp::Mod,
            _ => return None,
        })
    }

    /// The operators of `level` and every tighter level. They associate to
    /// the left, and comparisons do not chain.
    fn binary(&mut self, level: u8) -> Parse<Expr> {
        if level > MULTIPLY {
            return self.unary();
        }
        let mut lhs = self.binary(level + 1)?;
        while let Some(op) = self.binary_op(level) {
            self.advance();
            let rhs = self.binary(level + 1)?;
            let span = lhs.span.to(rhs.span);
            lhs = Expr {
                kind: ExprKind::Binary {
                    op,
                    lhs: Box::new(lhs),
                    rhs: Box::new(rhs),
                },
                span,
            };
            if level == COMPARE {
                if self.binary_op(COMPARE).is_some() {
                    return Err(
                        Diagnostic::new("comparisons can't be chained", self.peek().span)
                            .label("join two comparisons with `&&`"),
                    );
                }
                break;
            }
        }
        Ok(lhs)
    }

    /// `-expr`, or a negative number literal.
    fn unary(&mut self) -> Parse<Expr> {
        if !self.at(T::Minus) {
            return self.power();
        }
        let minus = self.advance().span;
        let number = self.peek();
        // The literal with the `-` folded in, and without it.
        let folded = match number.kind {
            _ if number.space_before => None,
            T::Int(magnitude) => Some((
                ExprKind::Int((magnitude as i64).wrapping_neg()),
                i64::try_from(magnitude).ok().map(ExprKind::Int),
            )),
            T::Float(value) => Some((ExprKind::Float(-value), Some(ExprKind::Float(value)))),
            _ => None,
        };
        let Some((negative, positive)) = folded else {
            let operand = self.unary()?;
            let span = minus.to(operand.span);
            return Ok(Expr {
                kind: ExprKind::Unary {
                    op: UnaryOp::Neg,
                    expr: Box::new(operand),
                },
                span,
            });
        };
        self.advance();
        let after_literal = self.pos;
        let literal = Expr {
            kind: negative,
            span: minus.to(number.span),
        };
        let base = self.postfix_ops(literal)?;
        if !self.at(T::Pow) || self.pos != after_literal {
            return self.power_tail(base);
        }
        // Directly before `**` the `-` applies to the power: `-2 ** 2` is
        // `-(2 ** 2)`.
        let Some(positive) = positive else {
            return Err(lexer::int_too_large(number.span));
        };
        let base = Expr {
            kind: positive,
            span: number.span,
        };
        let power = self.power_tail(base)?;
        let span = minus.to(power.span);
        Ok(Expr {
            kind: ExprKind::Unary {
                op: UnaryOp::Neg,
                expr: Box::new(power),
            },
            span,
        })
    }

    fn power(&mut self) -> Parse<Expr> {
        let base = self.not()?;
        self.power_tail(base)
    }

    /// `** exponent` after `base`, if present.
    fn power_tail(&mut self, base: Expr) -> Parse<Expr> {
        if !self.eat(T::Pow) {
            return Ok(base);
        }
        let exponent = self.unary()?;
        let span = base.span.to(exponent.span);
        Ok(Expr {
            kind: ExprKind::Binary {
                op: BinaryOp::Pow,
                lhs: Box::new(base),
                rhs: Box::new(exponent),
            },
            span,
        })
    }

    fn not(&mut self) -> Parse<Expr> {
        if !self.at(T::Bang) {
            let primary = self.primary()?;
            return self.postfix_ops(primary);
        }
        let bang = self.advance().span;
        let operand = self.not()?;
        let span = bang.to(operand.span);
        Ok(Expr {
            kind: ExprKind::Unary {
                op: UnaryOp::Not,
                expr: Box::new(operand),
            },
            span,
        })
    }

    /// Member accesses, calls with blocks, indexing and `::` after `expr`.
    fn postfix_ops(&mut self, mut expr: Expr) -> Parse<Expr> {
        loop {
            let start = expr.span;
            let kind = match self.peek().kind {
                T::Dot => {
                    self.advance();
                    let token = self.peek();
                    let (T::Ident(name) | T::Const(name)) = &token.kind else {
                        return Err(self.unexpected("expected a field or method name"));
                    };
                    let name = Ident {
                        name: name.clone(),
                        span: token.span,
                    };
                    self.advance();
                    let args = if self.at(T::LParen) {
                        Some(self.args()?)
                    } else {
                        None
                    };
                    let block = if self.at_kw(Kw::Do) {
                        Some(self.do_block()?)
                    } else {
                        None
                    };
                    if args.is_none()
                        && block.is_none()
                        && let Some(error) = self.bare_argument(self.span_from(start))
                    {
                        return Err(error);
                    }
                    ExprKind::Member {
                        recv: Box::new(expr),
                        name,
                        args,
                        block,
                    }
                }
                T::LBracket => {
                    let open = self.open_bracket();
                    let index = self.expr()?;
                    self.close_bracket(open, T::RBracket, "expected `]`")?;
                    ExprKind::Index {
                        recv: Box::new(expr),
                        index: Box::new(index),
                    }
                }
                T::ColonColon => {
                    self.advance();
                    let name = self.type_name("a constant or variant name")?;
                    ExprKind::Path {
                        recv: Box::new(expr),
                        name,
                    }
                }
                _ => return Ok(expr),
            };
            expr = Expr {
                kind,
                span: self.span_from(start),
            };
        }
    }

    /// `(arg, name: arg)`, from the `(`.
    fn args(&mut self) -> Parse<Vec<Arg>> {
        let open = self.open_bracket();
        let mut args = Vec::new();
        if !self.at(T::RParen) {
            loop {
                let token = self.peek();
                let name = match (&token.kind, &self.peek_at(1).kind) {
                    (T::Ident(name), T::Colon) => {
                        self.advance();
                        self.advance();
                        Some(Ident {
                            name: name.clone(),
                            span: token.span,
                        })
                    }
                    _ => None,
                };
                let value = self.expr()?;
                args.push(Arg { name, value });
                if !self.eat(T::Comma) {
                    break;
                }
            }
        }
        self.close_bracket(open, T::RParen, "expected `,` or `)`")?;
        Ok(args)
    }

    /// Comma-separated expressions up to the bracket `close`, whose opening
    /// bracket at `open` has been consumed.
    fn expr_list(&mut self, open: Span, close: T, label: &str) -> Parse<Vec<Expr>> {
        self.enclosed += 1;
        let mut items = Vec::new();
        if self.peek().kind != close {
            loop {
                items.push(self.expr()?);
                if !self.eat(T::Comma) {
                    break;
                }
            }
        }
        self.close_bracket(open, close, label)?;
        Ok(items)
    }

    fn primary(&mut self) -> Parse<Expr> {
        let token = self.peek();
        let kind = match &token.kind {
            T::Int(magnitude) => {
                self.advance();
                match i64::try_from(*magnitude) {
                    Ok(value) => ExprKind::Int(value),
                    Err(_) => return Err(lexer::int_too_large(token.span)),
                }
            }
            T::Float(value) => {
                self.advance();
                ExprKind::Float(*value)
            }
            T::Str(pieces) => {
                self.advance();
                ExprKind::Str(self.string_parts(pieces)?)
            }
            T::Kw(Kw::True) => {
                self.advance();
                ExprKind::Bool(true)
            }
            T::Kw(Kw::False) => {
                self.advance();
                ExprKind::Bool(false)
            }
            T::Kw(Kw::Self_) => {
                self.advance();
                ExprKind::SelfRef
            }
            T::Ident(name) | T::Const(name) => {
                let lowercase = matches!(token.kind, T::Ident(_));
                self.advance();
                let name = Ident {
                    name: name.clone(),
                    span: token.span,
                };
                match (self.at(T::LParen), lowercase) {
                    (true, true) => ExprKind::Call {
                        name,
                        args: self.args()?,
                    },
                    (true, false) => ExprKind::Construct {
                        name,
                        args: self.args()?,
                    },
                    (false, true) => {
                        if let Some(error) = self.bare_argument(token.span) {
                            return Err(error);
                        }
                        ExprKind::Var(name.name)
                    }
                    (false, false) => ExprKind::Const(name.name),
                }
            }
            T::Kw(Kw::Spawn) => {
                self.advance();
                let next = self.peek();
                if next.kind == T::LParen && !next.space_before {
                    self.advance();
                    ExprKind::Spawn(self.expr_list(next.span, T::RParen, "expected `,` or `)`")?)
                } else if Self::starts_spawn_args(next) {
                    return Err(Diagnostic::new(
                        "`spawn` with arguments needs parentheses here",
                        token.span,
                    )
                    .label("write `spawn(…)` inside a larger expression"));
                } else {
                    ExprKind::Spawn(Vec::new())
                }
            }
            T::LBracket => {
                self.advance();
                ExprKind::List(self.expr_list(token.span, T::RBracket, "expected `,` or `]`")?)
            }
            T::LParen => {
                self.open_bracket();
                let inner = self.expr()?;
                self.close_bracket(token.span, T::RParen, "expected `)`")?;
                inner.kind
            }
            T::Kw(Kw::If) => return self.if_expr("if", false),
            T::Kw(Kw::Unless) => return self.if_expr("unless", true),
            T::Kw(Kw::Case) => return self.case_expr(),
            T::Kw(Kw::Gpu) => {
                return Err(self.unexpected("`gpu` goes before `system`: `gpu system name`"));
            }
            T::Kw(
                Kw::Puts
                | Kw::Print
                | Kw::Emit
                | Kw::Return
                | Kw::Break
                | Kw::Next
                | Kw::While
                | Kw::Until
                | Kw::Loop
                | Kw::Each
                | Kw::On,
            ) => {
                return Err(self.unexpected(format!(
                    "expected an expression, and `{}` is a statement",
                    self.source(token.span)
                )));
            }
            _ => return Err(self.unexpected("expected an expression")),
        };
        Ok(Expr {
            kind,
            span: self.span_from(token.span),
        })
    }

    /// The parts of a string literal, with each interpolation parsed.
    fn string_parts(&self, pieces: &'a [StrPiece]) -> Parse<Vec<StrPart>> {
        let mut parts = Vec::new();
        for piece in pieces {
            match piece {
                StrPiece::Lit(text) => parts.push(StrPart::Lit(text.clone())),
                StrPiece::Interp(tokens) => {
                    let mut inner = Parser::new(self.file, self.text, tokens);
                    let expr = inner.expr()?;
                    if !inner.at(T::InterpEnd) {
                        return Err(inner.unexpected_after_expr("expected `}`"));
                    }
                    parts.push(StrPart::Expr(expr));
                }
            }
        }
        Ok(parts)
    }

    /// `if` or `unless`, through its `end`.
    fn if_expr(&mut self, word: &'static str, negate: bool) -> Parse<Expr> {
        let keyword = self.opened(word);
        self.if_rest(keyword, negate)
    }

    /// An `if` from its condition to the `end`, which a chain of `elsif`s
    /// shares. `start` is the `if`, `unless` or `elsif` keyword.
    fn if_rest(&mut self, start: Span, negate: bool) -> Parse<Expr> {
        let cond = Box::new(self.condition()?);
        self.then()?;
        let then = self.block()?;
        let else_ = match self.peek().kind {
            T::Kw(Kw::Elsif) if negate => {
                return Err(self.unexpected("`unless` can't have an `elsif`"));
            }
            T::Kw(Kw::Elsif) => {
                let keyword = self.advance().span;
                let nested = self.if_rest(keyword, false)?;
                let span = nested.span;
                let stmt = Stmt {
                    kind: StmtKind::Expr(nested),
                    span,
                };
                Some(Block {
                    stmts: vec![stmt],
                    span,
                })
            }
            T::Kw(Kw::Else) => {
                self.advance();
                let block = self.block()?;
                self.end()?;
                Some(block)
            }
            _ => {
                self.end()?;
                None
            }
        };
        Ok(Expr {
            kind: ExprKind::If {
                cond,
                negate,
                then,
                else_,
            },
            span: self.span_from(start),
        })
    }

    /// `case`, through its `end`.
    fn case_expr(&mut self) -> Parse<Expr> {
        let keyword = self.opened("case");
        let subject = Box::new(self.expr()?);
        if is_assign_op(&self.peek().kind) {
            return Err(self.assignment_in_expression());
        }
        self.header_end("expected a newline")?;
        self.skip_newlines();
        if !self.at_closer() {
            return Err(self.unexpected("expected `when`"));
        }
        let mut arms = Vec::new();
        while self.eat(T::Kw(Kw::When)) {
            let mut values = Vec::new();
            loop {
                values.push(self.expr()?);
                if !self.eat(T::Comma) {
                    break;
                }
            }
            self.then()?;
            let body = self.block()?;
            arms.push(CaseArm { values, body });
        }
        let else_ = if self.eat(T::Kw(Kw::Else)) {
            Some(self.block()?)
        } else {
            None
        };
        self.end()?;
        Ok(Expr {
            kind: ExprKind::Case {
                subject,
                arms,
                else_,
            },
            span: self.span_from(keyword),
        })
    }
}

#[cfg(test)]
mod tests;
