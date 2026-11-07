//! Parser of the formula of a Calculate line: its references, or the reason it does not parse.
//!
//! A formula holds numbers, references, `+ - * / ^`, parentheses, and the functions `length` and `direction` of one
//! value and `smaller` and `larger` of two. A reference is a bare id (a parameter), `l2` or `l2.amount` (a result or
//! one part of it, or a property of the place it names), or `my.x` and `my_new.x` (a property of mine).
//!
//! [`parse`] returns the formula as a [`FormulaTree`], the form the lowering reads.

use crate::sheet::limits::{MAX_FORMULA_DEPTH, MAX_FORMULA_NESTING};

/// Functions of a formula, each with the number of values it takes.
const FUNCTIONS: [(&str, usize); 4] = [("length", 1), ("direction", 1), ("smaller", 2), ("larger", 2)];

/// A formula parsed: numbers, references, operators and calls, each operator holding its operands.
#[derive(Debug, Clone, PartialEq)]
pub(in crate::sheet) enum FormulaTree<'f> {
    /// A number as written, such as `2.5e-3`.
    Number(&'f str),
    /// A reference, such as `l2.amount` or `infection_rate`.
    Reference(&'f str),
    /// The negation of a value.
    Negate(Box<Self>),
    /// Two values joined by one of `+ - * / ^`.
    Binary(char, Box<Self>, Box<Self>),
    /// A call of a function of a formula, by name, with its values.
    Call(&'f str, Vec<Self>),
}

impl FormulaTree<'_> {
    /// Returns the depth of the tree: 1 for a number or a reference, and one more than the deepest value for an
    /// operation.
    fn depth(&self) -> usize {
        match self {
            Self::Number(_) | Self::Reference(_) => 1,
            Self::Negate(value) => value.depth() + 1,
            Self::Binary(_, left, right) => left.depth().max(right.depth()) + 1,
            Self::Call(_, values) => values.iter().map(Self::depth).max().unwrap_or(0) + 1,
        }
    }
}

/// One token of a formula.
#[derive(Debug, Clone, PartialEq)]
enum Token<'f> {
    /// A number, as written.
    Number(&'f str),
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
pub(in crate::sheet) fn references(formula: &str) -> Result<Vec<&str>, String> {
    parse_with_references(formula, false).map(|(_, references)| references)
}

/// Returns `formula` parsed.
///
/// # Errors
///
/// Returns the reason the formula does not parse, as [`references`] does, and an error for a formula whose operations
/// nest past [`MAX_FORMULA_DEPTH`].
pub(in crate::sheet) fn parse(formula: &str) -> Result<FormulaTree<'_>, String> {
    parse_with_references(formula, true).map(|(tree, _)| tree)
}

/// Returns `formula` parsed, with its references in the order they appear.
///
/// With `build` false the tree is not built, and every operation reads as an empty number. A long sum then takes no
/// stack to drop.
fn parse_with_references(formula: &str, build: bool) -> Result<(FormulaTree<'_>, Vec<&str>), String> {
    let mut parser = Parser {
        tokens: tokens(formula)?,
        position: 0,
        references: Vec::new(),
        depth: 0,
        build,
    };
    if parser.tokens.is_empty() {
        return Err("the formula is empty".to_owned());
    }
    let tree = parser.expression()?;
    match parser.peek() {
        None => Ok((tree, parser.references)),
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
            tokens.push(Token::Number(&formula[start..index]));
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
        Token::Number(_) => "number".to_owned(),
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
    /// Whether the parser builds the tree.
    build: bool,
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

    /// Returns `tree`, an operation, when the parser builds the tree, and an empty number otherwise.
    ///
    /// # Errors
    ///
    /// Returns an error for a tree deeper than [`MAX_FORMULA_DEPTH`]. Its values are no deeper than the limit, so the
    /// depth takes little stack to find.
    fn made(&self, tree: FormulaTree<'f>) -> Result<FormulaTree<'f>, String> {
        if !self.build {
            return Ok(FormulaTree::Number(""));
        }
        if tree.depth() > MAX_FORMULA_DEPTH {
            return Err(format!(
                "the formula nests operations more than {MAX_FORMULA_DEPTH} deep, the most a formula can run with"
            ));
        }
        Ok(tree)
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
    fn expression(&mut self) -> Result<FormulaTree<'f>, String> {
        let mut tree = self.term()?;
        while self.at('+') || self.at('-') {
            let symbol = if self.at('+') { '+' } else { '-' };
            self.position += 1;
            let right = self.term()?;
            tree = self.made(FormulaTree::Binary(symbol, Box::new(tree), Box::new(right)))?;
        }
        Ok(tree)
    }

    /// Reads a product: powers joined by `*` and `/`.
    fn term(&mut self) -> Result<FormulaTree<'f>, String> {
        let mut tree = self.power()?;
        while self.at('*') || self.at('/') {
            let symbol = if self.at('*') { '*' } else { '/' };
            self.position += 1;
            let right = self.power()?;
            tree = self.made(FormulaTree::Binary(symbol, Box::new(tree), Box::new(right)))?;
        }
        Ok(tree)
    }

    /// Reads a power. A power groups from the right, and a leading `-` negates the whole power, so `-x ^ 2` is
    /// `-(x ^ 2)`.
    fn power(&mut self) -> Result<FormulaTree<'f>, String> {
        let negated = self.at('-');
        if negated {
            self.position += 1;
        }
        let mut tree = self.atom()?;
        if self.at('^') {
            self.position += 1;
            self.open_group()?;
            let exponent = self.power()?;
            self.depth -= 1;
            tree = self.made(FormulaTree::Binary('^', Box::new(tree), Box::new(exponent)))?;
        }
        if negated {
            self.made(FormulaTree::Negate(Box::new(tree)))
        } else {
            Ok(tree)
        }
    }

    /// Reads a number, a reference, a call or a group in parentheses.
    fn atom(&mut self) -> Result<FormulaTree<'f>, String> {
        let Some(token) = self.peek().cloned() else {
            return Err("the formula ends where it expects a value".to_owned());
        };
        self.position += 1;
        match token {
            Token::Number(text) => Ok(FormulaTree::Number(text)),
            Token::Symbol('(') => {
                self.open_group()?;
                let tree = self.expression()?;
                self.expect(')')?;
                self.depth -= 1;
                Ok(tree)
            }
            Token::Word(word) => {
                let Some(&(name, takes)) = FUNCTIONS.iter().find(|(name, _)| *name == word) else {
                    self.references.push(word);
                    return Ok(FormulaTree::Reference(word));
                };
                self.expect('(')?;
                self.open_group()?;
                let mut values = vec![self.expression()?];
                while self.at(',') {
                    self.position += 1;
                    values.push(self.expression()?);
                }
                let count = values.len();
                self.expect(')')?;
                self.depth -= 1;
                if count == takes {
                    self.made(FormulaTree::Call(name, values))
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
    use super::{parse, references};
    use crate::sheet::limits::{MAX_FORMULA_DEPTH, MAX_FORMULA_NESTING};

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
    fn a_formula_parses_to_its_tree() {
        use super::FormulaTree::{Binary, Call, Negate, Number, Reference};
        assert_eq!(
            parse("-x ^ 2 + smaller(a, 3) * 2.5e-3"),
            Ok(Binary(
                '+',
                Box::new(Negate(Box::new(Binary(
                    '^',
                    Box::new(Reference("x")),
                    Box::new(Number("2"))
                )))),
                Box::new(Binary(
                    '*',
                    Box::new(Call("smaller", vec![Reference("a"), Number("3")])),
                    Box::new(Number("2.5e-3"))
                ))
            ))
        );
        assert_eq!(
            parse("a - b - c"),
            Ok(Binary(
                '-',
                Box::new(Binary('-', Box::new(Reference("a")), Box::new(Reference("b")))),
                Box::new(Reference("c"))
            ))
        );
        assert_eq!(parse("2 ^ 3 ^ 2"), parse("2 ^ (3 ^ 2)"));
    }

    #[test]
    fn a_formula_nesting_operations_past_the_depth_limit_does_not_parse() {
        let sum = |terms: usize| vec!["x"; terms].join(" + ");
        assert!(parse(&sum(MAX_FORMULA_DEPTH)).is_ok());
        assert!(parse(&sum(MAX_FORMULA_DEPTH + 2)).is_err());
        assert!(
            references(&sum(MAX_FORMULA_DEPTH + 2)).is_ok(),
            "the checker reads any length"
        );
        assert!(parse(&vec!["x"; 100_000].join("+")).is_err());
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
