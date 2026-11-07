//! Parser of the formula of a Calculate line: its references, or the reason it does not parse.
//!
//! A formula holds numbers, references, `+ - * / ^`, parentheses, and the functions `length` and `direction` of one
//! value and `smaller` and `larger` of two. A reference is a bare id (a parameter), `l2` or `l2.amount` (a result or
//! one part of it, or a property of the place it names), or `my.x` and `my_new.x` (a property of mine).

use crate::sheet::limits::MAX_FORMULA_NESTING;

/// Functions of a formula, each with the number of values it takes.
const FUNCTIONS: [(&str, usize); 4] = [("length", 1), ("direction", 1), ("smaller", 2), ("larger", 2)];

/// One token of a formula.
#[derive(Debug, Clone, PartialEq)]
enum Token<'f> {
    /// A number.
    Number,
    /// A word: a reference or a function's name.
    Word(&'f str),
    /// An operator, a parenthesis or a comma.
    Symbol(char),
}

/// Returns the references of `formula` in the order they appear.
///
/// # Errors
///
/// Returns the reason the formula does not parse, such as "the formula ends where it expects a value". A formula
/// nesting groups past [`MAX_FORMULA_NESTING`] does not parse.
pub(super) fn references(formula: &str) -> Result<Vec<&str>, String> {
    let mut parser = Parser {
        tokens: tokens(formula)?,
        position: 0,
        references: Vec::new(),
        depth: 0,
    };
    if parser.tokens.is_empty() {
        return Err("the formula is empty".to_owned());
    }
    parser.expression()?;
    match parser.peek() {
        None => Ok(parser.references),
        Some(token) => Err(format!("the formula holds an unexpected {}", describe(token))),
    }
}

/// Returns the tokens of `formula`.
fn tokens(formula: &str) -> Result<Vec<Token<'_>>, String> {
    let bytes = formula.as_bytes();
    let mut tokens = Vec::new();
    let mut index = 0;
    while let Some(&byte) = bytes.get(index) {
        let start = index;
        if byte.is_ascii_whitespace() {
            index += 1;
        } else if byte.is_ascii_digit() {
            index = digits(bytes, index);
            if bytes.get(index) == Some(&b'.') && bytes.get(index + 1).is_some_and(u8::is_ascii_digit) {
                index = digits(bytes, index + 1);
            }
            if bytes.get(index) == Some(&b'e') {
                let sign = usize::from(bytes.get(index + 1) == Some(&b'-'));
                if bytes.get(index + 1 + sign).is_some_and(u8::is_ascii_digit) {
                    index = digits(bytes, index + 1 + sign);
                }
            }
            tokens.push(Token::Number);
        } else if byte.is_ascii_lowercase() {
            index = word(bytes, index);
            if bytes.get(index) == Some(&b'.') && bytes.get(index + 1).is_some_and(u8::is_ascii_lowercase) {
                index = word(bytes, index + 1);
            }
            tokens.push(Token::Word(&formula[start..index]));
        } else if b"+-*/^(),".contains(&byte) {
            index += 1;
            tokens.push(Token::Symbol(char::from(byte)));
        } else {
            let rest: String = formula[start..].chars().take(12).collect();
            return Err(format!("the formula cannot be read at '{rest}'"));
        }
    }
    Ok(tokens)
}

/// Returns the index past the digits at `index` of `bytes`.
fn digits(bytes: &[u8], mut index: usize) -> usize {
    while bytes.get(index).is_some_and(u8::is_ascii_digit) {
        index += 1;
    }
    index
}

/// Returns the index past the word characters, `[a-z0-9_]`, at `index` of `bytes`.
fn word(bytes: &[u8], mut index: usize) -> usize {
    while bytes
        .get(index)
        .is_some_and(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_')
    {
        index += 1;
    }
    index
}

/// Returns the words of `token` in a message.
fn describe(token: &Token<'_>) -> String {
    match token {
        Token::Number => "number".to_owned(),
        Token::Word(word) => format!("'{word}'"),
        Token::Symbol(symbol) => format!("'{symbol}'"),
    }
}

/// A recursive descent over the tokens of one formula.
struct Parser<'f> {
    /// Tokens of the formula.
    tokens: Vec<Token<'f>>,
    /// Index of the next token.
    position: usize,
    /// References read so far.
    references: Vec<&'f str>,
    /// Number of groups open at the next token: parentheses, calls and right sides of `^`.
    depth: usize,
}

impl<'f> Parser<'f> {
    /// Returns the next token without taking it.
    fn peek(&self) -> Option<&Token<'f>> {
        self.tokens.get(self.position)
    }

    /// Returns whether the next token is the symbol `symbol`.
    fn at(&self, symbol: char) -> bool {
        self.peek() == Some(&Token::Symbol(symbol))
    }

    /// Takes the symbol `symbol`.
    fn expect(&mut self, symbol: char) -> Result<(), String> {
        if self.at(symbol) {
            self.position += 1;
            Ok(())
        } else {
            match self.peek() {
                None => Err(format!("the formula ends where it expects '{symbol}'")),
                Some(token) => Err(format!(
                    "the formula holds {} where it expects '{symbol}'",
                    describe(token)
                )),
            }
        }
    }

    /// Opens one more group.
    ///
    /// # Errors
    ///
    /// Returns an error past [`MAX_FORMULA_NESTING`] open groups. Each group takes stack space as it is parsed.
    fn open_group(&mut self) -> Result<(), String> {
        self.depth += 1;
        if self.depth > MAX_FORMULA_NESTING {
            return Err(format!("the formula nests groups more than {MAX_FORMULA_NESTING} deep"));
        }
        Ok(())
    }

    /// Reads a sum: terms joined by `+` and `-`.
    fn expression(&mut self) -> Result<(), String> {
        self.term()?;
        while self.at('+') || self.at('-') {
            self.position += 1;
            self.term()?;
        }
        Ok(())
    }

    /// Reads a product: powers joined by `*` and `/`.
    fn term(&mut self) -> Result<(), String> {
        self.power()?;
        while self.at('*') || self.at('/') {
            self.position += 1;
            self.power()?;
        }
        Ok(())
    }

    /// Reads a power. A power groups from the right.
    fn power(&mut self) -> Result<(), String> {
        if self.at('-') {
            self.position += 1;
        }
        self.atom()?;
        if self.at('^') {
            self.position += 1;
            self.open_group()?;
            self.power()?;
            self.depth -= 1;
        }
        Ok(())
    }

    /// Reads a number, a reference, a call or a group in parentheses.
    fn atom(&mut self) -> Result<(), String> {
        let Some(token) = self.peek().cloned() else {
            return Err("the formula ends where it expects a value".to_owned());
        };
        self.position += 1;
        match token {
            Token::Number => Ok(()),
            Token::Symbol('(') => {
                self.open_group()?;
                self.expression()?;
                self.expect(')')?;
                self.depth -= 1;
                Ok(())
            }
            Token::Word(word) => {
                let Some(&(name, takes)) = FUNCTIONS.iter().find(|(name, _)| *name == word) else {
                    self.references.push(word);
                    return Ok(());
                };
                self.expect('(')?;
                self.open_group()?;
                self.expression()?;
                let mut count = 1;
                while self.at(',') {
                    self.position += 1;
                    self.expression()?;
                    count += 1;
                }
                self.expect(')')?;
                self.depth -= 1;
                if count == takes {
                    Ok(())
                } else {
                    Err(format!(
                        "{name} takes {takes} values in a formula, and this call passes {count}"
                    ))
                }
            }
            Token::Symbol(_) => Err(format!("the formula holds an unexpected {}", describe(&token))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::references;
    use crate::sheet::limits::MAX_FORMULA_NESTING;

    #[test]
    fn a_formula_returns_its_references_in_order() {
        assert_eq!(
            references("trail_falloff * l2.amount + my.reward_held"),
            Ok(vec!["trail_falloff", "l2.amount", "my.reward_held"])
        );
        assert_eq!(references("length(my.velocity)"), Ok(vec!["my.velocity"]));
        assert_eq!(references("-2.5e-3 ^ -x ^ 2"), Ok(vec!["x"]));
        assert_eq!(references("smaller(a, larger(b, 1))"), Ok(vec!["a", "b"]));
    }

    #[test]
    fn a_malformed_formula_is_refused_with_its_reason() {
        for (formula, reason) in [
            ("", "the formula is empty"),
            ("a +", "the formula ends where it expects a value"),
            ("(a", "the formula ends where it expects ')'"),
            ("a b", "the formula holds an unexpected 'b'"),
            (
                "smaller(a)",
                "smaller takes 2 values in a formula, and this call passes 1",
            ),
            ("length a", "the formula holds 'a' where it expects '('"),
            ("a % b", "the formula cannot be read at '% b'"),
            ("*", "the formula holds an unexpected '*'"),
        ] {
            assert_eq!(references(formula), Err(reason.to_owned()), "{formula}");
        }
    }

    #[test]
    fn a_formula_nesting_past_the_limit_is_refused_without_overflowing() {
        let too_deep = format!("the formula nests groups more than {MAX_FORMULA_NESTING} deep");
        let groups = |depth: usize| format!("{}x{}", "(".repeat(depth), ")".repeat(depth));
        assert_eq!(references(&groups(MAX_FORMULA_NESTING)), Ok(vec!["x"]));
        assert_eq!(references(&groups(MAX_FORMULA_NESTING + 1)), Err(too_deep.clone()));
        assert_eq!(references(&"(".repeat(100_000)), Err(too_deep.clone()));
        assert_eq!(references(&"length(".repeat(100_000)), Err(too_deep.clone()));
        let powers = |count: usize| vec!["x"; count + 1].join("^");
        assert!(references(&powers(MAX_FORMULA_NESTING)).is_ok());
        assert_eq!(references(&powers(100_000)), Err(too_deep));
        // Groups side by side do not nest.
        assert!(references(&vec!["(x)"; 100_000].join("+")).is_ok());
    }
}
