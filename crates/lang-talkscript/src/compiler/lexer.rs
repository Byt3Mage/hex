use super::token::{Span, Token};
use crate::{compiler::token::TokenType, tt};

// Error Types
#[derive(Debug, Clone, Copy, thiserror::Error)]
pub enum LexErrorKind {
    #[error("Unexpected character `{0}`")]
    UnexpectedChar(char),
    #[error("Unterminated string")]
    UnterminatedString,
    #[error("Invalid escape character `{0}`")]
    InvalidEscape(char),
    #[error("Invalid suffix `{0}` for token type `{1:?}`")]
    InvalidSuffix(char, TokenType),
}

#[derive(Debug, thiserror::Error)]
#[error("{kind} encountered at {span:?}\n{hint}")]
pub struct LexerError {
    pub kind: LexErrorKind,
    pub span: Span,
    pub hint: &'static str,
}

pub struct Lexer<'a> {
    input: &'a str,
    chars: std::str::CharIndices<'a>,
    current: Option<(usize, char)>,
    line: usize,
    column: usize,
    byte_pos: usize,
}

impl<'a> Lexer<'a> {
    pub fn new(input: &'a str) -> Self {
        let mut chars = input.char_indices();
        let current = chars.next();

        Self {
            input,
            chars,
            current,
            line: 1,
            column: 1,
            byte_pos: 0,
        }
    }

    fn advance(&mut self) {
        if let Some((pos, ch)) = self.current {
            self.byte_pos = pos + ch.len_utf8();

            if ch == '\n' {
                self.line += 1;
                self.column = 0;
            } else {
                self.column += 1;
            }
        }
        self.current = self.chars.next();
    }

    #[inline]
    fn make_token(&self, ty: TokenType, start_byte: usize, lexeme: &'a str) -> Token<'a> {
        Token {
            ty,
            lexeme,
            span: Span {
                start: start_byte,
                end: self.byte_pos,
                line: self.line,
                column: self.column,
            },
        }
    }

    pub fn next_token(&mut self) -> Result<Token<'a>, LexerError> {
        self.skip_whitespace();

        let Some((o, ch)) = self.current else {
            return Ok(self.make_token(tt![eof], self.byte_pos, "<eof>"));
        };

        match ch {
            // Single symbols
            '(' => Ok(self.make_single(tt!['('], o)),
            ')' => Ok(self.make_single(tt![')'], o)),
            '{' => Ok(self.make_single(tt!['{'], o)),
            '}' => Ok(self.make_single(tt!['}'], o)),
            '[' => Ok(self.make_single(tt!['['], o)),
            ']' => Ok(self.make_single(tt![']'], o)),
            ',' => Ok(self.make_single(tt![,], o)),
            ';' => Ok(self.make_single(tt![;], o)),
            '@' => Ok(self.make_single(tt![@], o)),

            // Double symbols
            '.' => Ok(self.make_double('.', tt![.], tt![..], o)),
            ':' => Ok(self.make_double(':', tt![:], tt![::], o)),
            '!' => Ok(self.make_double('=', tt![!], tt![!=], o)),
            '+' => Ok(self.make_double('=', tt![+], tt![+=], o)),
            '*' => Ok(self.make_double('=', tt![*], tt![*=], o)),
            '/' => Ok(self.make_double('=', tt![/], tt![/=], o)),
            '%' => Ok(self.make_double('=', tt![%], tt![%=], o)),
            '&' => Ok(self.make_double('=', tt![&], tt![&=], o)),
            '|' => Ok(self.make_double('=', tt![|], tt![|=], o)),
            '^' => Ok(self.make_double('=', tt![^], tt![^=], o)),

            // Complex double or triple symbols
            '?' => Ok(self.make_question(o)),
            '=' => Ok(self.make_equals(o)),
            '-' => Ok(self.make_minus(o)),
            '>' => Ok(self.make_greater(o)),
            '<' => Ok(self.make_less(o)),

            // String literals
            '"' => self.make_string(o),

            // Ints and float literals
            c if c.is_ascii_digit() => self.make_number(o),

            // Idents
            c if c.is_ascii_alphabetic() || c == '_' => Ok(self.make_ident_or_kw(o)),

            c => Err(LexerError {
                kind: LexErrorKind::UnexpectedChar(c),
                span: Span::new(o, self.byte_pos, self.line, self.column),
                hint: "",
            }),
        }
    }

    fn skip_whitespace(&mut self) {
        while let Some((_, ch)) = self.current {
            if ch.is_whitespace() {
                self.advance();
            } else if ch == '/' {
                if let Some((_, '/')) = self.peek_next() {
                    self.skip_line_comment();
                } else {
                    break;
                }
            } else {
                break;
            }
        }
    }

    fn peek_next(&self) -> Option<(usize, char)> {
        self.chars.clone().next()
    }

    fn skip_line_comment(&mut self) {
        // Skip both '/' characters
        self.advance();
        self.advance();

        // Consume until newline or EOF
        while let Some((_, ch)) = self.current {
            if ch == '\n' {
                break;
            }
            self.advance();
        }
    }

    #[inline]
    fn make_single(&mut self, ty: TokenType, start: usize) -> Token<'a> {
        self.advance();
        self.make_token(ty, start, &self.input[start..self.byte_pos])
    }

    fn make_double(
        &mut self,
        second_ch: char,
        single: TokenType,
        double: TokenType,
        start_byte: usize,
    ) -> Token<'a> {
        self.advance();
        match self.current {
            Some((_, ch)) if ch == second_ch => self.make_single(double, start_byte),
            _ => self.make_token(single, start_byte, &self.input[start_byte..self.byte_pos]),
        }
    }

    fn make_equals(&mut self, start_byte: usize) -> Token<'a> {
        self.advance(); // Consume '='
        match self.current {
            Some((_, '=')) => self.make_single(tt![==], start_byte),
            Some((_, '>')) => self.make_single(tt![=>], start_byte),
            _ => self.make_token(tt![=], start_byte, &self.input[start_byte..self.byte_pos]),
        }
    }

    fn make_minus(&mut self, start_byte: usize) -> Token<'a> {
        self.advance(); // Consume '-'

        match self.current {
            Some((_, '=')) => self.make_single(tt![-=], start_byte),
            Some((_, '>')) => self.make_single(tt![->], start_byte),
            _ => self.make_token(tt![-], start_byte, &self.input[start_byte..start_byte + 1]),
        }
    }

    fn make_question(&mut self, start_byte: usize) -> Token<'a> {
        self.advance(); // Consume '?'

        match self.current {
            Some((_, '.')) => self.make_single(tt![?.], start_byte),
            Some((_, ':')) => self.make_single(tt![?:], start_byte),
            _ => self.make_token(tt![?], start_byte, &self.input[start_byte..start_byte + 1]),
        }
    }

    fn make_greater(&mut self, start_byte: usize) -> Token<'a> {
        self.advance();

        match self.current {
            Some((_, '=')) => self.make_single(tt![>=], start_byte),
            Some((_, '>')) => self.make_double('=', tt![>>], tt![>>=], start_byte),
            _ => self.make_token(tt![>], start_byte, &self.input[start_byte..self.byte_pos]),
        }
    }

    fn make_less(&mut self, start_byte: usize) -> Token<'a> {
        self.advance();

        match self.current {
            Some((_, '=')) => self.make_single(tt![<=], start_byte),
            Some((_, '<')) => self.make_double('=', tt![<<], tt![<<=], start_byte),
            _ => self.make_token(tt![<], start_byte, &self.input[start_byte..self.byte_pos]),
        }
    }

    fn make_string(&mut self, start_byte: usize) -> Result<Token<'a>, LexerError> {
        let start_line = self.line;
        let start_col = self.column;

        self.advance(); // Skip opening quote

        let mut escape = false;
        let mut end_byte;

        while let Some((byte, ch)) = self.current {
            end_byte = byte + ch.len_utf8();

            match ch {
                '\\' if !escape => {
                    escape = true;
                    self.advance();
                }
                '"' if !escape => {
                    self.advance();

                    return Ok(self.make_token(
                        tt![str_lit],
                        start_byte,
                        &self.input[start_byte..end_byte],
                    ));
                }
                c if escape => match c {
                    'n' | 't' | 'r' | '\\' | '"' | '\'' => {
                        escape = false;
                        self.advance();
                    }
                    _ => {
                        return Err(LexerError {
                            kind: LexErrorKind::InvalidEscape(ch),
                            span: Span::new(byte, end_byte, self.line, self.column),
                            hint: r#"hint: Valid escapes: \n, \t, \r, \\, \", \'"#,
                        });
                    }
                },
                _ => {
                    escape = false;
                    self.advance();
                }
            }
        }

        // EOF reached without closing quote
        Err(LexerError {
            kind: LexErrorKind::UnterminatedString,
            span: Span::new(start_byte, self.byte_pos, start_line, start_col),
            hint: r#"hint: Did you forget a closing `"`?"#,
        })
    }

    fn make_number(&mut self, start_byte: usize) -> Result<Token<'a>, LexerError> {
        let mut ty = tt![cint_lit];
        let mut end_byte = start_byte;

        while let Some((byte_pos, ch)) = self.current {
            match ch {
                '0'..='9' => {
                    end_byte = byte_pos + ch.len_utf8();
                    self.advance();
                }
                '.' if ty != tt![float_lit] => {
                    ty = tt![float_lit];
                    end_byte = byte_pos + ch.len_utf8();
                    self.advance();
                }
                _ => break,
            }
        }

        // Check for suffix
        if let Some((b, ch)) = self.current {
            match (ch, ty) {
                ('i' | 'u', tt![float_lit]) => {
                    return Err(LexerError {
                        kind: LexErrorKind::InvalidSuffix(ch, ty),
                        span: Span::new(start_byte, b + ch.len_utf8(), self.line, self.column),
                        hint: "hint: float literals cannot have integer suffixes",
                    });
                }

                ('i', _) => {
                    ty = tt![int_lit];
                    self.advance();
                }
                ('u', _) => {
                    ty = tt![uint_lit];
                    self.advance();
                }
                ('f', _) => {
                    ty = tt![float_lit];
                    self.advance();
                }
                _ => {}
            }
        }

        Ok(self.make_token(ty, start_byte, &self.input[start_byte..end_byte]))
    }

    fn make_ident_or_kw(&mut self, start_byte: usize) -> Token<'a> {
        let mut end_byte = start_byte;

        while let Some((byte_pos, ch)) = self.current {
            if ch.is_ascii_alphanumeric() || ch == '_' {
                end_byte = byte_pos + ch.len_utf8();
                self.advance();
            } else {
                break;
            }
        }

        let lexeme = &self.input[start_byte..end_byte];

        let ty = match lexeme {
            "and" => tt![and],
            "any" => tt![any],
            "as" => tt![as],
            "bool" => tt![bool],
            "break" => tt![break],
            "char" => tt![char],
            "comptime" => tt![comptime],
            "const" => tt![const],
            "continue" => tt![continue],
            "cint" => tt![cint],
            "cstr" => tt![cstr],
            "else" => tt![else],
            "enum" => tt![enum],
            "false" => tt![false],
            "float" => tt![float],
            "fn" => tt![fn],
            "for" => tt![for],
            "if" => tt![if],
            "import" => tt![import],
            "in" => tt![in],
            "int" => tt![int],
            "let" => tt![let],
            "loop" => tt![loop],
            "macro" => tt![macro],
            "match" => tt![match],
            "mod" => tt![mod],
            "mut" => tt![mut],
            "null" => tt![null],
            "or" => tt![or],
            "package" => tt![package],
            "pub" => tt![pub],
            "return" => tt![return],
            "self" => tt![self],
            "static" => tt![static],
            "str" => tt![str],
            "struct" => tt![struct],
            "trait" => tt![trait],
            "true" => tt![true],
            "uint" => tt![uint],
            "union" => tt![union],
            "use" => tt![use],
            "void" => tt![void],
            "while" => tt![while],
            _ => tt![ident],
        };

        self.make_token(ty, start_byte, lexeme)
    }
}
