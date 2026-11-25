//! Expansion of a `math.expr` node into the arithmetic, comparison, logic and vector kinds its text names.
//!
//! An *expansion* is the list of nodes the text becomes, each with a fresh id, and the value each output of the node
//! takes: a link to an expanded node, or the value of an input or a literal the text names directly. Every expanded
//! node sits in the region of the expression node.

use std::collections::BTreeMap;

use crate::catalogue::{component_of, expression_inputs, name_list, vector_of};
use crate::node::Node;
use crate::types::ValueType;
use crate::value::{InputValue, Literal, PortRef};

/// Returns whether an expression's input can have the type `value_type`.
pub(crate) fn takes_type(value_type: &ValueType) -> bool {
    matches!(
        value_type,
        ValueType::Bool
            | ValueType::U32
            | ValueType::I32
            | ValueType::F32
            | ValueType::U64
            | ValueType::F64
            | ValueType::Vec2U
            | ValueType::Vec2I
            | ValueType::Vec2F
    )
}

/// Nodes an expression expands into, and the value of each output.
#[derive(Debug, Clone)]
pub(crate) struct Expansion {
    /// Expanded nodes, in the order the text computes them.
    pub nodes: Vec<Node>,
    /// Value of each output, by name.
    pub outputs: BTreeMap<String, InputValue>,
    /// Type of each output, by name.
    pub types: BTreeMap<String, ValueType>,
}

/// Returns the expansion of the expression node `node`, numbering the expanded nodes from `*next` up.
///
/// # Errors
///
/// Returns the message for a text that does not parse, names an unknown name or call, mixes types, or leaves an
/// output unset, and for ids past `u32::MAX`.
pub(crate) fn expand(node: &Node, next: &mut u32) -> Result<Expansion, String> {
    let text = node
        .config_str("text")
        .ok_or_else(|| "has no `config.text`".to_owned())?;
    let outputs = name_list(node, "outputs");
    let mut builder = Builder {
        node,
        next,
        nodes: Vec::new(),
        names: BTreeMap::new(),
    };
    for (name, value_type) in expression_inputs(node) {
        let value = node
            .inputs
            .get(&name)
            .cloned()
            .ok_or_else(|| format!("leaves the input `{name}` unwritten"))?;
        builder.names.insert(name, Operand::Value(value, value_type));
    }
    let tokens = tokenize(text)?;
    let mut parser = Parser { tokens, at: 0 };
    let mut expansion = Expansion {
        nodes: Vec::new(),
        outputs: BTreeMap::new(),
        types: BTreeMap::new(),
    };
    loop {
        parser.skip_separators();
        if parser.at_end() {
            break;
        }
        let name = parser.name()?;
        if !outputs.contains(&name) {
            return Err(format!("sets '{name}', which `config.outputs` does not list"));
        }
        if expansion.outputs.contains_key(&name) {
            return Err(format!("sets the output '{name}' twice"));
        }
        parser.expect(&Token::Assign)?;
        let tree = parser.expression()?;
        if !parser.at_end() && !parser.at_separator() {
            return Err(format!("has text after the assignment of '{name}'"));
        }
        let operand = builder.build(&tree)?;
        let (value, value_type) = Builder::settle(operand)?;
        builder
            .names
            .insert(name.clone(), Operand::Value(value.clone(), value_type.clone()));
        expansion.outputs.insert(name.clone(), value);
        expansion.types.insert(name, value_type);
    }
    if let Some(missing) = outputs.iter().find(|name| !expansion.outputs.contains_key(*name)) {
        return Err(format!("never sets the output '{missing}'"));
    }
    expansion.nodes = builder.nodes;
    Ok(expansion)
}

/// Returns the number of nodes the expression node `node` expands into, 0 for one that does not expand.
pub(crate) fn expanded_nodes(node: &Node) -> Vec<Node> {
    let mut next = 1;
    expand(node, &mut next)
        .map(|expansion| expansion.nodes)
        .unwrap_or_default()
}

/// One token of an expression's text.
#[derive(Debug, Clone, PartialEq)]
enum Token {
    Number(String),
    Name(String),
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    Equal,
    NotEqual,
    Assign,
    Open,
    Close,
    Comma,
    Dot,
    Separator,
}

impl Token {
    /// Returns the kind a comparison token expands into, `None` for a token that is no comparison.
    fn comparison(&self) -> Option<&'static str> {
        match self {
            Self::Less => Some("cmp.lt"),
            Self::LessEqual => Some("cmp.le"),
            Self::Greater => Some("cmp.gt"),
            Self::GreaterEqual => Some("cmp.ge"),
            Self::Equal => Some("cmp.eq"),
            Self::NotEqual => Some("cmp.ne"),
            _ => None,
        }
    }
}

/// Returns the tokens of `text`.
fn tokenize(text: &str) -> Result<Vec<Token>, String> {
    let chars: Vec<char> = text.chars().collect();
    let mut tokens = Vec::new();
    let mut at = 0;
    while let Some(&c) = chars.get(at) {
        let next = chars.get(at + 1).copied();
        let (token, length) = match c {
            ' ' | '\t' | '\r' => {
                at += 1;
                continue;
            }
            '\n' | ';' => (Token::Separator, 1),
            '+' => (Token::Plus, 1),
            '-' => (Token::Minus, 1),
            '*' => (Token::Star, 1),
            '/' => (Token::Slash, 1),
            '%' => (Token::Percent, 1),
            '(' => (Token::Open, 1),
            ')' => (Token::Close, 1),
            ',' => (Token::Comma, 1),
            '<' if next == Some('=') => (Token::LessEqual, 2),
            '<' => (Token::Less, 1),
            '>' if next == Some('=') => (Token::GreaterEqual, 2),
            '>' => (Token::Greater, 1),
            '=' if next == Some('=') => (Token::Equal, 2),
            '=' => (Token::Assign, 1),
            '!' if next == Some('=') => (Token::NotEqual, 2),
            '.' if !next.is_some_and(|digit| digit.is_ascii_digit()) => (Token::Dot, 1),
            c if c.is_ascii_digit() || c == '.' => {
                let start = at;
                let mut end = at;
                while let Some(&digit) = chars.get(end) {
                    let exponent_sign =
                        (digit == '+' || digit == '-') && end > start && matches!(chars.get(end - 1), Some('e' | 'E'));
                    if digit.is_ascii_digit() || digit == '.' || digit == 'e' || digit == 'E' || exponent_sign {
                        end += 1;
                    } else {
                        break;
                    }
                }
                (Token::Number(chars[start..end].iter().collect()), end - start)
            }
            c if c.is_ascii_alphabetic() || c == '_' => {
                let start = at;
                let mut end = at;
                while chars
                    .get(end)
                    .is_some_and(|letter| letter.is_ascii_alphanumeric() || *letter == '_')
                {
                    end += 1;
                }
                (Token::Name(chars[start..end].iter().collect()), end - start)
            }
            other => return Err(format!("contains the invalid character '{other}'")),
        };
        tokens.push(token);
        at += length;
    }
    Ok(tokens)
}

/// A parsed expression.
#[derive(Debug, Clone)]
enum Tree {
    Number(String),
    Name(String),
    Bool(bool),
    Negate(Box<Self>),
    Not(Box<Self>),
    Binary(Token, Box<Self>, Box<Self>),
    And(Box<Self>, Box<Self>),
    Or(Box<Self>, Box<Self>),
    Call(String, Vec<Self>),
    Component(Box<Self>, String),
}

/// Deepest nesting a parse follows, which keeps a hostile text from running out of stack.
///
/// Each parenthesis, unary operator, operand of a chain such as `a + b + c` and component read counts one level, since
/// each is one level of the tree that the expansion and the drop of the tree recurse through.
const MAX_DEPTH: usize = 64;

/// Recursive descent over the tokens of one expression.
struct Parser {
    tokens: Vec<Token>,
    at: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.at)
    }

    fn at_end(&self) -> bool {
        self.at >= self.tokens.len()
    }

    fn at_separator(&self) -> bool {
        self.peek() == Some(&Token::Separator)
    }

    fn skip_separators(&mut self) {
        while self.at_separator() {
            self.at += 1;
        }
    }

    fn expect(&mut self, token: &Token) -> Result<(), String> {
        if self.peek() == Some(token) {
            self.at += 1;
            Ok(())
        } else {
            Err(format!("needs {token:?}, got {:?}", self.peek()))
        }
    }

    fn name(&mut self) -> Result<String, String> {
        match self.peek().cloned() {
            Some(Token::Name(name)) => {
                self.at += 1;
                Ok(name)
            }
            other => Err(format!("needs an output name, got {other:?}")),
        }
    }

    fn keyword(&mut self, word: &str) -> bool {
        if matches!(self.peek(), Some(Token::Name(name)) if name == word) {
            self.at += 1;
            true
        } else {
            false
        }
    }

    fn expression(&mut self) -> Result<Tree, String> {
        self.or(0)
    }

    fn deeper(depth: usize) -> Result<usize, String> {
        if depth >= MAX_DEPTH {
            Err(format!("nests deeper than {MAX_DEPTH} levels"))
        } else {
            Ok(depth + 1)
        }
    }

    fn or(&mut self, depth: usize) -> Result<Tree, String> {
        let mut depth = Self::deeper(depth)?;
        let mut left = self.and(depth)?;
        while self.keyword("or") {
            depth = Self::deeper(depth)?;
            left = Tree::Or(Box::new(left), Box::new(self.and(depth)?));
        }
        Ok(left)
    }

    fn and(&mut self, mut depth: usize) -> Result<Tree, String> {
        let mut left = self.comparison(depth)?;
        while self.keyword("and") {
            depth = Self::deeper(depth)?;
            left = Tree::And(Box::new(left), Box::new(self.comparison(depth)?));
        }
        Ok(left)
    }

    fn comparison(&mut self, depth: usize) -> Result<Tree, String> {
        let left = self.sum(depth)?;
        let op = match self.peek() {
            Some(token) if token.comparison().is_some() => token.clone(),
            _ => return Ok(left),
        };
        self.at += 1;
        let right = self.sum(depth)?;
        if self.peek().is_some_and(|token| token.comparison().is_some()) {
            return Err("chains two comparisons without `and` between them".to_owned());
        }
        Ok(Tree::Binary(op, Box::new(left), Box::new(right)))
    }

    fn sum(&mut self, mut depth: usize) -> Result<Tree, String> {
        let mut left = self.product(depth)?;
        while let Some(token @ (Token::Plus | Token::Minus)) = self.peek().cloned() {
            self.at += 1;
            depth = Self::deeper(depth)?;
            left = Tree::Binary(token, Box::new(left), Box::new(self.product(depth)?));
        }
        Ok(left)
    }

    fn product(&mut self, mut depth: usize) -> Result<Tree, String> {
        let mut left = self.unary(depth)?;
        while let Some(token @ (Token::Star | Token::Slash | Token::Percent)) = self.peek().cloned() {
            self.at += 1;
            depth = Self::deeper(depth)?;
            left = Tree::Binary(token, Box::new(left), Box::new(self.unary(depth)?));
        }
        Ok(left)
    }

    fn unary(&mut self, depth: usize) -> Result<Tree, String> {
        let depth = Self::deeper(depth)?;
        if self.peek() == Some(&Token::Minus) {
            self.at += 1;
            return Ok(Tree::Negate(Box::new(self.unary(depth)?)));
        }
        if self.keyword("not") {
            return Ok(Tree::Not(Box::new(self.unary(depth)?)));
        }
        self.postfix(depth)
    }

    fn postfix(&mut self, mut depth: usize) -> Result<Tree, String> {
        let mut tree = self.primary(depth)?;
        while self.peek() == Some(&Token::Dot) {
            self.at += 1;
            depth = Self::deeper(depth)?;
            let component = self.name()?;
            if component != "x" && component != "y" {
                return Err(format!("names the component '{component}', expected `x` or `y`"));
            }
            tree = Tree::Component(Box::new(tree), component);
        }
        Ok(tree)
    }

    fn primary(&mut self, depth: usize) -> Result<Tree, String> {
        match self.peek().cloned() {
            Some(Token::Number(text)) => {
                self.at += 1;
                Ok(Tree::Number(text))
            }
            Some(Token::Open) => {
                self.at += 1;
                let tree = self.or(depth)?;
                self.expect(&Token::Close)?;
                Ok(tree)
            }
            Some(Token::Name(name)) => {
                self.at += 1;
                match name.as_str() {
                    "true" => return Ok(Tree::Bool(true)),
                    "false" => return Ok(Tree::Bool(false)),
                    "and" | "or" | "not" => return Err(format!("needs a value, got `{name}`")),
                    _ => {}
                }
                if self.peek() != Some(&Token::Open) {
                    return Ok(Tree::Name(name));
                }
                self.at += 1;
                let mut arguments = Vec::new();
                if self.peek() != Some(&Token::Close) {
                    loop {
                        arguments.push(self.or(depth)?);
                        if self.peek() == Some(&Token::Comma) {
                            self.at += 1;
                        } else {
                            break;
                        }
                    }
                }
                self.expect(&Token::Close)?;
                Ok(Tree::Call(name, arguments))
            }
            other => Err(format!("needs a value, got {other:?}")),
        }
    }
}

/// A value while the expansion is built: a typed value, or a number literal that takes its type from beside it.
#[derive(Debug, Clone)]
enum Operand {
    Value(InputValue, ValueType),
    Whole(i64),
    Point(String),
}

/// State of one expansion.
struct Builder<'a> {
    node: &'a Node,
    next: &'a mut u32,
    nodes: Vec<Node>,
    names: BTreeMap<String, Operand>,
}

impl Builder<'_> {
    /// Adds a node of kind `op` with `inputs`, and returns a link to its output `port`.
    fn add(&mut self, op: &str, inputs: Vec<(&str, InputValue)>, port: &str) -> Result<InputValue, String> {
        let id = crate::normal::mint_id(self.next)?;
        let mut node = Node::new(id, op);
        node.region = self.node.region;
        for (name, value) in inputs {
            node.inputs.insert(name.to_owned(), value);
        }
        self.nodes.push(node);
        Ok(InputValue::Link(PortRef {
            node: id,
            port: Some(port.to_owned()),
        }))
    }

    /// Returns `operand` with a type, a whole number as a `u32` and a number with a point as an `f32`.
    fn settle(operand: Operand) -> Result<(InputValue, ValueType), String> {
        match operand {
            Operand::Value(value, value_type) => Ok((value, value_type)),
            Operand::Whole(_) => Self::literal(&operand, &ValueType::U32),
            Operand::Point(_) => Self::literal(&operand, &ValueType::F32),
        }
    }

    /// Returns the number literal `operand` as a value of `value_type`.
    fn literal(operand: &Operand, value_type: &ValueType) -> Result<(InputValue, ValueType), String> {
        let refused = |text: String| format!("cannot write {text} as {value_type}");
        let literal = match (operand, value_type) {
            (Operand::Whole(number), ValueType::U32) => {
                Literal::U32(u32::try_from(*number).map_err(|error| refused(format!("{number} ({error})")))?)
            }
            (Operand::Whole(number), ValueType::I32) => {
                Literal::I32(i32::try_from(*number).map_err(|error| refused(format!("{number} ({error})")))?)
            }
            (Operand::Whole(number), ValueType::U64) => {
                Literal::U64(u64::try_from(*number).map_err(|error| refused(format!("{number} ({error})")))?)
            }
            (Operand::Whole(number), ValueType::F32) => Literal::F32(*number as f32),
            (Operand::Whole(number), ValueType::F64) => Literal::F64(*number as f64),
            (Operand::Point(text), ValueType::F32) => Literal::F32(
                text.parse::<f32>()
                    .ok()
                    .filter(|number| number.is_finite())
                    .ok_or_else(|| refused(text.clone()))?,
            ),
            (Operand::Point(text), ValueType::F64) => Literal::F64(
                text.parse::<f64>()
                    .ok()
                    .filter(|number| number.is_finite())
                    .ok_or_else(|| refused(text.clone()))?,
            ),
            (Operand::Value(value, found), _) if found == value_type => return Ok((value.clone(), found.clone())),
            (Operand::Whole(number), _) => return Err(refused(number.to_string())),
            (Operand::Point(text), _) => return Err(refused(text.clone())),
            (Operand::Value(_, found), _) => return Err(format!("needs {value_type}, got {found}")),
        };
        Ok((InputValue::Literal(literal), value_type.clone()))
    }

    /// Returns two operands of one type, a number literal taking the scalar type of the other operand.
    fn pair(left: &Operand, right: &Operand) -> Result<[(InputValue, ValueType); 2], String> {
        let scalar = |value_type: &ValueType| component_of(value_type).unwrap_or_else(|| value_type.clone());
        match (left, right) {
            (Operand::Value(a, a_type), Operand::Value(b, b_type)) => {
                Ok([(a.clone(), a_type.clone()), (b.clone(), b_type.clone())])
            }
            (Operand::Value(_, known), _) => {
                let known = known.clone();
                Ok([Self::literal(left, &known)?, Self::literal(right, &scalar(&known))?])
            }
            (_, Operand::Value(_, known)) => {
                let known = known.clone();
                Ok([Self::literal(left, &scalar(&known))?, Self::literal(right, &known)?])
            }
            _ => {
                let both = if matches!(left, Operand::Point(_)) || matches!(right, Operand::Point(_)) {
                    ValueType::F32
                } else {
                    ValueType::U32
                };
                Ok([Self::literal(left, &both)?, Self::literal(right, &both)?])
            }
        }
    }

    /// Returns the expansion of `tree`.
    fn build(&mut self, tree: &Tree) -> Result<Operand, String> {
        Ok(match tree {
            Tree::Number(text) => {
                if text.contains(['.', 'e', 'E']) {
                    Operand::Point(text.clone())
                } else {
                    Operand::Whole(
                        text.parse()
                            .map_err(|error| format!("cannot read the number {text}: {error}"))?,
                    )
                }
            }
            Tree::Bool(flag) => Operand::Value(InputValue::Literal(Literal::Bool(*flag)), ValueType::Bool),
            Tree::Name(name) => self
                .names
                .get(name)
                .cloned()
                .ok_or_else(|| format!("reads '{name}', which is not an input or an earlier output"))?,
            Tree::Negate(inner) => match self.build(inner)? {
                Operand::Whole(number) => Operand::Whole(-number),
                Operand::Point(text) => Operand::Point(format!("-{text}")),
                value @ Operand::Value(..) => {
                    let (value, value_type) = Self::settle(value)?;
                    Operand::Value(self.add("math.neg", vec![("a", value)], "out")?, value_type)
                }
            },
            Tree::Not(inner) => {
                let operand = self.build(inner)?;
                let (value, _) = Self::literal(&operand, &ValueType::Bool)?;
                Operand::Value(self.add("logic.not", vec![("a", value)], "out")?, ValueType::Bool)
            }
            Tree::And(left, right) | Tree::Or(left, right) => {
                let (left, right) = (self.build(left)?, self.build(right)?);
                let (left, _) = Self::literal(&left, &ValueType::Bool)?;
                let (right, _) = Self::literal(&right, &ValueType::Bool)?;
                let op = if matches!(tree, Tree::And(..)) {
                    "logic.and"
                } else {
                    "logic.or"
                };
                Operand::Value(self.add(op, vec![("a", left), ("b", right)], "out")?, ValueType::Bool)
            }
            Tree::Binary(token, left, right) => self.binary(token, left, right)?,
            Tree::Component(inner, component) => {
                let operand = self.build(inner)?;
                let (value, value_type) = Self::settle(operand)?;
                let scalar =
                    component_of(&value_type).ok_or_else(|| format!("reads `.{component}` of {value_type}"))?;
                Operand::Value(self.add("vec.split", vec![("v", value)], component)?, scalar)
            }
            Tree::Call(name, arguments) => self.call(name, arguments)?,
        })
    }

    /// Returns the expansion of a binary operator.
    fn binary(&mut self, token: &Token, left: &Tree, right: &Tree) -> Result<Operand, String> {
        let (left, right) = (self.build(left)?, self.build(right)?);
        if let Some(op) = token.comparison() {
            let [(a, a_type), (b, b_type)] = Self::pair(&left, &right)?;
            if a_type != b_type {
                return Err(format!("compares {a_type} with {b_type}"));
            }
            return Ok(Operand::Value(
                self.add(op, vec![("a", a), ("b", b)], "out")?,
                ValueType::Bool,
            ));
        }
        let [(a, a_type), (b, b_type)] = Self::pair(&left, &right)?;
        let op = match token {
            Token::Plus => "math.add",
            Token::Minus => "math.sub",
            Token::Star => "math.mul",
            Token::Slash => "math.div",
            _ => "math.rem",
        };
        if a_type == b_type {
            return Ok(Operand::Value(self.add(op, vec![("a", a), ("b", b)], "out")?, a_type));
        }
        // A `vec2f` times or by an `f32` scales it.
        match (token, &a_type, &b_type) {
            (Token::Star, ValueType::Vec2F, ValueType::F32) => Ok(Operand::Value(
                self.add("vec.scale", vec![("v", a), ("s", b)], "out")?,
                ValueType::Vec2F,
            )),
            (Token::Star, ValueType::F32, ValueType::Vec2F) => Ok(Operand::Value(
                self.add("vec.scale", vec![("v", b), ("s", a)], "out")?,
                ValueType::Vec2F,
            )),
            (Token::Slash, ValueType::Vec2F, ValueType::F32) => {
                let both = self.add("vec.make", vec![("x", b.clone()), ("y", b)], "out")?;
                Ok(Operand::Value(
                    self.add("math.div", vec![("a", a), ("b", both)], "out")?,
                    ValueType::Vec2F,
                ))
            }
            _ => Err(format!("has mismatched types {a_type} and {b_type}")),
        }
    }

    /// Returns the expansion of the call `name` of `arguments`.
    fn call(&mut self, name: &str, arguments: &[Tree]) -> Result<Operand, String> {
        let mut operands = Vec::with_capacity(arguments.len());
        for argument in arguments {
            operands.push(self.build(argument)?);
        }
        let unary = match name {
            "abs" => Some("math.abs"),
            "floor" => Some("math.floor"),
            "ceil" => Some("math.ceil"),
            "trunc" => Some("math.trunc"),
            "round" => Some("math.round"),
            "sqrt" => Some("math.sqrt"),
            "recip" => Some("math.recip"),
            "exp" => Some("math.exp"),
            "log" => Some("math.log"),
            "log2" => Some("math.log2"),
            "log10" => Some("math.log10"),
            "sin" => Some("math.sin"),
            "cos" => Some("math.cos"),
            _ => None,
        };
        if let Some(op) = unary {
            let [operand] = fixed_operands(name, operands)?;
            let (value, value_type) = match operand {
                Operand::Whole(number) => Self::literal(&Operand::Whole(number), &ValueType::F32)?,
                other => Self::settle(other)?,
            };
            return Ok(Operand::Value(self.add(op, vec![("a", value)], "out")?, value_type));
        }
        match name {
            "min" | "max" | "pow" | "dot" => {
                let [left, right] = fixed_operands(name, operands)?;
                let [(a, a_type), (b, _)] = Self::pair(&left, &right)?;
                Ok(match name {
                    "min" => Operand::Value(self.add("math.min", vec![("a", a), ("b", b)], "out")?, a_type),
                    "max" => Operand::Value(self.add("math.max", vec![("a", a), ("b", b)], "out")?, a_type),
                    "pow" => Operand::Value(self.add("math.pow", vec![("base", a), ("exponent", b)], "out")?, a_type),
                    _ => Operand::Value(self.add("vec.dot", vec![("a", a), ("b", b)], "out")?, ValueType::F32),
                })
            }
            "clamp" => {
                let [x, lo, hi] = fixed_operands(name, operands)?;
                let (x, x_type) = Self::settle(x)?;
                let (lo, _) = Self::literal(&lo, &x_type)?;
                let (hi, _) = Self::literal(&hi, &x_type)?;
                Ok(Operand::Value(
                    self.add("math.clamp", vec![("x", x), ("lo", lo), ("hi", hi)], "out")?,
                    x_type,
                ))
            }
            "if" => {
                let [cond, if_true, if_false] = fixed_operands(name, operands)?;
                let (cond, _) = Self::literal(&cond, &ValueType::Bool)?;
                let [(if_true, value_type), (if_false, _)] = Self::pair(&if_true, &if_false)?;
                Ok(Operand::Value(
                    self.add(
                        "logic.select",
                        vec![("cond", cond), ("if_true", if_true), ("if_false", if_false)],
                        "out",
                    )?,
                    value_type,
                ))
            }
            "vec" => {
                let [x, y] = fixed_operands(name, operands)?;
                let [(x, scalar), (y, _)] = Self::pair(&x, &y)?;
                let vector = vector_of(&scalar).ok_or_else(|| format!("makes a vector of {scalar}"))?;
                Ok(Operand::Value(
                    self.add("vec.make", vec![("x", x), ("y", y)], "out")?,
                    vector,
                ))
            }
            "length" => {
                let [vector] = fixed_operands(name, operands)?;
                let (value, _) = Self::literal(&vector, &ValueType::Vec2F)?;
                Ok(Operand::Value(
                    self.add("vec.length", vec![("v", value)], "out")?,
                    ValueType::F32,
                ))
            }
            _ => Err(format!("calls the unknown function `{name}`")),
        }
    }
}

/// Returns `operands` as an array, for a call of `name` that takes `N` arguments.
///
/// # Errors
///
/// Returns the message for a call with another number of arguments.
fn fixed_operands<const N: usize>(name: &str, operands: Vec<Operand>) -> Result<[Operand; N], String> {
    <[Operand; N]>::try_from(operands).map_err(|operands| {
        format!(
            "calls `{name}` with {}, expected {N}",
            crate::model_text::counted(operands.len(), "argument")
        )
    })
}
